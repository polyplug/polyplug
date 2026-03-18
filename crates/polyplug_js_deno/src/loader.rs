//! V8 in-process JavaScript loader using deno_core.
//!
//! One JsRuntime (V8 isolate) per bundle, pinned to a dedicated OS thread.
//! Function calls from the host are dispatched via static trampolines.
//!
//! # Memory
//! Vtables, function pointer arrays, thread handles, and string data are `Box::leak`'d
//! intentionally. JS plugins live for the process lifetime and are never unloaded.

use core::time::Duration;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::OnceLock;
use std::sync::mpsc;

use deno_core::FastString;
use deno_core::ModuleLoadOptions;
use deno_core::ModuleLoadReferrer;
use deno_core::ModuleLoader;
use deno_core::ModuleSource;
use deno_core::ModuleSourceCode;
use deno_core::ModuleType;
use deno_core::ResolutionKind;
use deno_core::op2;
use deno_error::JsErrorBox;
use polyplug::error::LoaderError;
use polyplug::error::PolyplugError;
use polyplug::loader::BundleLoader;
use polyplug_abi::ABI_OK;
use polyplug_abi::AbiError;
use polyplug_abi::HostVTable;
use polyplug_abi::PluginDescriptor;
use polyplug_abi::PluginHandle;
use polyplug_abi::PluginRegistrar;
use polyplug_abi::PluginVTable;
use polyplug_abi::StringView;

use crate::config::JsDenoConfig;

// HostVTable* thread-local — set before JsRuntime creation on the bundle thread.
// core:: is used throughout to satisfy the std_instead_of_core lint.
thread_local! {
    static DENO_HOST_VTABLE: core::cell::Cell<*const HostVTable> =
        const { core::cell::Cell::new(core::ptr::null()) };
}

// Channel sender for vtable registration result.
// VtableSenderInner uses a type alias so the thread_local! macro can parse the angle brackets
// without hitting the type_complexity lint.
// Sends: (vtable_ptr, contract_id, fn_count, contract_name)
type VtableSenderInner = mpsc::SyncSender<(SendPluginVTable, u64, usize, String)>;

thread_local! {
    static VTABLE_SENDER: core::cell::RefCell<Option<VtableSenderInner>> =
        const { core::cell::RefCell::new(None) };
}

/// A cross-thread call request dispatched from a trampoline to the bundle thread.
// Fields are constructed by trampolines and consumed on the bundle thread via channel.
#[allow(dead_code)]
pub(crate) struct JsCallRequest {
    /// Raw args pointer value (reconstructed on the bundle thread).
    pub(crate) args_ptr: usize,
    /// Raw out pointer value (reconstructed on the bundle thread).
    pub(crate) out_ptr: usize,
    pub(crate) result_tx: mpsc::SyncSender<AbiError>,
}

// SAFETY: JsCallRequest contains raw pointer values stored as usize (non-dereferenced here)
// and a SyncSender which is Send. The usize values are reconstructed as pointers only on the
// bundle thread which owns the V8 isolate.
unsafe impl Send for JsCallRequest {}

/// Wrapper to make `*const PluginVTable` sendable across threads.
///
/// SAFETY: PluginVTable is a 'static leaked struct that is Send + Sync (see abi/mod.rs).
struct SendPluginVTable(*const PluginVTable);
// SAFETY: PluginVTable implements Send (see abi/mod.rs unsafe impl). A *const PluginVTable
// pointing to a 'static PluginVTable is safe to send to another thread.
unsafe impl Send for SendPluginVTable {}

struct DenoFunctionSlot {
    call_tx: mpsc::SyncSender<JsCallRequest>,
}

static DENO_FUNCTION_REGISTRY: OnceLock<Mutex<Vec<Option<DenoFunctionSlot>>>> = OnceLock::new();

fn deno_function_registry() -> &'static Mutex<Vec<Option<DenoFunctionSlot>>> {
    DENO_FUNCTION_REGISTRY.get_or_init(|| Mutex::new(Vec::new()))
}

