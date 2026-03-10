//! Integration tests for the polyplug extension system.
//!
//! This test crate is the crate root for the `integration_extension` test binary.
//! (AGENTS.md Rule 1: module roots use dirname/mod.rs)

#![allow(clippy::expect_used)]

use polyplug::abi::StringView;
use polyplug::extensions::Extension;
use polyplug::extensions::trace::EXT_TRACE_ID;
use polyplug::extensions::trace::TraceExtension;
use polyplug::extensions::trace::TraceVTable;
use polyplug::runtime::Runtime;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Output;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

// ─── Test-only CounterExtension ──────────────────────────────────────────────

struct CounterExtension {
    id: u32,
    ptr: *const (),
}

// SAFETY: CounterExtension holds a *const () pointing to a static vtable.
// The pointer never changes after construction, so Send + Sync are safe.
unsafe impl Send for CounterExtension {}
// SAFETY: Same reasoning as Send — concurrent reads are safe.
unsafe impl Sync for CounterExtension {}

impl Extension for CounterExtension {
    fn extension_id(&self) -> u32 {
        self.id
    }

    fn vtable_ptr(&self) -> *const () {
        self.ptr
    }
}

// ─── Global callback flag ─────────────────────────────────────────────────────

static CALLBACK_FLAG: OnceLock<Arc<AtomicBool>> = OnceLock::new();

// ─── One-time runtime setup ───────────────────────────────────────────────────

fn ensure_runtime_built() {
    static SETUP: OnceLock<()> = OnceLock::new();
    SETUP.get_or_init(|| {
        let flag: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
        let _: Result<(), Arc<AtomicBool>> = CALLBACK_FLAG.set(flag.clone());
        let cb_flag: Arc<AtomicBool> = flag;
        Runtime::builder()
            .extension(Box::new(TraceExtension::new(move |_msg: &str| {
                cb_flag.store(true, Ordering::SeqCst);
            })))
            .build()
            .expect("runtime build must succeed");
    });
}

// ─── Helper: run polyplugc with --bundle ─────────────────────────────────────

fn run_polyplugc_bundle(bundle_toml: &Path, lang: &str, out_dir: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_polyplugc"))
        .arg("generate")
        .arg("--bundle")
        .arg(bundle_toml)
        .arg("--lang")
        .arg(lang)
        .arg("--out")
        .arg(out_dir)
        .output()
        .expect("failed to spawn polyplugc")
}

