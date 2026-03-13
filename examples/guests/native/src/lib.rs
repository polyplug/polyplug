//! string_transformer — native Rust plugin implementing pipeline.transformer v1.0.
//!
//! This is a hand-written cdylib plugin showing how to write a native plugin
//! without using polyplugc for code generation.
//!
//! Contract: pipeline.transformer
//! Function: transform(record: DataRecord) -> DataRecord
//!
//! This transformer uppercases the `name` field and appends " (transformed)" to `value`.
//!
//! ## How to build
//!
//! ```bash
//! cargo build --release
//! cp target/release/libstring_transformer.so .
//! ```

// ─── ABI Types (mirrored from polyplug) ──────────────────────────────────────
// We cannot depend on polyplug here (cdylib circular dependency).
// Mirror the ABI types inline. These are frozen per §7 ABI Stability.

/// Non-owning UTF-8 string view — mirrors polyplug::abi::StringView.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct StringView {
    pub ptr: *const u8,
    pub len: usize,
}

impl StringView {
    pub const fn null() -> StringView {
        StringView {
            ptr: core::ptr::null(),
            len: 0,
        }
    }
}

// SAFETY: StringView is a read-only view into externally-owned 'static data.
// Concurrent reads from multiple threads are safe.
unsafe impl Send for StringView {}
// SAFETY: Same reasoning as Send.
unsafe impl Sync for StringView {}

/// ABI error — mirrors polyplug::abi::AbiError.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AbiError {
    pub code: u32,
    pub message: StringView,
}

impl AbiError {
    pub const fn ok() -> AbiError {
        AbiError {
            code: 0,
            message: StringView::null(),
        }
    }
}

// SAFETY: AbiError is a plain-old-data struct with no interior mutability.
unsafe impl Send for AbiError {}
// SAFETY: AbiError is a plain-old-data struct with no interior mutability.
unsafe impl Sync for AbiError {}

/// Wrapper for a function pointer stored in a static vtable array.
/// The pointer is 'static (lifetime of the plugin binary) and read-only.
#[repr(transparent)]
pub struct FnPtr(pub *const ());

// SAFETY: FnPtr wraps a 'static function pointer. Function pointers are safe
// to share across threads — the function itself handles its own synchronization.
unsafe impl Send for FnPtr {}
// SAFETY: Function pointers are inherently Sync — multiple threads may call the
// same function concurrently. The underlying data is read-only 'static memory.
unsafe impl Sync for FnPtr {}

/// Plugin VTable — mirrors polyplug::abi::PluginVTable.
#[repr(C)]
pub struct PluginVTable {
    pub contract_id: u64,
    pub contract_version: u32,
    pub function_count: u32,
    pub functions: *const FnPtr,
}

// SAFETY: PluginVTable points to 'static function arrays. All fields are static pointers.
unsafe impl Send for PluginVTable {}
// SAFETY: PluginVTable points to 'static function arrays. All fields are static pointers.
unsafe impl Sync for PluginVTable {}

/// Plugin descriptor — mirrors polyplug::abi::PluginDescriptor.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PluginDescriptor {
    pub name: StringView,
    pub contract_name: StringView,
    pub version_major: u32,
    pub version_minor: u32,
    pub version_patch: u32,
}

// SAFETY: PluginDescriptor contains only StringViews and u32 values.
unsafe impl Send for PluginDescriptor {}
// SAFETY: PluginDescriptor contains only StringViews and u32 values.
unsafe impl Sync for PluginDescriptor {}

/// Opaque handle to a loaded plugin — mirrors polyplug::abi::PluginHandle.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PluginHandle {
    pub index: u32,
    pub generation: u32,
}

/// Host VTable — mirrors polyplug::abi::HostVTable.
#[repr(C)]
pub struct HostVTable {
    pub alloc: unsafe extern "C" fn(size: usize, align: usize) -> *mut u8,
    pub free: unsafe extern "C" fn(ptr: *mut u8, size: usize, align: usize),
    pub find_by_contract: unsafe extern "C" fn(contract_id: u64, min_version: u32) -> PluginHandle,
    pub find_by_bundle:
        unsafe extern "C" fn(bundle_id: u64, contract_id: u64, min_version: u32) -> PluginHandle,
    pub find_all_by_contract: unsafe extern "C" fn(
        contract_id: u64,
        min_version: u32,
        out: *mut PluginHandle,
        out_cap: usize,
    ) -> usize,
    pub resolve_plugin: unsafe extern "C" fn(handle: PluginHandle) -> *const PluginVTable,
    pub get_extension: unsafe extern "C" fn(extension_id: u32) -> *const (),
}

/// Plugin registrar — mirrors polyplug::abi::PluginRegistrar.
#[repr(C)]
pub struct PluginRegistrar {
    pub register_plugin: unsafe extern "C" fn(
        registrar: *mut PluginRegistrar,
        descriptor: *const PluginDescriptor,
        vtable: *const PluginVTable,
    ) -> AbiError,
    pub host: *const HostVTable,
}

// ─── Contract ─────────────────────────────────────────────────────────────────
// FNV1a-64 of "pipeline.transformer" at major version 1.
// From examples/contract_ids.txt: TRANSFORMER_CONTRACT_ID = 0x0E3044133E12EB05