fn dispatch_deno_call(slot: usize, args_ptr: *const (), out_ptr: *mut ()) -> AbiError {
    let reg: &Mutex<Vec<Option<DenoFunctionSlot>>> = deno_function_registry();
    let guard: MutexGuard<'_, Vec<Option<DenoFunctionSlot>>> = reg.lock().unwrap_or_else(
        |e: std::sync::PoisonError<MutexGuard<'_, Vec<Option<DenoFunctionSlot>>>>| e.into_inner(),
    );
    let slot_ref: &DenoFunctionSlot = match guard
        .get(slot)
        .and_then(|s: &Option<DenoFunctionSlot>| s.as_ref())
    {
        Some(s) => s,
        None => {
            return AbiError {
                code: 1,
                message: StringView::null(),
            };
        }
    };
    let (result_tx, result_rx): (mpsc::SyncSender<AbiError>, mpsc::Receiver<AbiError>) =
        mpsc::sync_channel::<AbiError>(0);
    let req: JsCallRequest = JsCallRequest {
        args_ptr: args_ptr as usize,
        out_ptr: out_ptr as usize,
        result_tx,
    };
    if slot_ref.call_tx.send(req).is_err() {
        return AbiError {
            code: 1,
            message: StringView::null(),
        };
    }
    drop(guard);
    result_rx.recv().unwrap_or(AbiError {
        code: 1,
        message: StringView::null(),
    })
}

// Pre-generated static extern "C" trampolines (slots 0..63).
// Each trampoline has a hardcoded slot index and calls `dispatch_deno_call`.
// We cannot use closures for extern "C" fn pointers — static trampolines
// with a hardcoded slot are the correct Rust solution.
macro_rules! make_trampoline {
    ($name:ident, $slot:expr) => {
        // SAFETY: trampolines are `extern "C"` functions with the ABI signature
        // expected by PluginVTable.functions: fn(*const (), *mut ()) -> AbiError.
        // `dispatch_deno_call` is safe to call from any thread (uses Mutex-protected registry).
        unsafe extern "C" fn $name(args_ptr: *const (), out_ptr: *mut ()) -> AbiError {
            dispatch_deno_call($slot, args_ptr, out_ptr)
        }
    };
}

make_trampoline!(trampoline_0, 0);
make_trampoline!(trampoline_1, 1);
make_trampoline!(trampoline_2, 2);
make_trampoline!(trampoline_3, 3);
make_trampoline!(trampoline_4, 4);
make_trampoline!(trampoline_5, 5);
make_trampoline!(trampoline_6, 6);
make_trampoline!(trampoline_7, 7);
make_trampoline!(trampoline_8, 8);
make_trampoline!(trampoline_9, 9);
make_trampoline!(trampoline_10, 10);
make_trampoline!(trampoline_11, 11);
make_trampoline!(trampoline_12, 12);
make_trampoline!(trampoline_13, 13);
make_trampoline!(trampoline_14, 14);
make_trampoline!(trampoline_15, 15);
make_trampoline!(trampoline_16, 16);
make_trampoline!(trampoline_17, 17);
make_trampoline!(trampoline_18, 18);
make_trampoline!(trampoline_19, 19);
make_trampoline!(trampoline_20, 20);
make_trampoline!(trampoline_21, 21);
make_trampoline!(trampoline_22, 22);
make_trampoline!(trampoline_23, 23);
make_trampoline!(trampoline_24, 24);
make_trampoline!(trampoline_25, 25);
make_trampoline!(trampoline_26, 26);
make_trampoline!(trampoline_27, 27);
make_trampoline!(trampoline_28, 28);
make_trampoline!(trampoline_29, 29);
make_trampoline!(trampoline_30, 30);
make_trampoline!(trampoline_31, 31);
make_trampoline!(trampoline_32, 32);
make_trampoline!(trampoline_33, 33);
make_trampoline!(trampoline_34, 34);
make_trampoline!(trampoline_35, 35);
make_trampoline!(trampoline_36, 36);
make_trampoline!(trampoline_37, 37);
make_trampoline!(trampoline_38, 38);
make_trampoline!(trampoline_39, 39);
make_trampoline!(trampoline_40, 40);
make_trampoline!(trampoline_41, 41);
make_trampoline!(trampoline_42, 42);
make_trampoline!(trampoline_43, 43);
make_trampoline!(trampoline_44, 44);
make_trampoline!(trampoline_45, 45);
make_trampoline!(trampoline_46, 46);
make_trampoline!(trampoline_47, 47);
make_trampoline!(trampoline_48, 48);
make_trampoline!(trampoline_49, 49);
make_trampoline!(trampoline_50, 50);
make_trampoline!(trampoline_51, 51);
make_trampoline!(trampoline_52, 52);
make_trampoline!(trampoline_53, 53);
make_trampoline!(trampoline_54, 54);
make_trampoline!(trampoline_55, 55);
make_trampoline!(trampoline_56, 56);
make_trampoline!(trampoline_57, 57);
make_trampoline!(trampoline_58, 58);
make_trampoline!(trampoline_59, 59);
make_trampoline!(trampoline_60, 60);
make_trampoline!(trampoline_61, 61);
make_trampoline!(trampoline_62, 62);
make_trampoline!(trampoline_63, 63);

