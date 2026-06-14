//! Integration test: the **generated** Python `_<plugin>_IMPL` / `_<plugin>_FACTORY`
//! module globals are isolated per (bundle, runtime), NOT shared across runtimes.
//!
//! # Why this exists
//!
//! The Python guest generator stores each plugin's constructed implementation in a
//! module-level global `_<plugin>_IMPL` (set by `polyplug_init` from the author
//! factory `_<plugin>_FACTORY`), and the per-call `<plugin>_<fn>_abi` callable
//! reads it from that module's globals. CPython is shared process-wide, so a naive
//! reading suggests two `Runtime` instances loading the same-named bundle would
//! clobber each other's `_<plugin>_IMPL` through the shared `sys.modules` cache —
//! the same class of cross-runtime leak the C# host-contract factory (#68) had.
//!
//! They do not. The `polyplug_python` loader's per-bundle module isolation pass
//! (`isolate_bundle_modules` + the process-global `ISOLATION_NONCE`) re-keys every
//! freshly imported in-bundle module — **including the `generated.*` package that
//! holds `_<plugin>_IMPL`/`_<plugin>_FACTORY`** — under a unique per-load prefix.
//! Each load therefore gets its own module object with its own globals, so the
//! generated impl/factory globals are per-(bundle,runtime) instance state (the
//! Rule-12-sanctioned category), exactly like Lua per-`_G` and JS per-context
//! globals.
//!
//! `crates/polyplug_python/tests/python_loader.rs::two_runtimes_same_named_bundle_do_not_collide_in_sys_modules`
//! proves the isolation pass re-keys a *hand-written* in-bundle module global. This
//! test proves the same guarantee for the **actual generated** impl/factory pattern:
//! two runtimes load two same-named bundles whose generated glue is byte-identical
//! (same plugin name → identical `set_probe_factory`/`_probe_IMPL` symbols and the
//! same `bundle_id` name-hash) but whose author factories construct impls returning
//! different values. Each runtime must dispatch its OWN impl's value.
//!
//! Uses the embedded CPython interpreter (pyo3), so it runs wherever the other
//! `integration_python` loader tests run.

#![allow(clippy::expect_used)]
#![allow(clippy::undocumented_unsafe_blocks)]

use polyplug::runtime::Runtime;
use polyplug_abi::AbiError;
use polyplug_abi::AbiErrorCode;
use polyplug_abi::DispatchType;
use polyplug_abi::GuestContractHandle;
use polyplug_abi::GuestContractInstance;
use polyplug_abi::GuestContractInterface;
use polyplug_codegen::GenerateConfig;
use polyplug_codegen::GenerateOutput;
use polyplug_codegen::Lang;
use polyplug_codegen::Side;
use polyplug_python::PythonConfig;
use polyplug_python::PythonLoader;
use polyplug_utils::guest_contract_id;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

/// Both bundles deliberately share this name so their `bundle_id` name-hash
/// collides — the per-load `ISOLATION_NONCE` is then the *only* thing keeping
/// their generated modules distinct in the shared `sys.modules`.
const SHARED_BUNDLE_NAME: &str = "iso_probe_shared";

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("parent of tests/integration")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

/// The single-contract API both bundles generate from: `iso.Probe@1` with a
/// no-arg `value() -> i32`. The returned integer comes entirely from the author
/// impl constructed by the factory, so it is the per-runtime discriminator.
const PROBE_API_TOML: &str = "[[plugin_contract]]\n\
     name = \"iso.Probe\"\n\
     version = \"1.0.0\"\n\n\
     [[plugin_contract.functions]]\n\
     name = \"value\"\n\
     return = \"i32\"\n";

