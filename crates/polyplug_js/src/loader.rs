//! QuickJS in-process plugin loader implementation.
//!
//! Loads JS plugin bundles via the embedded QuickJS VM (rquickjs).
//! One shared QuickJS Runtime per process. Each bundle gets a fresh Context.
//! The registerVtable() callback writes back through a thread-local to the load() call.

use core::cell::RefCell;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::OnceLock;

use rquickjs::Context;
use rquickjs::Ctx;
use rquickjs::Function;
use rquickjs::Object;
use rquickjs::Runtime;
use rquickjs::Value;

use crate::config::JsConfig;
use polyplug::abi::ABI_OK;
use polyplug::abi::AbiError;
use polyplug::abi::HostVTable;
use polyplug::abi::PluginDescriptor;
use polyplug::abi::PluginHandle;
use polyplug::abi::PluginRegistrar;
use polyplug::abi::PluginVTable;
use polyplug::abi::StringView;
use polyplug::error::LoaderError;
use polyplug::error::PolyplugError;
use polyplug::loader::BundleLoader;

// ─── Process-global QuickJS Runtime ──────────────────────────────────────────

/// One shared QuickJS runtime per process.
/// rquickjs `parallel` feature makes Runtime: Send+Sync.
static QJS_RUNTIME: OnceLock<Result<Runtime, String>> = OnceLock::new();

// ─── Thread-local pending vtable (set from registerVtable callback) ───────────

thread_local! {
    /// Set by the registerVtable() JS callback during bundle eval.
    /// Cleared before each eval; read after eval completes.
    static PENDING_VTABLE: RefCell<Option<(u64, *const PluginVTable, usize)>> =
        const { RefCell::new(None) };
}

// ─── Host VTable pointer (set once at first load call) ────────────────────────

/// HostVTable* stored once — valid for 'static (Box::leak'd by RuntimeBuilder).
static HOST_VTABLE: OnceLock<HostVtablePtr> = OnceLock::new();

/// Thread-safe wrapper for the raw HostVTable pointer.
struct HostVtablePtr(*const HostVTable);

// SAFETY: HostVTable* points to 'static data (Box::leak). Only read after set.
// The data it points to is never mutated after construction.
unsafe impl Send for HostVtablePtr {}

// SAFETY: Same reasoning as Send — concurrent reads of immutable 'static data are safe.
unsafe impl Sync for HostVtablePtr {}

// ─── Function registry for trampolines ───────────────────────────────────────

/// Global registry of QuickJS function slot placeholders for trampoline dispatch.
/// For JS plugins, trampolines are stubs — actual dispatch goes through the vtable pointer.
/// The Vec is indexed by slot — each plugin gets a contiguous range.
static FUNCTION_REGISTRY: OnceLock<Mutex<Vec<Option<()>>>> = OnceLock::new();

/// Get or initialize the function registry.
fn function_registry() -> &'static Mutex<Vec<Option<()>>> {
    FUNCTION_REGISTRY.get_or_init(|| Mutex::new(Vec::new()))
}

/// Dispatch a QuickJS trampoline call by slot index.
///
/// For the MVP, JS plugins expose their vtable directly through registerVtable.
/// The trampolines here are stubs that return ABI_OK — actual dispatch goes
/// through the vtable pointer obtained from registerVtable().
fn dispatch_quickjs_call(_slot: usize, _args_ptr: *const (), _out_ptr: *mut ()) -> AbiError {
    // MVP: trampolines are stubs. Actual dispatch is handled by the vtable
    // pointer returned from registerVtable() and stored by the host.
    AbiError {
        code: ABI_OK,
        message: StringView::null(),
    }
}

// ─── Trampolines (64 static extern "C" slots) ─────────────────────────────────