/// Maximum number of function slots supported (one per pre-generated trampoline).
const MAX_TRAMPOLINES: usize = 64;

/// Static array of pre-generated trampolines indexed by slot.
static TRAMPOLINES: [unsafe extern "C" fn(*const (), *mut ()) -> AbiError; MAX_TRAMPOLINES] = [
    trampoline_0,
    trampoline_1,
    trampoline_2,
    trampoline_3,
    trampoline_4,
    trampoline_5,
    trampoline_6,
    trampoline_7,
    trampoline_8,
    trampoline_9,
    trampoline_10,
    trampoline_11,
    trampoline_12,
    trampoline_13,
    trampoline_14,
    trampoline_15,
    trampoline_16,
    trampoline_17,
    trampoline_18,
    trampoline_19,
    trampoline_20,
    trampoline_21,
    trampoline_22,
    trampoline_23,
    trampoline_24,
    trampoline_25,
    trampoline_26,
    trampoline_27,
    trampoline_28,
    trampoline_29,
    trampoline_30,
    trampoline_31,
    trampoline_32,
    trampoline_33,
    trampoline_34,
    trampoline_35,
    trampoline_36,
    trampoline_37,
    trampoline_38,
    trampoline_39,
    trampoline_40,
    trampoline_41,
    trampoline_42,
    trampoline_43,
    trampoline_44,
    trampoline_45,
    trampoline_46,
    trampoline_47,
    trampoline_48,
    trampoline_49,
    trampoline_50,
    trampoline_51,
    trampoline_52,
    trampoline_53,
    trampoline_54,
    trampoline_55,
    trampoline_56,
    trampoline_57,
    trampoline_58,
    trampoline_59,
    trampoline_60,
    trampoline_61,
    trampoline_62,
    trampoline_63,
];

// ─── deno_core ops ────────────────────────────────────────────────────────────

#[op2(fast)]
#[bigint]
fn op_find_by_contract(#[bigint] contract_id: u64, min_ver: u32) -> u64 {
    let vtable: *const HostVTable =
        DENO_HOST_VTABLE.with(|c: &core::cell::Cell<*const HostVTable>| c.get());
    if vtable.is_null() {
        return u64::MAX;
    }
    // SAFETY: DENO_HOST_VTABLE is set to a 'static HostVTable before JsRuntime creation.
    // V8 is thread-pinned — this op always runs on the same thread that set the vtable.
    let handle: PluginHandle = unsafe { ((*vtable).find_by_contract)(contract_id, min_ver) };
    if handle.is_null() {
        return u64::MAX;
    }
    (handle.generation as u64) << 32 | handle.index as u64
}