/// Write one Python probe bundle into `tmp/<dir_name>`: generate the guest glue
/// from `PROBE_API_TOML`, write the hand-written entry whose factory builds an
/// impl returning `return_value`, vendor the current guest SDK, and return the
/// bundle directory. All bundles use `SHARED_BUNDLE_NAME` and plugin `probe`, so
/// their generated symbols and `bundle_id` are identical by construction.
fn write_probe_bundle(tmp: &Path, dir_name: &str, return_value: i32) -> PathBuf {
    let bundle_dir: PathBuf = tmp.join(dir_name);
    std::fs::create_dir_all(&bundle_dir).expect("create probe bundle dir");

    let api_path: PathBuf = bundle_dir.join("api.toml");
    std::fs::write(&api_path, PROBE_API_TOML).expect("write api.toml");

    let bundle_toml: String = format!(
        "[bundle]\n\
         name = \"{name}\"\n\
         version = \"1.0.0\"\n\
         api = \"api.toml\"\n\
         loader = \"python\"\n\
         file = \"entry.py\"\n\n\
         [[plugin]]\n\
         name = \"probe\"\n\
         version = \"1.0.0\"\n\
         implements = [\"iso.Probe@1.0\"]\n",
        name = SHARED_BUNDLE_NAME,
    );
    let bundle_toml_path: PathBuf = bundle_dir.join("bundle.toml");
    std::fs::write(&bundle_toml_path, bundle_toml).expect("write bundle.toml");

    // Generate the python guest glue (generated/guest/contracts.py, …) + manifest.
    let gen_dir: PathBuf = bundle_dir.join("generated");
    let config: GenerateConfig = GenerateConfig {
        api_toml: bundle_toml_path,
        lang: Lang::Python,
        side: Side::Guest,
        out_dir: gen_dir.clone(),
    };
    let output: GenerateOutput = polyplugc::generate(config).expect("polyplugc generate (python)");
    for file in &output.files {
        let file_path: PathBuf = gen_dir.join(&file.path);
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent).expect("create generated parent dir");
        }
        std::fs::write(&file_path, &file.content).expect("write generated file");
    }

    // The loader discovers manifest.toml at the bundle root.
    std::fs::rename(
        gen_dir.join("manifest.toml"),
        bundle_dir.join("manifest.toml"),
    )
    .expect("move manifest.toml to bundle root");

    // The only hand-written source: a duck-typed impl whose `value()` returns the
    // per-runtime discriminator, registered via the generated factory. The factory
    // receives the HostApi pointer at `polyplug_init` time (no SDK-level host
    // storage); this impl ignores it. `polyplug_init` is re-exported so the loader
    // finds it at the entry module's top level.
    let entry_py: String = format!(
        "from generated.guest.contracts import set_probe_factory, polyplug_init\n\
         \n\
         \n\
         class ProbeImpl:\n\
         \x20   def __init__(self, host_ptr: int) -> None:\n\
         \x20       self._host_ptr = host_ptr\n\
         \n\
         \x20   def value(self) -> int:\n\
         \x20       return {return_value}\n\
         \n\
         \n\
         set_probe_factory(ProbeImpl)\n",
    );
    std::fs::write(bundle_dir.join("entry.py"), entry_py).expect("write entry.py");

    vendor_python_sdk(&bundle_dir);

    bundle_dir
}

/// Vendor the CURRENT python guest SDK into `<bundle>/site-packages/` (the loader
/// prepends it to `sys.path`). Copies the real SDK — never a stale fixture — so the
/// helpers match the generated glue exactly. Mirrors `integration_peer_caller_python`.
fn vendor_python_sdk(bundle_dir: &Path) {
    let site: PathBuf = bundle_dir.join("site-packages");
    let sdk_root: PathBuf = workspace_root().join("sdks").join("python");

    let guest_dst: PathBuf = site.join("polyplug_guest");
    std::fs::create_dir_all(&guest_dst).expect("create polyplug_guest dir");
    std::fs::copy(
        sdk_root
            .join("guest")
            .join("polyplug_guest")
            .join("__init__.py"),
        guest_dst.join("__init__.py"),
    )
    .expect("vendor polyplug_guest");

    let abi_src: PathBuf = sdk_root.join("polyplug_abi").join("polyplug_abi");
    let abi_dst: PathBuf = site.join("polyplug_abi");
    std::fs::create_dir_all(&abi_dst).expect("create polyplug_abi dir");
    for name in ["__init__.py", "abi.py", "string_view_helper.py"] {
        std::fs::copy(abi_src.join(name), abi_dst.join(name))
            .unwrap_or_else(|e| panic!("vendor polyplug_abi/{name}: {e}"));
    }

    // polyplug_abi.abi falls back to `from polyplug.abi.abi import *`, so the
    // canonical generated ABI module must be reachable as the `polyplug` package.
    let polyplug_abi_pkg: PathBuf = site.join("polyplug").join("abi");
    std::fs::create_dir_all(&polyplug_abi_pkg).expect("create polyplug/abi dir");
    std::fs::write(site.join("polyplug").join("__init__.py"), b"").expect("polyplug __init__");
    std::fs::write(polyplug_abi_pkg.join("__init__.py"), b"").expect("polyplug/abi __init__");
    std::fs::copy(
        sdk_root.join("abi").join("abi.py"),
        polyplug_abi_pkg.join("abi.py"),
    )
    .expect("vendor polyplug/abi/abi.py");
}