/// Write the shared bundle.toml fixture to the given path.
fn write_ext_bundle_toml(path: &Path) {
    let content: &str = concat!(
        "[bundle]\n",
        "name = \"test_ext_bundle\"\n",
        "version = \"0.1.0\"\n",
        "\n",
        "[[plugin]]\n",
        "name = \"test_ext_plugin\"\n",
        "version = \"0.1.0\"\n",
        "implements = []\n",
        "optional = [\"trace\"]\n",
    );
    std::fs::write(path, content).expect("failed to write bundle.toml fixture");
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[test]
fn extension_registered_returns_non_null() {
    ensure_runtime_built();
    // SAFETY: polyplug_get_extension reads from OnceLock; no pointer preconditions.
    let ptr: *const () =
        unsafe { polyplug::polyplug_get_extension(core::ptr::null(), EXT_TRACE_ID) };
    assert!(
        !ptr.is_null(),
        "registered extension must return non-null pointer"
    );
}

#[test]
fn extension_absent_returns_null() {
    ensure_runtime_built();
    // SAFETY: polyplug_get_extension reads from OnceLock; no pointer preconditions.
    let ptr: *const () =
        unsafe { polyplug::polyplug_get_extension(core::ptr::null(), 0xDEAD_0000_u32) };
    assert!(
        ptr.is_null(),
        "unknown extension ID must return null pointer"
    );
}

#[test]
fn trace_callback_invoked() {
    ensure_runtime_built();
    // SAFETY: polyplug_get_extension reads from OnceLock; no pointer preconditions.
    let ptr: *const () =
        unsafe { polyplug::polyplug_get_extension(core::ptr::null(), EXT_TRACE_ID) };
    assert!(!ptr.is_null(), "trace extension must be registered");
    let vtable: *const TraceVTable = ptr as *const TraceVTable;
    // SAFETY: ptr is non-null and points to a valid TraceVTable (verified by extension_registered test).
    // The vtable is valid for the Runtime lifetime (leaked Box).
    // (*vtable).state is the non-null TraceState pointer set in TraceExtension::new.
    unsafe {
        ((*vtable).emit)(StringView::from_static(b"hello"), (*vtable).state);
    }
    let flag: bool = CALLBACK_FLAG
        .get()
        .expect("CALLBACK_FLAG set in ensure_runtime_built")
        .load(Ordering::SeqCst);
    assert!(flag, "trace callback must have been invoked");
}

#[test]
fn counter_extension_custom() {
    let ce: CounterExtension = CounterExtension {
        id: 0xAABB_CCDD_u32,
        ptr: &() as *const () as *const _,
    };
    assert_eq!(
        ce.extension_id(),
        0xAABB_CCDD_u32,
        "extension_id must match"
    );
    assert!(!ce.vtable_ptr().is_null(), "vtable_ptr must be non-null");
}

// ─── Codegen tests: each generator must emit EXT_TRACE_ID when optional=["trace"] ───

#[test]
fn codegen_rust_emits_ext_trace_id() {
    let tmp: PathBuf = std::env::temp_dir().join("polyplug_ext_test_rust");
    std::fs::create_dir_all(&tmp).expect("create temp dir");
    let bundle_path: PathBuf = tmp.join("bundle.toml");
    write_ext_bundle_toml(&bundle_path);
    let out_dir: PathBuf = tmp.join("out");
    let output: Output = run_polyplugc_bundle(&bundle_path, "rust", &out_dir);
    assert!(
        output.status.success(),
        "polyplugc rust failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let init_content: String =
        std::fs::read_to_string(out_dir.join("guest/init.rs")).expect("guest/init.rs not found");
    assert!(
        init_content.contains("EXT_TRACE_ID"),
        "rust init.rs must contain EXT_TRACE_ID; got:\n{init_content}"
    );
}

#[test]
fn codegen_cpp_emits_ext_trace_id() {
    let tmp: PathBuf = std::env::temp_dir().join("polyplug_ext_test_cpp");
    std::fs::create_dir_all(&tmp).expect("create temp dir");
    let bundle_path: PathBuf = tmp.join("bundle.toml");
    write_ext_bundle_toml(&bundle_path);
    let out_dir: PathBuf = tmp.join("out");
    let output: Output = run_polyplugc_bundle(&bundle_path, "cpp", &out_dir);
    assert!(
        output.status.success(),
        "polyplugc cpp failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let init_content: String =
        std::fs::read_to_string(out_dir.join("guest/init.hpp")).expect("guest/init.hpp not found");
    assert!(
        init_content.contains("EXT_TRACE_ID"),
        "cpp init.hpp must contain EXT_TRACE_ID; got:\n{init_content}"
    );
}

#[test]
fn codegen_csharp_emits_ext_trace_id() {
    let tmp: PathBuf = std::env::temp_dir().join("polyplug_ext_test_csharp");
    std::fs::create_dir_all(&tmp).expect("create temp dir");
    let bundle_path: PathBuf = tmp.join("bundle.toml");
    write_ext_bundle_toml(&bundle_path);
    let out_dir: PathBuf = tmp.join("out");
    let output: Output = run_polyplugc_bundle(&bundle_path, "csharp", &out_dir);
    assert!(
        output.status.success(),
        "polyplugc csharp failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    // C# generator emits ExtTraceId (PascalCase) rather than EXT_TRACE_ID.
    let init_content: String =
        std::fs::read_to_string(out_dir.join("guest/Init.cs")).expect("guest/Init.cs not found");
    assert!(
        init_content.contains("ExtTraceId"),
        "csharp Init.cs must contain ExtTraceId; got:\n{init_content}"
    );
}

#[test]
fn codegen_python_emits_ext_trace_id() {
    let tmp: PathBuf = std::env::temp_dir().join("polyplug_ext_test_python");
    std::fs::create_dir_all(&tmp).expect("create temp dir");
    let bundle_path: PathBuf = tmp.join("bundle.toml");
    write_ext_bundle_toml(&bundle_path);
    let out_dir: PathBuf = tmp.join("out");
    let output: Output = run_polyplugc_bundle(&bundle_path, "python", &out_dir);
    assert!(
        output.status.success(),
        "polyplugc python failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let init_content: String =
        std::fs::read_to_string(out_dir.join("guest/init.py")).expect("guest/init.py not found");
    assert!(
        init_content.contains("EXT_TRACE_ID"),
        "python init.py must contain EXT_TRACE_ID; got:\n{init_content}"
    );
}

#[test]
fn codegen_lua_emits_ext_trace_id() {
    let tmp: PathBuf = std::env::temp_dir().join("polyplug_ext_test_lua");
    std::fs::create_dir_all(&tmp).expect("create temp dir");
    let bundle_path: PathBuf = tmp.join("bundle.toml");
    write_ext_bundle_toml(&bundle_path);
    let out_dir: PathBuf = tmp.join("out");
    let output: Output = run_polyplugc_bundle(&bundle_path, "lua", &out_dir);
    assert!(
        output.status.success(),
        "polyplugc lua failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let init_content: String =
        std::fs::read_to_string(out_dir.join("guest/init.lua")).expect("guest/init.lua not found");
    assert!(
        init_content.contains("EXT_TRACE_ID"),
        "lua init.lua must contain EXT_TRACE_ID; got:\n{init_content}"
    );
}

#[test]
fn codegen_js_quickjs_emits_ext_trace_id() {
    let tmp: PathBuf = std::env::temp_dir().join("polyplug_ext_test_js_quickjs");
    std::fs::create_dir_all(&tmp).expect("create temp dir");
    let bundle_path: PathBuf = tmp.join("bundle.toml");
    write_ext_bundle_toml(&bundle_path);
    let out_dir: PathBuf = tmp.join("out");
    let output: Output = run_polyplugc_bundle(&bundle_path, "js-quickjs", &out_dir);
    assert!(
        output.status.success(),
        "polyplugc js-quickjs failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let init_content: String =
        std::fs::read_to_string(out_dir.join("guest/init.ts")).expect("guest/init.ts not found");
    assert!(
        init_content.contains("EXT_TRACE_ID"),
        "js-quickjs init.ts must contain EXT_TRACE_ID; got:\n{init_content}"
    );
}

#[test]
fn codegen_js_deno_emits_ext_trace_id() {
    let tmp: PathBuf = std::env::temp_dir().join("polyplug_ext_test_js_deno");
    std::fs::create_dir_all(&tmp).expect("create temp dir");
    let bundle_path: PathBuf = tmp.join("bundle.toml");
    write_ext_bundle_toml(&bundle_path);
    let out_dir: PathBuf = tmp.join("out");
    let output: Output = run_polyplugc_bundle(&bundle_path, "js-deno", &out_dir);
    assert!(
        output.status.success(),
        "polyplugc js-deno failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let init_content: String =
        std::fs::read_to_string(out_dir.join("guest/init.ts")).expect("guest/init.ts not found");
    assert!(
        init_content.contains("EXT_TRACE_ID"),
        "js-deno init.ts must contain EXT_TRACE_ID; got:\n{init_content}"
    );
}