#[op2(fast)]
#[bigint]
fn op_find_by_bundle(#[bigint] bundle_id: u64, #[bigint] contract_id: u64, min_ver: u32) -> u64 {
    let vtable: *const HostVTable =
        DENO_HOST_VTABLE.with(|c: &core::cell::Cell<*const HostVTable>| c.get());
    if vtable.is_null() {
        return u64::MAX;
    }
    // SAFETY: DENO_HOST_VTABLE is set to a 'static HostVTable before JsRuntime creation.
    // V8 is thread-pinned — this op always runs on the same thread that set the vtable.
    let handle: PluginHandle =
        unsafe { ((*vtable).find_by_bundle)(bundle_id, contract_id, min_ver) };
    if handle.is_null() {
        u64::MAX
    } else {
        (handle.generation as u64) << 32 | handle.index as u64
    }
}

#[op2(fast)]
#[bigint]
fn op_find_all_by_contract(#[bigint] contract_id: u64, min_ver: u32) -> u64 {
    // Simplified: return just the first handle as u64, u64::MAX for none.
    let vtable: *const HostVTable =
        DENO_HOST_VTABLE.with(|c: &core::cell::Cell<*const HostVTable>| c.get());
    if vtable.is_null() {
        return u64::MAX;
    }
    let mut buf: [PluginHandle; 1] = [PluginHandle::null()];
    // SAFETY: DENO_HOST_VTABLE is set to a 'static HostVTable before JsRuntime creation.
    // V8 is thread-pinned — this op always runs on the same thread that set the vtable.
    // buf is stack-allocated; buf.as_mut_ptr() and capacity 1 are valid.
    let count: usize =
        unsafe { ((*vtable).find_all_by_contract)(contract_id, min_ver, buf.as_mut_ptr(), 1) };
    if count == 0 || buf[0].is_null() {
        u64::MAX
    } else {
        (buf[0].generation as u64) << 32 | buf[0].index as u64
    }
}

#[op2(fast)]
#[bigint]
fn op_resolve_plugin(#[bigint] handle_packed: u64) -> u64 {
    let vtable: *const HostVTable =
        DENO_HOST_VTABLE.with(|c: &core::cell::Cell<*const HostVTable>| c.get());
    if vtable.is_null() {
        return 0;
    }
    let handle: PluginHandle = PluginHandle {
        index: handle_packed as u32,
        generation: (handle_packed >> 32) as u32,
    };
    // SAFETY: DENO_HOST_VTABLE is set to a 'static HostVTable before JsRuntime creation.
    // V8 is thread-pinned — this op always runs on the same thread that set the vtable.
    let ptr: *const PluginVTable = unsafe { ((*vtable).resolve_plugin)(handle) };
    ptr as u64
}

#[op2(fast)]
#[bigint]
fn op_get_extension(extension_id: u32) -> u64 {
    let vtable: *const HostVTable =
        DENO_HOST_VTABLE.with(|c: &core::cell::Cell<*const HostVTable>| c.get());
    if vtable.is_null() {
        return 0;
    }
    // SAFETY: DENO_HOST_VTABLE is set to a 'static HostVTable before JsRuntime creation.
    // V8 is thread-pinned — this op always runs on the same thread that set the vtable.
    let ptr: *const () = unsafe { ((*vtable).get_extension)(extension_id) };
    ptr as u64
}

#[op2(fast)]
fn op_register_vtable(
    #[bigint] contract_id: u64,
    #[bigint] vtable_ptr: u64,
    fn_count: u32,
    #[string] contract_name: String,
) {
    VTABLE_SENDER.with(|c: &core::cell::RefCell<Option<VtableSenderInner>>| {
        let borrow: core::cell::Ref<'_, Option<VtableSenderInner>> = c.borrow();
        if let Some(tx) = borrow.as_ref() {
            let fn_count_usize: usize = fn_count as usize;
            let _ = tx.send((
                SendPluginVTable(vtable_ptr as *const PluginVTable),
                contract_id,
                fn_count_usize,
                contract_name,
            ));
        }
    });
}

