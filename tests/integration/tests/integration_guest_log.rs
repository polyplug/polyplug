//! Runtime test: `polyplug_guest::log` delivers through a REAL loaded bundle.
//!
//! The guest SDK's `log` helper had zero consumers and zero tests — exactly the
//! never-run surface class this suite exists to close. The flow proven here:
//!
//! 1. the host installs a capturing logger via `RuntimeBuilder::logger`
//!    (RuntimeConfig.log funnel),
//! 2. the runtime loads the native `test_plugin` fixture bundle, whose
//!    `polyplug_init` stores the host vtable via
//!    `polyplug_guest::store_host_vtable`,
//! 3. the test drives the fixture's `polyplug_test_guest_log` probe (resolved
//!    via dlsym from the same resident image), which calls
//!    `polyplug_guest::log(Info, "guest.test_plugin", ...)`,
//! 4. the record must arrive verbatim in the host-installed logger.

#![allow(clippy::expect_used)]

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

use polyplug::runtime::Runtime;
use polyplug::runtime::RuntimeBuilder;
use polyplug_abi::types::LogLevel;
use polyplug_native::{NativeConfig, NativeLoader};

const TEST_PLUGIN_DIR: &str = env!("TEST_PLUGIN_DIR");

/// Platform-specific shared-library filename inside the fixture bundle dir.
/// The probe MUST be resolved from the bundle-dir copy — the file the runtime
/// dlopened — not the sibling fixtures/ copy: two distinct paths are two
/// distinct images, and the host vtable stored by `polyplug_init` lives only
/// in the image the runtime loaded.
fn plugin_lib_filename() -> &'static str {
    if cfg!(target_os = "macos") {
        "libtest_plugin.dylib"
    } else if cfg!(target_os = "windows") {
        "test_plugin.dll"
    } else {
        "libtest_plugin.so"
    }
}

#[test]
fn polyplug_guest_log_delivers_to_host_logger() {
    let records: Arc<Mutex<Vec<(LogLevel, String, String)>>> = Arc::new(Mutex::new(Vec::new()));
    let records_clone: Arc<Mutex<Vec<(LogLevel, String, String)>>> = Arc::clone(&records);

    let runtime: Arc<Runtime> = RuntimeBuilder::new()
        .loader(NativeLoader::new(NativeConfig::default()))
        .logger(move |level: LogLevel, scope: &str, msg: &str| {
            records_clone.lock().expect("records lock").push((
                level,
                scope.to_owned(),
                msg.to_owned(),
            ));
        })
        .build()
        .expect("runtime build must succeed");

    runtime
        .load_bundle(&PathBuf::from(TEST_PLUGIN_DIR))
        .expect("native test_plugin bundle must load");

    // Resolve the probe from the SAME image the runtime loaded (dlopen of the
    // same path ref-counts the already-resident library, so the host vtable
    // stored by polyplug_init during the runtime load is visible to the probe).
    let plugin_path: PathBuf = PathBuf::from(TEST_PLUGIN_DIR).join(plugin_lib_filename());
    // SAFETY: the library is the freshly built trusted test fixture.
    let library: libloading::Library =
        unsafe { libloading::Library::new(&plugin_path) }.expect("fixture .so must dlopen");
    // SAFETY: the fixture exports this exact no-arg extern "C" symbol.
    let probe: libloading::Symbol<'_, unsafe extern "C" fn()> = unsafe {
        library
            .get(b"polyplug_test_guest_log")
            .expect("probe symbol must resolve")
    };
    // SAFETY: probe is a valid no-arg extern "C" fn exported by the fixture.
    unsafe { probe() };

    let captured: Vec<(LogLevel, String, String)> = records.lock().expect("records lock").clone();
    assert!(
        captured.contains(&(
            LogLevel::Info,
            String::from("guest.test_plugin"),
            String::from("hello from polyplug_guest::log"),
        )),
        "expected the guest SDK log record to reach the host-installed logger, got: {captured:?}"
    );
}