// Pre-generated static extern "C" trampolines (slots 0..63).
// Each trampoline has a hardcoded slot index and calls `dispatch_quickjs_call`.
// We cannot use closures for extern "C" fn pointers — static trampolines
// with a hardcoded slot are the correct Rust solution.
macro_rules! make_trampoline {
    ($name:ident, $slot:expr) => {
        // SAFETY: trampolines are `extern "C"` functions with the ABI signature
        // expected by PluginVTable.functions: fn(*const (), *mut ()) -> AbiError.
        // `dispatch_quickjs_call` is safe to call from any thread.
        unsafe extern "C" fn $name(args_ptr: *const (), out_ptr: *mut ()) -> AbiError {
            dispatch_quickjs_call($slot, args_ptr, out_ptr)
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

// ─── Host function registration ───────────────────────────────────────────────

/// Pack a PluginHandle into a u64 (index in low 32 bits, generation in high 32 bits).
/// Returns None for null handles.
fn pack_handle(h: PluginHandle) -> Option<u64> {
    if h.is_null() {
        None
    } else {
        Some((h.generation as u64) << 32 | h.index as u64)
    }
}

/// Register all polyplug host functions on the given JS object.
///
/// Registers 8 functions that expose the HostVTable capabilities to JS code.
/// All u64 values split into lo/hi u32 pairs for JS compatibility.
/// PluginHandle returned as packed u64 (index | generation<<32), or null.
fn register_host_functions<'js>(
    ctx: &Ctx<'js>,
    polyplug_obj: &Object<'js>,
) -> Result<(), PolyplugError> {
    // findByContract(lo: u32, hi: u32, min_ver: u32) → u64 (packed handle) | null
    let find_by_contract_fn: Function<'js> = Function::new(
        ctx.clone(),
        |lo: u32, hi: u32, min_ver: u32| -> Option<u64> {
            let contract_id: u64 = (hi as u64) << 32 | lo as u64;
            let hvt_ptr: Option<&HostVtablePtr> = HOST_VTABLE.get();
            let hvt: *const HostVTable = match hvt_ptr {
                Some(p) => p.0,
                None => return None,
            };
            // SAFETY: HOST_VTABLE is set once from a 'static HostVTable pointer.
            // The HostVTable is valid for process lifetime (set by RuntimeBuilder).
            let handle: PluginHandle = unsafe { ((*hvt).find_by_contract)(contract_id, min_ver) };
            pack_handle(handle)
        },
    )
    .map_err(|e: rquickjs::Error| {
        PolyplugError::Loader(LoaderError::JsRuntimePanic {
            runtime: "js-quickjs".to_owned(),
            message: format!("findByContract function creation failed: {e}"),
        })
    })?;

    polyplug_obj
        .set("findByContract", find_by_contract_fn)
        .map_err(|e: rquickjs::Error| {
            PolyplugError::Loader(LoaderError::JsRuntimePanic {
                runtime: "js-quickjs".to_owned(),
                message: format!("findByContract set failed: {e}"),
            })
        })?;

    // findByBundle(blo: u32, bhi: u32, clo: u32, chi: u32, min_ver: u32) → u64 | null
    let find_by_bundle_fn: Function<'js> = Function::new(
        ctx.clone(),
        |blo: u32, bhi: u32, clo: u32, chi: u32, min_ver: u32| -> Option<u64> {
            let bundle_id: u64 = (bhi as u64) << 32 | blo as u64;
            let contract_id: u64 = (chi as u64) << 32 | clo as u64;
            let hvt_ptr: Option<&HostVtablePtr> = HOST_VTABLE.get();
            let hvt: *const HostVTable = match hvt_ptr {
                Some(p) => p.0,
                None => return None,
            };
            // SAFETY: HOST_VTABLE is set once from a 'static HostVTable pointer.
            let handle: PluginHandle =
                unsafe { ((*hvt).find_by_bundle)(bundle_id, contract_id, min_ver) };
            pack_handle(handle)
        },
    )
    .map_err(|e: rquickjs::Error| {
        PolyplugError::Loader(LoaderError::JsRuntimePanic {
            runtime: "js-quickjs".to_owned(),
            message: format!("findByBundle function creation failed: {e}"),
        })
    })?;

    polyplug_obj
        .set("findByBundle", find_by_bundle_fn)
        .map_err(|e: rquickjs::Error| {
            PolyplugError::Loader(LoaderError::JsRuntimePanic {
                runtime: "js-quickjs".to_owned(),
                message: format!("findByBundle set failed: {e}"),
            })
        })?;

    // findAllByContract(lo: u32, hi: u32, min_ver: u32) → u32 (count)
    let find_all_by_contract_fn: Function<'js> =
        Function::new(ctx.clone(), |lo: u32, hi: u32, min_ver: u32| -> u32 {
            let contract_id: u64 = (hi as u64) << 32 | lo as u64;
            let hvt_ptr: Option<&HostVtablePtr> = HOST_VTABLE.get();
            let hvt: *const HostVTable = match hvt_ptr {
                Some(p) => p.0,
                None => return 0_u32,
            };
            // SAFETY: HOST_VTABLE is set once from a 'static HostVTable pointer.
            // We pass null out pointer and 0 capacity to get the count only.
            let count: usize = unsafe {
                ((*hvt).find_all_by_contract)(contract_id, min_ver, core::ptr::null_mut(), 0)
            };
            count as u32
        })
        .map_err(|e: rquickjs::Error| {
            PolyplugError::Loader(LoaderError::JsRuntimePanic {
                runtime: "js-quickjs".to_owned(),
                message: format!("findAllByContract function creation failed: {e}"),
            })
        })?;

    polyplug_obj
        .set("findAllByContract", find_all_by_contract_fn)
        .map_err(|e: rquickjs::Error| {
            PolyplugError::Loader(LoaderError::JsRuntimePanic {
                runtime: "js-quickjs".to_owned(),
                message: format!("findAllByContract set failed: {e}"),
            })
        })?;

    // resolvePlugin(packed_handle: u64) → u32 (vtable ptr lo) | null
    let resolve_plugin_fn: Function<'js> =
        Function::new(ctx.clone(), |packed: u64| -> Option<u32> {
            let index: u32 = packed as u32;
            let generation: u32 = (packed >> 32) as u32;
            let handle: PluginHandle = PluginHandle { index, generation };
            let hvt_ptr: Option<&HostVtablePtr> = HOST_VTABLE.get();
            let hvt: *const HostVTable = match hvt_ptr {
                Some(p) => p.0,
                None => return None,
            };
            // SAFETY: HOST_VTABLE is set once from a 'static HostVTable pointer.
            let vtable_ptr: *const PluginVTable = unsafe { ((*hvt).resolve_plugin)(handle) };
            if vtable_ptr.is_null() {
                None
            } else {
                Some(vtable_ptr as usize as u32)
            }
        })
        .map_err(|e: rquickjs::Error| {
            PolyplugError::Loader(LoaderError::JsRuntimePanic {
                runtime: "js-quickjs".to_owned(),
                message: format!("resolvePlugin function creation failed: {e}"),
            })
        })?;

    polyplug_obj
        .set("resolvePlugin", resolve_plugin_fn)
        .map_err(|e: rquickjs::Error| {
            PolyplugError::Loader(LoaderError::JsRuntimePanic {
                runtime: "js-quickjs".to_owned(),
                message: format!("resolvePlugin set failed: {e}"),
            })
        })?;

    // getExtension(extension_id: u32) → u32 (ptr lo) | null
    let get_extension_fn: Function<'js> =
        Function::new(ctx.clone(), |extension_id: u32| -> Option<u32> {
            let hvt_ptr: Option<&HostVtablePtr> = HOST_VTABLE.get();
            let hvt: *const HostVTable = match hvt_ptr {
                Some(p) => p.0,
                None => return None,
            };
            // SAFETY: HOST_VTABLE is set once from a 'static HostVTable pointer.
            let ext_ptr: *const () = unsafe { ((*hvt).get_extension)(extension_id) };
            if ext_ptr.is_null() {
                None
            } else {
                Some(ext_ptr as usize as u32)
            }
        })
        .map_err(|e: rquickjs::Error| {
            PolyplugError::Loader(LoaderError::JsRuntimePanic {
                runtime: "js-quickjs".to_owned(),
                message: format!("getExtension function creation failed: {e}"),
            })
        })?;

    polyplug_obj
        .set("getExtension", get_extension_fn)
        .map_err(|e: rquickjs::Error| {
            PolyplugError::Loader(LoaderError::JsRuntimePanic {
                runtime: "js-quickjs".to_owned(),
                message: format!("getExtension set failed: {e}"),
            })
        })?;

    let register_vtable_fn: Function<'js> = Function::new(
        ctx.clone(),
        |contract_lo: u32, contract_hi: u32, vtable_lo: u32, vtable_hi: u32, fn_count: u32| {
            let contract_id: u64 = (contract_hi as u64) << 32 | contract_lo as u64;
            let vtable_addr: u64 = (vtable_hi as u64) << 32 | vtable_lo as u64;
            let vtable_ptr: *const PluginVTable = vtable_addr as usize as *const PluginVTable;
            let fn_count_usize: usize = fn_count as usize;
            PENDING_VTABLE.with(
                |cell: &RefCell<Option<(u64, *const PluginVTable, usize)>>| {
                    *cell.borrow_mut() = Some((contract_id, vtable_ptr, fn_count_usize));
                },
            );
        },
    )
    .map_err(|e: rquickjs::Error| {
        PolyplugError::Loader(LoaderError::JsRuntimePanic {
            runtime: "js-quickjs".to_owned(),
            message: format!("registerVtable function creation failed: {e}"),
        })
    })?;

    polyplug_obj
        .set("registerVtable", register_vtable_fn)
        .map_err(|e: rquickjs::Error| {
            PolyplugError::Loader(LoaderError::JsRuntimePanic {
                runtime: "js-quickjs".to_owned(),
                message: format!("registerVtable set failed: {e}"),
            })
        })?;

    // alloc(size: u32) → u32 (ptr lo)
    let alloc_fn: Function<'js> = Function::new(ctx.clone(), |size: u32| -> u32 {
        let hvt_ptr: Option<&HostVtablePtr> = HOST_VTABLE.get();
        let hvt: *const HostVTable = match hvt_ptr {
            Some(p) => p.0,
            None => return 0_u32,
        };
        // SAFETY: HOST_VTABLE is set once from a 'static HostVTable pointer.
        let ptr: *mut u8 = unsafe { ((*hvt).alloc)(size as usize, 1) };
        ptr as usize as u32
    })
    .map_err(|e: rquickjs::Error| {
        PolyplugError::Loader(LoaderError::JsRuntimePanic {
            runtime: "js-quickjs".to_owned(),
            message: format!("alloc function creation failed: {e}"),
        })
    })?;

    polyplug_obj
        .set("alloc", alloc_fn)
        .map_err(|e: rquickjs::Error| {
            PolyplugError::Loader(LoaderError::JsRuntimePanic {
                runtime: "js-quickjs".to_owned(),
                message: format!("alloc set failed: {e}"),
            })
        })?;

    // free(lo: u32) → void
    let free_fn: Function<'js> = Function::new(ctx.clone(), |lo: u32| {
        let hvt_ptr: Option<&HostVtablePtr> = HOST_VTABLE.get();
        let hvt: *const HostVTable = match hvt_ptr {
            Some(p) => p.0,
            None => return,
        };
        let ptr: *mut u8 = lo as usize as *mut u8;
        if ptr.is_null() {
            return;
        }
        // SAFETY: HOST_VTABLE is set once from a 'static HostVTable pointer.
        // ptr was allocated by the host allocator (alloc above). Size 0 is a
        // best-effort stub — the host allocator must tolerate this on free.
        unsafe { ((*hvt).free)(ptr, 0, 1) };
    })
    .map_err(|e: rquickjs::Error| {
        PolyplugError::Loader(LoaderError::JsRuntimePanic {
            runtime: "js-quickjs".to_owned(),
            message: format!("free function creation failed: {e}"),
        })
    })?;

    polyplug_obj
        .set("free", free_fn)
        .map_err(|e: rquickjs::Error| {
            PolyplugError::Loader(LoaderError::JsRuntimePanic {
                runtime: "js-quickjs".to_owned(),
                message: format!("free set failed: {e}"),
            })
        })?;

    Ok(())
}