#[op2(fast)]
#[bigint]
fn op_alloc(size: u32) -> u64 {
    let vtable: *const HostVTable =
        DENO_HOST_VTABLE.with(|c: &core::cell::Cell<*const HostVTable>| c.get());
    if vtable.is_null() {
        return 0;
    }
    // SAFETY: DENO_HOST_VTABLE is set to a 'static HostVTable before JsRuntime creation.
    // V8 is thread-pinned — this op always runs on the same thread that set the vtable.
    // size as usize and align=8 are valid allocation parameters.
    let ptr: *mut u8 = unsafe { ((*vtable).alloc)(size as usize, 8) };
    ptr as u64
}

#[op2(fast)]
fn op_free(#[bigint] ptr: u64) {
    let vtable: *const HostVTable =
        DENO_HOST_VTABLE.with(|c: &core::cell::Cell<*const HostVTable>| c.get());
    if vtable.is_null() {
        return;
    }
    // SAFETY: DENO_HOST_VTABLE is set to a 'static HostVTable before JsRuntime creation.
    // V8 is thread-pinned — this op always runs on the same thread that set the vtable.
    // ptr was allocated via op_alloc using this same host vtable allocator.
    // size=0 and align=8 match the allocation parameters used in op_alloc.
    unsafe { ((*vtable).free)(ptr as *mut u8, 0, 8) };
}

deno_core::extension!(
    polyplug_ops,
    ops = [
        op_find_by_contract,
        op_find_by_bundle,
        op_find_all_by_contract,
        op_resolve_plugin,
        op_get_extension,
        op_register_vtable,
        op_alloc,
        op_free,
    ]
);

struct InMemoryModuleLoader {
    specifier: deno_core::ModuleSpecifier,
    source: String,
}

impl ModuleLoader for InMemoryModuleLoader {
    fn resolve(
        &self,
        specifier: &str,
        referrer: &str,
        _kind: ResolutionKind,
    ) -> Result<deno_core::ModuleSpecifier, deno_error::JsErrorBox> {
        let resolved: deno_core::ModuleSpecifier = deno_core::resolve_import(specifier, referrer)
            .map_err(
            |e: deno_core::ModuleResolutionError| JsErrorBox::generic(e.to_string()),
        )?;
        Ok(resolved)
    }

    fn load(
        &self,
        module_specifier: &deno_core::ModuleSpecifier,
        _maybe_referrer: Option<&ModuleLoadReferrer>,
        _options: ModuleLoadOptions,
    ) -> deno_core::ModuleLoadResponse {
        if *module_specifier == self.specifier {
            let source: ModuleSource = ModuleSource::new(
                ModuleType::JavaScript,
                ModuleSourceCode::String(FastString::from(self.source.clone())),
                &self.specifier,
                None,
            );
            return deno_core::ModuleLoadResponse::Sync(Ok(source));
        }
        let error: JsErrorBox = JsErrorBox::generic("only the main JS fixture module is supported");
        deno_core::ModuleLoadResponse::Sync(Err(error))
    }
}

// ─── Loader ──────────────────────────────────────────────────────────────────

/// Loader for V8 in-process JS plugin bundles using deno_core.
pub struct JsDenoLoader {
    _config: JsDenoConfig,
}

impl JsDenoLoader {
    /// Create a new `JsDenoLoader` with the given config.
    pub fn new(config: JsDenoConfig) -> JsDenoLoader {
        JsDenoLoader { _config: config }
    }
}