/// Dispatch fn 0 (`value`, no args) of `iso.Probe@1` in `rt` and return the i32
/// the guest wrote, asserting the call succeeded.
fn dispatch_probe_value(rt: &Runtime) -> i32 {
    let contract_id: u64 = guest_contract_id("iso.Probe", 1);
    let handle: GuestContractHandle = rt
        .find_guest_contract(contract_id, 0)
        .expect("iso.Probe must be registered after load");
    let vtable_ptr: *const GuestContractInterface = rt
        .resolve_guest_contract(handle)
        .expect("handle must be valid");
    // SAFETY: vtable_ptr is a live interface for the loaded bundle; the Python
    // module stays loaded for the process lifetime.
    let vtable: &GuestContractInterface = unsafe { &*vtable_ptr };
    assert_eq!(
        vtable.dispatch_type,
        DispatchType::VirtualMachine,
        "python loader must use VM dispatch"
    );

    let mut out: i32 = 0;
    let mut err: AbiError = AbiError::ok();
    // SAFETY: the dispatch_type assertion proves the `vm` union variant is active.
    // `value` takes no args (null `args` accepted), `out` points to a live i32
    // matching the declared `i32` return, and a null arena selects the host alloc
    // fallback; all outlive the synchronous call.
    unsafe {
        (vtable.dispatch.vm.call)(
            vtable.dispatch.vm.loader_data,
            GuestContractInstance::null(),
            0,
            core::ptr::null(),
            &mut out as *mut i32 as *mut (),
            core::ptr::null_mut(),
            &mut err as *mut AbiError,
        )
    };
    assert_eq!(
        err.code,
        AbiErrorCode::Ok as u32,
        "iso.Probe.value dispatch must succeed"
    );
    out
}

/// Two `Runtime` instances in one process each load a same-named generated Python
/// bundle whose author factory builds an impl returning a distinct value. If the
/// generated `_probe_IMPL` module global were shared across runtimes (a real
/// cross-runtime leak), the second load would clobber the first and both would
/// observe the same value. The per-load isolation nonce prevents this: each
/// dispatches its OWN value.
#[test]
fn two_runtimes_generated_probe_impl_globals_do_not_collide() {
    let tmp: tempfile::TempDir = tempfile::TempDir::new().expect("tempdir");

    let bundle_a: PathBuf = write_probe_bundle(tmp.path(), "probe_a", 0x11);
    let bundle_b: PathBuf = write_probe_bundle(tmp.path(), "probe_b", 0x22);

    let loader_a: PythonLoader = PythonLoader::new(PythonConfig::default());
    let loader_b: PythonLoader = PythonLoader::new(PythonConfig::default());
    let runtime_a: Arc<Runtime> = Runtime::builder()
        .loader(loader_a)
        .build()
        .expect("build runtime A");
    let runtime_b: Arc<Runtime> = Runtime::builder()
        .loader(loader_b)
        .build()
        .expect("build runtime B");

    runtime_a
        .load_bundle(&bundle_a)
        .expect("runtime A load must succeed");
    runtime_b
        .load_bundle(&bundle_b)
        .expect("runtime B load must succeed");

    let value_a: i32 = dispatch_probe_value(&runtime_a);
    let value_b: i32 = dispatch_probe_value(&runtime_b);

    assert_eq!(
        value_a, 0x11,
        "runtime A must dispatch its OWN generated _probe_IMPL (0x11)"
    );
    assert_eq!(
        value_b, 0x22,
        "runtime B must dispatch its OWN generated _probe_IMPL (0x22), not A's cached module"
    );
}