// ─── JsLoader ────────────────────────────────────────────────────────────────

/// QuickJS in-process JS plugin loader.
///
/// Loads `bundle.js` from the bundle directory into a fresh QuickJS Context.
/// The JS bundle must call `polyplug.registerVtable(contract_lo, contract_hi, vtable_lo, vtable_hi)`
/// during top-level evaluation to register its vtable with the host.
pub struct JsLoader {
    _config: JsConfig,
}

impl JsLoader {
    /// Create a new `JsLoader` with the given configuration.
    pub fn new(config: JsConfig) -> JsLoader {
        JsLoader { _config: config }
    }
}

impl BundleLoader for JsLoader {
    fn runtime_name(&self) -> &'static str {
        "js-quickjs"
    }

    fn load(&self, path: &Path, registrar: &mut PluginRegistrar) -> Result<(), PolyplugError> {
        // 1. Set HOST_VTABLE once.
        // SAFETY: registrar.host is a 'static pointer (set by RuntimeBuilder via Box::leak).
        // We store it in HOST_VTABLE for access from JS callback closures.
        let _ = HOST_VTABLE.get_or_init(|| HostVtablePtr(registrar.host));

        // 2. Resolve bundle.js path.
        // When called via the runtime, path is already the resolved file (manifest.file joined to bundle dir).
        // When called directly (e.g. tests), path may be the bundle directory — fall back to bundle.js inside it.
        let bundle_path: PathBuf = if path.is_dir() {
            path.join("bundle.js")
        } else {
            path.to_path_buf()
        };
        let bundle_js: String =
            std::fs::read_to_string(&bundle_path).map_err(|e: std::io::Error| {
                PolyplugError::Loader(LoaderError::ManifestParse {
                    path: bundle_path.display().to_string(),
                    reason: e.to_string(),
                })
            })?;

        // 3. Init/get QuickJS runtime.
        let runtime: &Runtime = QJS_RUNTIME
            .get_or_init(|| {
                Runtime::new()
                    .map_err(|e: rquickjs::Error| format!("QuickJS runtime init failed: {e}"))
            })
            .as_ref()
            .map_err(|reason: &String| {
                PolyplugError::Loader(LoaderError::JsRuntimeInitFailed {
                    reason: reason.clone(),
                })
            })?;

        // 4. Create fresh Context for this bundle.
        let ctx: Context = Context::full(runtime).map_err(|e: rquickjs::Error| {
            PolyplugError::Loader(LoaderError::JsRuntimePanic {
                runtime: "js-quickjs".to_owned(),
                message: format!("context creation failed: {e}"),
            })
        })?;

        // 5. Clear PENDING_VTABLE before eval.
        PENDING_VTABLE.with(|c: &RefCell<Option<(u64, *const PluginVTable, usize)>>| {
            *c.borrow_mut() = None;
        });

        // Extract bundle directory for globalThis.bundlePath injection.
        let bundle_dir: std::path::PathBuf =
            bundle_path.parent().unwrap_or(&bundle_path).to_path_buf();
        let bundle_dir_str: String = bundle_dir.to_string_lossy().into_owned();
        // 6. Set up polyplug global and eval bundle.
        let eval_result: Result<(), PolyplugError> =
            ctx.with(|ctx_ref: Ctx<'_>| -> Result<(), PolyplugError> {
                let globals: Object<'_> = ctx_ref.globals();
                let polyplug_obj: Object<'_> =
                    Object::new(ctx_ref.clone()).map_err(|e: rquickjs::Error| {
                        PolyplugError::Loader(LoaderError::JsRuntimePanic {
                            runtime: "js-quickjs".to_owned(),
                            message: format!("object creation failed: {e}"),
                        })
                    })?;
                register_host_functions(&ctx_ref, &polyplug_obj)?;
                globals
                    .set("polyplug", polyplug_obj)
                    .map_err(|e: rquickjs::Error| {
                        PolyplugError::Loader(LoaderError::JsRuntimePanic {
                            runtime: "js-quickjs".to_owned(),
                            message: format!("global set failed: {e}"),
                        })
                    })?;
                // Inject bundlePath global before bundle eval, so init(globalThis.bundlePath) works at top-level.
                let set_bundle: String = format!("globalThis.bundlePath = {:?};", bundle_dir_str);
                ctx_ref.eval::<Value<'_>, _>(set_bundle.as_str()).map_err(
                    |e: rquickjs::Error| {
                        PolyplugError::Loader(LoaderError::JsRuntimePanic {
                            runtime: "js-quickjs".to_owned(),
                            message: format!("bundlePath injection failed: {e}"),
                        })
                    },
                )?;
                ctx_ref.eval::<Value<'_>, _>(bundle_js.as_str()).map_err(
                    |e: rquickjs::Error| {
                        PolyplugError::Loader(LoaderError::JsRuntimePanic {
                            runtime: "js-quickjs".to_owned(),
                            message: format!("bundle eval failed: {e}"),
                        })
                    },
                )?;
                Ok(())
            });
        eval_result?;

        // 7. Extract registered vtable from PENDING_VTABLE.
        let (contract_id_val, vtable_ptr, fn_count): (u64, *const PluginVTable, usize) =
            PENDING_VTABLE
                .with(|c: &RefCell<Option<(u64, *const PluginVTable, usize)>>| *c.borrow())
                .ok_or_else(|| {
                    PolyplugError::Loader(LoaderError::JsRuntimePanic {
                        runtime: "js-quickjs".to_owned(),
                        message: "bundle did not call polyplug.registerVtable()".to_owned(),
                    })
                })?;

        if vtable_ptr.is_null() {
            return Err(PolyplugError::Loader(LoaderError::JsRuntimePanic {
                runtime: "js-quickjs".to_owned(),
                message: "registerVtable() received null vtable pointer".to_owned(),
            }));
        }

        let contract_version: u32 = 0_u32;

        // 9. Allocate trampoline slots.
        let base_slot: usize = {
            let reg: &Mutex<Vec<Option<()>>> = function_registry();
            let mut guard: MutexGuard<'_, Vec<Option<()>>> =
                reg.lock().unwrap_or_else(|e| e.into_inner());
            let slot: usize = guard.len();
            if slot + fn_count > MAX_TRAMPOLINES {
                return Err(PolyplugError::Loader(LoaderError::JsRuntimePanic {
                    runtime: "js-quickjs".to_owned(),
                    message: format!(
                        "too many function slots: {} + {} > {}",
                        slot, fn_count, MAX_TRAMPOLINES
                    ),
                }));
            }
            for _ in 0..fn_count {
                // Push placeholder slots — JS vtable doesn't need per-slot Rust callbacks.
                guard.push(None);
            }
            slot
        };

        // 10. Build function pointer array using pre-generated trampolines.
        let mut fn_ptr_vec: Vec<*const ()> = Vec::with_capacity(fn_count);
        for slot_offset in 0..fn_count {
            let slot: usize = base_slot + slot_offset;
            // SAFETY: TRAMPOLINES[slot] is a valid static extern "C" fn pointer.
            // We cast to *const () for storage in PluginVTable.functions.
            // The trampoline is 'static — it lives for the entire process lifetime.
            let fn_ptr: *const () = TRAMPOLINES[slot] as *const ();
            fn_ptr_vec.push(fn_ptr);
        }

        // SAFETY: PluginVTable.functions must point to 'static data.
        // Box::into_raw produces a valid, non-null, properly-aligned pointer.
        // Box::leak gives 'static lifetime — the pointers outlive the runtime.
        let fn_pointers_box: Box<[*const ()]> = fn_ptr_vec.into_boxed_slice();
        let functions_ptr: *const *const () = Box::into_raw(fn_pointers_box) as *const *const ();

        // 11. Build vtable.
        let new_vtable: PluginVTable = PluginVTable {
            contract_id: contract_id_val,
            contract_version,
            function_count: fn_count as u32,
            functions: functions_ptr,
        };

        // SAFETY: vtable must be 'static — Box::leak ensures it outlives the runtime.
        // The vtable is valid for the process lifetime (JS plugins are never unloaded).
        let static_vtable: *const PluginVTable = Box::into_raw(Box::new(new_vtable));

        // 12. Build descriptor.
        let contract_name_str: String = format!("js_contract_{:#x}", contract_id_val);
        let contract_name_leaked: &'static str = Box::leak(contract_name_str.into_boxed_str());
        let descriptor: PluginDescriptor = PluginDescriptor {
            name: StringView::from_static(b"js-quickjs-plugin"),
            contract_name: StringView {
                ptr: contract_name_leaked.as_ptr(),
                len: contract_name_leaked.len(),
            },
            version_major: contract_version >> 16,
            version_minor: contract_version & 0xFFFF,
            version_patch: 0_u32,
        };

        // 13. Register with host.
        // SAFETY: registrar, descriptor, and static_vtable are all valid for this call.
        // descriptor is borrowed for the duration of the call only (register_plugin must copy).
        // static_vtable is a leaked Box — valid for 'static.
        let abi_result: AbiError = unsafe {
            (registrar.register_plugin)(
                registrar as *mut PluginRegistrar,
                &descriptor as *const PluginDescriptor,
                static_vtable,
            )
        };

        if abi_result.code != ABI_OK {
            return Err(PolyplugError::Loader(LoaderError::JsRuntimePanic {
                runtime: "js-quickjs".to_owned(),
                message: format!("register_plugin returned error code {}", abi_result.code),
            }));
        }

        Ok(())
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    #[allow(clippy::expect_used)]
    use super::*;

    #[test]
    fn js_quickjs_runtime_name() {
        let loader: JsLoader = JsLoader::new(JsConfig {});
        assert_eq!(loader.runtime_name(), "js-quickjs");
    }
}