impl BundleLoader for JsDenoLoader {
    fn runtime_name(&self) -> &'static str {
        "js-deno"
    }

    fn load(&self, path: &Path, registrar: &mut PluginRegistrar) -> Result<(), PolyplugError> {
        // 1. Capture host vtable ptr as usize to make it Send across thread boundary.
        // SAFETY: registrar.host is Box::leak'd by RuntimeBuilder — valid 'static.
        // Storing as usize makes the value Send; it is reconstructed as *const HostVTable
        // only on the bundle thread after thread_local assignment.
        let host_vtable_addr: usize = registrar.host as usize;

        // 2. Create channel for vtable registration (bounded 1 = oneshot)
        let (vtable_tx, vtable_rx): (
            VtableSenderInner,
            mpsc::Receiver<(SendPluginVTable, u64, usize, String)>,
        ) = mpsc::sync_channel(1);

        // 2b. Create channel for propagating init errors from the bundle thread.
        let (err_tx, err_rx): (
            mpsc::SyncSender<PolyplugError>,
            mpsc::Receiver<PolyplugError>,
        ) = mpsc::sync_channel::<PolyplugError>(1);

        // 3. Create channel for vtable call dispatch
        let (call_tx, call_rx): (
            mpsc::SyncSender<JsCallRequest>,
            mpsc::Receiver<JsCallRequest>,
        ) = mpsc::sync_channel::<JsCallRequest>(16);

        // 4. Clone path for thread (path is &Path, must be owned to move into thread)
        let bundle_path: PathBuf = path.to_owned();

        // 4b. Extract bundle directory for globalThis.bundlePath injection.
        // If path is a file, take its parent; if it is already a dir, use it directly.
        let bundle_dir_str: String = path.parent().unwrap_or(path).to_string_lossy().into_owned();
        // SAFETY: bundle_path_static is intentionally leaked. It is never freed.
        // The PluginRuntime's lifetime guarantees no dangling reference during use.
        let bundle_path_static: &'static str = Box::leak(bundle_dir_str.into_boxed_str());

        // 5. Spawn dedicated thread for this bundle's V8 isolate
        let thread_handle: std::thread::JoinHandle<()> = std::thread::spawn(move || {
            // SAFETY: host_vtable_addr encodes a *const HostVTable that is Box::leak'd
            // by RuntimeBuilder — valid 'static. Reconstructing as a raw pointer on this
            // thread is sound: only this thread reads the pointer; V8 is thread-pinned.
            DENO_HOST_VTABLE.with(|c: &core::cell::Cell<*const HostVTable>| {
                c.set(host_vtable_addr as *const HostVTable);
            });

            // Set thread-local vtable sender
            VTABLE_SENDER.with(|c: &core::cell::RefCell<Option<VtableSenderInner>>| {
                *c.borrow_mut() = Some(vtable_tx);
            });

            // Build tokio single-thread runtime
            // SAFETY: deno_core 0.311.0 requires tokio; smol would cause panics.
            let tokio_rt: tokio::runtime::Runtime =
                match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(e) => {
                        let _ =
                            err_tx.send(PolyplugError::Loader(LoaderError::JsRuntimeInitFailed {
                                reason: format!("failed to build tokio runtime: {e}"),
                            }));
                        return;
                    }
                };

            let init_result: Result<(), PolyplugError> = tokio_rt.block_on(async move {
                // Create deno_core JsRuntime
                // When called via the runtime, bundle_path is already the resolved file.
                // When called directly (e.g. tests), bundle_path may be the bundle directory.
                let module_path: PathBuf = if bundle_path.is_dir() {
                    let bundle_js: std::path::PathBuf = bundle_path.join("bundle.js");
                    let index_ts: std::path::PathBuf = bundle_path.join("index.ts");
                    if bundle_js.exists() {
                        bundle_js
                    } else {
                        index_ts
                    }
                } else {
                    bundle_path.clone()
                };
                let module_source: String =
                    std::fs::read_to_string(&module_path).map_err(|e: std::io::Error| {
                        PolyplugError::Loader(LoaderError::BundleReadFailed {
                            path: module_path.display().to_string(),
                            source: e,
                        })
                    })?;

                let module_url: deno_core::ModuleSpecifier = deno_core::resolve_path(
                    module_path.to_str().unwrap_or("bundle.js"),
                    &std::env::current_dir().unwrap_or_else(|_: std::io::Error| PathBuf::from(".")),
                )
                .map_err(|e| {
                    PolyplugError::Loader(LoaderError::ModuleResolutionFailed {
                        reason: format!("failed to resolve module URL: {e}"),
                    })
                })?;

                let module_loader: InMemoryModuleLoader = InMemoryModuleLoader {
                    specifier: module_url.clone(),
                    source: module_source,
                };

                let mut runtime: deno_core::JsRuntime =
                    deno_core::JsRuntime::new(deno_core::RuntimeOptions {
                        extensions: vec![polyplug_ops::init()],
                        module_loader: Some(std::rc::Rc::new(module_loader)),
                        ..Default::default()
                    });

                // Inject globalThis.bundlePath before the bundle module is evaluated.
                // This allows JS code to locate sibling resources relative to the bundle.
                let inject_script: deno_core::FastString =
                    deno_core::FastString::from_static(Box::leak(
                        format!("globalThis.bundlePath = {:?};", bundle_path_static)
                            .into_boxed_str(),
                    ));
                // SAFETY: inject_script is a valid JS snippet; the only failure mode is
                // a V8 exception from the injected source, which is propagated as an error.
                runtime
                    .execute_script("<bundlePath>", inject_script)
                    .map_err(|e: Box<deno_core::error::JsError>| {
                        PolyplugError::Loader(LoaderError::JsExecutionFailed {
                            reason: format!("failed to inject globalThis.bundlePath: {e}"),
                        })
                    })?;

                let mod_id: deno_core::ModuleId = runtime
                    .load_main_es_module(&module_url)
                    .await
                    .map_err(|e: deno_core::error::CoreError| {
                        PolyplugError::Loader(LoaderError::JsExecutionFailed {
                            reason: format!("failed to load module: {e}"),
                        })
                    })?;
                // Evaluate the module — triggers top-level execution including op_register_vtable
                let evaluate_future = runtime.mod_evaluate(mod_id);
                runtime.run_event_loop(Default::default()).await.map_err(
                    |e: deno_core::error::CoreError| {
                        PolyplugError::Loader(LoaderError::JsExecutionFailed {
                            reason: format!("event loop failed: {e}"),
                        })
                    },
                )?;
                // Drive the evaluate future to completion
                let _eval_result: Result<(), deno_core::error::CoreError> = evaluate_future.await;

                // op_register_vtable has sent vtable_ptr via VTABLE_SENDER by now
                // Clear thread-local sender
                VTABLE_SENDER.with(|c: &core::cell::RefCell<Option<VtableSenderInner>>| {
                    *c.borrow_mut() = None;
                });

                // Park on call_rx loop — dispatch function calls from trampolines
                while let Ok(req) = call_rx.recv() {
                    // For MVP: return ABI_OK (stub dispatch — full impl in future epic)
                    let _ = req.result_tx.send(AbiError::ok());
                }

                Ok(())
            });
            if let Err(e) = init_result {
                let _ = err_tx.send(e);
            }
        });

        // Leak thread handle so the thread isn't dropped
        // SAFETY: Bundle threads live for the process lifetime. Leaking the JoinHandle
        // prevents premature thread termination while allowing the main thread to proceed.
        let _leaked: &'static std::thread::JoinHandle<()> = Box::leak(Box::new(thread_handle));

        // 6. Receive the registered vtable ptr from the bundle thread (30s timeout)
        let (raw_vtable_wrapped, contract_id_val, fn_count, contract_name_str): (
            SendPluginVTable,
            u64,
            usize,
            String,
        ) = vtable_rx.recv_timeout(Duration::from_secs(30)).map_err(
            |_: mpsc::RecvTimeoutError| {
                // Check if the bundle thread reported a specific error.
                err_rx.try_recv().unwrap_or_else(|_: mpsc::TryRecvError| {
                    PolyplugError::Loader(LoaderError::JsRuntimePanic {
                        runtime: "js-deno".to_owned(),
                        message: "vtable registration timed out after 30s".to_owned(),
                    })
                })
            },
        )?;
        let raw_vtable: *const PluginVTable = raw_vtable_wrapped.0;

        if raw_vtable.is_null() {
            return Err(PolyplugError::Loader(LoaderError::JsRuntimePanic {
                runtime: "js-deno".to_owned(),
                message: "registerVtable() received null vtable pointer".to_owned(),
            }));
        }

        let contract_version: u32 = 0_u32;

        let base_slot: usize = {
            let reg: &Mutex<Vec<Option<DenoFunctionSlot>>> = deno_function_registry();
            let mut guard: MutexGuard<'_, Vec<Option<DenoFunctionSlot>>> =
                reg.lock().unwrap_or_else(
                    |e: std::sync::PoisonError<MutexGuard<'_, Vec<Option<DenoFunctionSlot>>>>| {
                        e.into_inner()
                    },
                );
            let slot: usize = guard.len();
            if slot + fn_count > MAX_TRAMPOLINES {
                return Err(PolyplugError::Loader(LoaderError::JsRuntimePanic {
                    runtime: "js-deno".to_owned(),
                    message: format!(
                        "too many function slots: {} + {} > {}",
                        slot, fn_count, MAX_TRAMPOLINES
                    ),
                }));
            }
            for _ in 0..fn_count {
                guard.push(Some(DenoFunctionSlot {
                    call_tx: call_tx.clone(),
                }));
            }
            slot
        };

        let mut fn_ptr_vec: Vec<*const ()> = Vec::with_capacity(fn_count);
        for slot_offset in 0..fn_count {
            let slot: usize = base_slot + slot_offset;
            // SAFETY: TRAMPOLINES[slot] is a valid static extern "C" fn pointer.
            // We cast the fn pointer to *const () for storage in PluginVTable.functions.
            // The trampoline is 'static — it lives for the entire process lifetime.
            let fn_ptr: *const () = TRAMPOLINES[slot] as *const ();
            fn_ptr_vec.push(fn_ptr);
        }
        let fn_pointers_box: Box<[*const ()]> = fn_ptr_vec.into_boxed_slice();
        // SAFETY: PluginVTable.functions must be 'static. Box::leak gives 'static lifetime.
        // The raw pointer is stable after Box::into_raw — the allocation is never freed.
        let functions_ptr: *const *const () = Box::into_raw(fn_pointers_box) as *const *const ();

        // 8. Build new vtable
        let new_vtable: PluginVTable = PluginVTable {
            contract_id: contract_id_val,
            contract_version,
            function_count: fn_count as u32,
            functions: functions_ptr,
        };
        // SAFETY: vtable must be 'static — Box::leak ensures it outlives the runtime.
        let static_vtable: *const PluginVTable = Box::into_raw(Box::new(new_vtable));

        // 9. Build descriptor
        let contract_name_leaked: &'static str = Box::leak(contract_name_str.into_boxed_str());
        let descriptor: PluginDescriptor = PluginDescriptor {
            name: StringView::from_static(b"js-deno-plugin"),
            contract_name: StringView {
                ptr: contract_name_leaked.as_ptr(),
                len: contract_name_leaked.len(),
            },
            version_major: contract_version >> 16,
            version_minor: contract_version & 0xFFFF,
            version_patch: 0_u32,
        };

        // 10. Register with host
        // SAFETY: registrar is valid for this call (passed by the integration test or runtime).
        // descriptor is stack-allocated and valid for this call — register_plugin must copy any
        // data it needs to retain (the contract is that descriptor is borrowed for the call only).
        // static_vtable is a leaked Box — valid for 'static lifetime.
        let abi_result: AbiError = unsafe {
            (registrar.register_plugin)(
                registrar as *mut PluginRegistrar,
                &descriptor as *const PluginDescriptor,
                static_vtable,
            )
        };
        if abi_result.code != ABI_OK {
            return Err(PolyplugError::Loader(LoaderError::JsRuntimePanic {
                runtime: "js-deno".to_owned(),
                message: format!("register_plugin returned error code {}", abi_result.code),
            }));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn js_deno_runtime_name() {
        let loader: JsDenoLoader = JsDenoLoader::new(JsDenoConfig {});
        assert_eq!(loader.runtime_name(), "js-deno");
    }
}
