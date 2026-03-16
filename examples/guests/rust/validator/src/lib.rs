//! rust_validator — Rust native plugin implementing pipeline.Validator v1.0.
//!
//! Contract: validate(data: StringView) -> StringView
//! Input:  "DECODED:name|value|42"
//! Output: "VALID:ok" or "INVALID:reason"

// ─── ABI Types (mirrored from polyplug) ──────────────────────────────────────
// We cannot depend on polyplug here (cdylib circular dependency).
// Mirror the ABI types inline. These are frozen per §7 ABI Stability.

use std::sync::OnceLock;

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

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PluginHandle {
    pub index: u32,
    pub generation: u32,
}

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

#[repr(C)]
pub struct PluginRegistrar {
    pub register_plugin: unsafe extern "C" fn(
        registrar: *mut PluginRegistrar,
        descriptor: *const PluginDescriptor,
        vtable: *const PluginVTable,
    ) -> AbiError,
    pub host: *const HostVTable,
}

// ─── Function pointer newtype wrapper ────────────────────────────────────────

#[repr(transparent)]
pub struct FnPtr(pub *const ());

// SAFETY: FnPtr wraps a 'static function pointer. Function pointers are safe
// to share across threads — the function itself handles its own synchronization.
unsafe impl Send for FnPtr {}
// SAFETY: Function pointers are inherently Sync — multiple threads may call the
// same function concurrently. The underlying data is read-only 'static memory.
unsafe impl Sync for FnPtr {}

// ─── Trace Extension (mirrored layout) ───────────────────────────────────────

const EXT_TRACE_ID: u32 = 0xC4EB9AEE_u32;

#[repr(C)]
pub struct TraceVTable {
    pub emit: unsafe extern "C" fn(msg: StringView, state: *const ()),
    pub state: *const (),
}

// SAFETY: TraceVTable fields are a function pointer and a *const () to a leaked allocation.
unsafe impl Send for TraceVTable {}
// SAFETY: Same reasoning — no mutable state, concurrent reads are safe.
unsafe impl Sync for TraceVTable {}

// ─── Contract ID ──────────────────────────────────────────────────────────────
// FNV1a-64 of "pipeline.Validator@1" = 0xA553FAB5D11C7AF0

const VALIDATOR_CONTRACT_ID: u64 = 0xA553FAB5D11C7AF0;

// ─── Trace callback storage ─────────────────────────────────────────────────

#[derive(Clone, Copy)]
struct TracePtr(*const TraceVTable);

// SAFETY: TracePtr wraps a pointer to a leaked host allocation that is
// immutable after construction. Reading it from multiple threads is safe.
unsafe impl Send for TracePtr {}
// SAFETY: Same reasoning — no mutable state, concurrent reads are safe.
unsafe impl Sync for TracePtr {}

static TRACE_CB: OnceLock<Option<TracePtr>> = OnceLock::new();

/// # Safety
/// Reads the TRACE_CB OnceLock which stores a potentially-null pointer.
/// The pointer is valid for the lifetime of the runtime (host guarantees this).
unsafe fn emit_trace(msg: &str) {
    if let Some(Some(TracePtr(vtable_ptr))) = TRACE_CB.get() {
        // SAFETY: vtable_ptr was obtained from get_extension(EXT_TRACE_ID) and is
        // guaranteed non-null here. It points to a valid TraceVTable for the lifetime
        // of the host runtime. msg is a valid UTF-8 &str for this call's duration.
        let sv = StringView {
            ptr: msg.as_ptr(),
            len: msg.len(),
        };
        unsafe { ((**vtable_ptr).emit)(sv, (**vtable_ptr).state) };
    }
}

// ─── Implementation ───────────────────────────────────────────────────────────
// Contract: validate(data: StringView) -> StringView
// Input:  "DECODED:name|value|42"
// Output: "VALID:ok" or "INVALID:reason"

/// # Safety
/// `args` must point to a valid `StringView`. `out` must point to a valid `StringView`.
extern "C" fn plugin_validate(args: *const (), out: *mut ()) -> AbiError {
    // SAFETY: The host runtime guarantees args points to a valid StringView
    // and out points to a valid StringView. Enforced by the ABI contract.
    unsafe { validate_impl(args as *const StringView, out as *mut StringView) }
}