const TRANSFORMER_CONTRACT_ID: u64 = 0x0E3044133E12EB05;

// ─── pipeline.transformer contract types ─────────────────────────────────────

/// Shared data record flowing through the pipeline.
#[repr(C)]
pub struct DataRecord {
    pub name: StringView,
    pub value: StringView,
    pub count: u32,
}

// ─── Implementation ───────────────────────────────────────────────────────────

/// The `transform` function: uppercases `name` and appends " (transformed)" to `value`.
///
/// Signature matches the generic dispatch: `extern "C" fn(*const (), *mut ()) -> AbiError`.
///
/// # Safety
/// `args` must point to a valid `DataRecord`. `out` must point to a valid `DataRecord`.
extern "C" fn plugin_transform(args: *const (), out: *mut ()) -> AbiError {
    // SAFETY: The host runtime guarantees args points to a valid DataRecord
    // and out points to a valid DataRecord. Enforced by the ABI contract.
    unsafe { transform_impl(args as *const DataRecord, out as *mut DataRecord) }
}

/// Core transform implementation operating on typed pointers.
///
/// # Safety
/// `input` must be a non-null pointer to a valid `DataRecord`.
/// `out` must be a non-null pointer to a valid `DataRecord` writable by caller.
unsafe fn transform_impl(input: *const DataRecord, out: *mut DataRecord) -> AbiError {
    // SAFETY: input is non-null and valid per ABI contract from plugin_transform.
    let record: &DataRecord = unsafe { &*input };

    // SAFETY: name.ptr points to valid UTF-8 bytes for name.len bytes — guaranteed by host.
    let name_bytes: &[u8] =
        unsafe { core::slice::from_raw_parts(record.name.ptr, record.name.len) };
    let name_str: &str = match core::str::from_utf8(name_bytes) {
        Ok(s) => s,
        Err(_) => {
            return AbiError {
                code: 1,
                message: StringView {
                    ptr: b"invalid UTF-8 in name".as_ptr(),
                    len: 21,
                },
            };
        }
    };

    // SAFETY: value.ptr points to valid UTF-8 bytes for value.len bytes — guaranteed by host.
    let value_bytes: &[u8] =
        unsafe { core::slice::from_raw_parts(record.value.ptr, record.value.len) };
    let value_str: &str = match core::str::from_utf8(value_bytes) {
        Ok(s) => s,
        Err(_) => {
            return AbiError {
                code: 1,
                message: StringView {
                    ptr: b"invalid UTF-8 in value".as_ptr(),
                    len: 22,
                },
            };
        }
    };

    let transformed_name: String = name_str.to_uppercase();
    let transformed_value: String = format!("{} (transformed)", value_str);

    let name_leaked: &'static [u8] = Box::leak(transformed_name.into_bytes().into_boxed_slice());
    let value_leaked: &'static [u8] = Box::leak(transformed_value.into_bytes().into_boxed_slice());

    let result = DataRecord {
        name: StringView {
            ptr: name_leaked.as_ptr(),
            len: name_leaked.len(),
        },
        value: StringView {
            ptr: value_leaked.as_ptr(),
            len: value_leaked.len(),
        },
        count: record.count,
    };

    // SAFETY: out is non-null and points to a valid DataRecord per ABI contract.
    unsafe {
        core::ptr::write(out, result);
    }

    AbiError::ok()
}

// ─── Static VTable ────────────────────────────────────────────────────────────

static TRANSFORMER_FNS: [FnPtr; 1] = [FnPtr(plugin_transform as *const ())];

static TRANSFORMER_VTABLE: PluginVTable = PluginVTable {
    contract_id: TRANSFORMER_CONTRACT_ID,
    contract_version: 0u32,
    function_count: 1,
    functions: TRANSFORMER_FNS.as_ptr(),
};

static TRANSFORMER_DESCRIPTOR: PluginDescriptor = PluginDescriptor {
    name: StringView {
        ptr: b"string_transformer".as_ptr(),
        len: 18,
    },
    contract_name: StringView {
        ptr: b"pipeline.transformer".as_ptr(),
        len: 20,
    },
    version_major: 1,
    version_minor: 0,
    version_patch: 0,
};

// ─── ABI Exports ─────────────────────────────────────────────────────────────

/// ABI version sentinel — loader checks this before calling polyplug_init.
#[unsafe(no_mangle)]
pub extern "C" fn polyplug_abi_version() -> u32 {
    1
}

/// Plugin init — called by the loader to register vtables.
///
/// # Safety
/// `registrar` must be a valid non-null pointer to a PluginRegistrar from the host.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_init(registrar: *mut PluginRegistrar) -> AbiError {
    if registrar.is_null() {
        return AbiError {
            code: 1,
            message: StringView::null(),
        };
    }

    // SAFETY: registrar is non-null and provided by the host runtime per ABI contract.
    let reg: &mut PluginRegistrar = unsafe { &mut *registrar };

    // SAFETY: register_plugin is a valid function pointer set by the host.
    // TRANSFORMER_DESCRIPTOR and TRANSFORMER_VTABLE are 'static.
    unsafe {
        (reg.register_plugin)(
            registrar,
            &TRANSFORMER_DESCRIPTOR as *const PluginDescriptor,
            &TRANSFORMER_VTABLE as *const PluginVTable,
        )
    }
}