/// # Safety
/// `input` must be a non-null pointer to a valid `StringView`.
/// `out` must be a non-null pointer to a valid `StringView` writable by caller.
unsafe fn validate_impl(input: *const StringView, out: *mut StringView) -> AbiError {
    // SAFETY: input is non-null and valid per ABI contract from plugin_validate.
    let sv: &StringView = unsafe { &*input };

    // SAFETY: sv.ptr points to valid UTF-8 bytes for sv.len bytes — guaranteed by host.
    let input_bytes: &[u8] = if sv.ptr.is_null() || sv.len == 0 {
        b""
    } else {
        unsafe { core::slice::from_raw_parts(sv.ptr, sv.len) }
    };

    let input_str: &str = match core::str::from_utf8(input_bytes) {
        Ok(s) => s,
        Err(_) => {
            return write_result(out, "INVALID:input is not valid UTF-8");
        }
    };

    let result_str: &str = validate_decoded_format(input_str);
    let write_err: AbiError = write_result(out, result_str);

    // SAFETY: emit_trace reads a stable OnceLock — no unsoundness.
    unsafe { emit_trace("[rust_validator] validate called") };

    write_err
}

fn validate_decoded_format(input: &str) -> &'static str {
    let payload: &str = match input.strip_prefix("DECODED:") {
        Some(p) => p,
        None => return "INVALID:missing DECODED: prefix",
    };

    let parts: Vec<&str> = payload.split('|').collect();
    if parts.len() != 3 {
        return "INVALID:expected exactly 3 pipe-separated fields";
    }

    if parts[0].is_empty() {
        return "INVALID:name field is empty";
    }

    if parts[1].is_empty() {
        return "INVALID:value field is empty";
    }

    if parts[2].parse::<i64>().is_err() {
        return "INVALID:third field is not a valid integer";
    }

    "VALID:ok"
}

fn write_result(out: *mut StringView, msg: &str) -> AbiError {
    let result_bytes: Vec<u8> = msg.as_bytes().to_vec();
    let leaked: &'static [u8] = Box::leak(result_bytes.into_boxed_slice());

    let result_sv = StringView {
        ptr: leaked.as_ptr(),
        len: leaked.len(),
    };

    // SAFETY: out is non-null and points to a valid StringView per ABI contract.
    unsafe {
        core::ptr::write(out, result_sv);
    }

    AbiError::ok()
}

// ─── Static VTable ────────────────────────────────────────────────────────────

static VALIDATOR_FNS: [FnPtr; 1] = [FnPtr(plugin_validate as *const ())];

static VALIDATOR_VTABLE: PluginVTable = PluginVTable {
    contract_id: VALIDATOR_CONTRACT_ID,
    contract_version: 0u32,
    function_count: 1,
    functions: VALIDATOR_FNS.as_ptr(),
};

static VALIDATOR_DESCRIPTOR: PluginDescriptor = PluginDescriptor {
    name: StringView {
        ptr: b"rust_validator".as_ptr(),
        len: 14,
    },
    contract_name: StringView {
        ptr: b"pipeline.Validator".as_ptr(),
        len: 18,
    },
    version_major: 1,
    version_minor: 0,
    version_patch: 0,
};

// ─── ABI Exports ─────────────────────────────────────────────────────────────

#[unsafe(no_mangle)]
pub extern "C" fn polyplug_abi_version() -> u32 {
    1
}

#[repr(C)]
pub struct PluginContext {
    pub bundle_path: StringView,
}

/// # Safety
/// `registrar` must be a valid non-null pointer to a PluginRegistrar from the host.
/// `ctx` must be a valid non-null pointer to a PluginContext from the host.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_init(
    registrar: *mut PluginRegistrar,
    ctx: *const PluginContext,
) -> AbiError {
    if registrar.is_null() {
        return AbiError {
            code: 1,
            message: StringView::null(),
        };
    }
    if ctx.is_null() {
        return AbiError {
            code: 1,
            message: StringView::null(),
        };
    }

    // SAFETY: ctx is non-null and valid per ABI contract.
    let _bundle_path: StringView = unsafe { (*ctx).bundle_path };

    // SAFETY: registrar is non-null, so host field is valid per ABI contract.
    let host: *const HostVTable = unsafe { (*registrar).host };
    if !host.is_null() {
        // SAFETY: host is non-null and valid per ABI contract.
        let ext_ptr: *const () = unsafe { ((*host).get_extension)(EXT_TRACE_ID) };
        let trace_vtable: Option<TracePtr> = if ext_ptr.is_null() {
            None
        } else {
            Some(TracePtr(ext_ptr as *const TraceVTable))
        };
        let _: Result<(), _> = TRACE_CB.set(trace_vtable);
    } else {
        let _: Result<(), _> = TRACE_CB.set(None);
    }

    // SAFETY: emit_trace reads a stable OnceLock — no unsoundness.
    unsafe { emit_trace("[rust_validator] init") };

    // SAFETY: registrar is non-null and provided by the host runtime per ABI contract.
    let reg: &mut PluginRegistrar = unsafe { &mut *registrar };

    // SAFETY: register_plugin is a valid function pointer set by the host.
    unsafe {
        (reg.register_plugin)(
            registrar,
            &VALIDATOR_DESCRIPTOR as *const PluginDescriptor,
            &VALIDATOR_VTABLE as *const PluginVTable,
        )
    }
}
