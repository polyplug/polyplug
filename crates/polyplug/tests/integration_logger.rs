//! Integration tests: host-supplied logger callback (`RuntimeConfig::log`).
//!
//! Covers delivery through both host entry points:
//! - the raw `RuntimeConfig` fields (extern "C" callback + user_data), and
//! - the `RuntimeBuilder::logger` Rust-closure wrapper.
//!
//! The Warn-path event used throughout is a scan diagnostic: a bundle
//! directory whose `manifest.toml` is unparseable produces a
//! `scan: ...` warning during `build()` (scope `"builder"`).

#![allow(clippy::expect_used)]

use core::ffi::c_void;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use polyplug::runtime::Runtime;
use polyplug_abi::runtime::RuntimeConfig;
use polyplug_abi::types::{LogLevel, StringView};
use tempfile::TempDir;

/// Create a plugin dir containing one bundle with a malformed manifest.toml.
fn dir_with_malformed_manifest() -> TempDir {
    let tmp: TempDir = TempDir::new().expect("create tempdir");
    let bundle_dir: PathBuf = tmp.path().join("broken_bundle");
    fs::create_dir_all(&bundle_dir).expect("create bundle dir");
    fs::write(
        bundle_dir.join("manifest.toml"),
        "this is { not [ valid toml",
    )
    .expect("write malformed manifest");
    tmp
}

#[test]
fn builder_logger_closure_receives_level_scope_message() {
    let tmp: TempDir = dir_with_malformed_manifest();

    let records: Arc<Mutex<Vec<(LogLevel, String, String)>>> = Arc::new(Mutex::new(Vec::new()));
    let records_clone: Arc<Mutex<Vec<(LogLevel, String, String)>>> = Arc::clone(&records);

    let runtime: Arc<Runtime> = Runtime::builder()
        .plugin_dir(tmp.path().to_path_buf())
        .logger(move |level: LogLevel, scope: &str, msg: &str| {
            records_clone.lock().expect("records lock").push((
                level,
                scope.to_owned(),
                msg.to_owned(),
            ));
        })
        .build()
        .expect("build runtime");
    drop(runtime);

    let captured: Vec<(LogLevel, String, String)> = records.lock().expect("records lock").clone();
    assert!(
        captured.iter().any(|(level, scope, msg)| {
            *level == LogLevel::Warn && scope == "builder" && msg.starts_with("scan:")
        }),
        "expected a (Warn, \"builder\", \"scan: ...\") record, got: {captured:?}"
    );
}

/// Raw extern "C" callback writing into the `Mutex<Vec<...>>` behind
/// `user_data` (no statics).
unsafe extern "C" fn capture_log(
    user_data: *mut c_void,
    level: u32,
    scope: StringView,
    message: StringView,
) {
    // SAFETY: each test passes a pointer to a Box<Mutex<Vec<...>>> sink that
    // outlives the runtime emitting the calls.
    let sink: &Mutex<Vec<(u32, String, String)>> =
        unsafe { &*(user_data as *const Mutex<Vec<(u32, String, String)>>) };
    // SAFETY: the runtime guarantees both views are valid UTF-8 for the
    // duration of the call; the bytes are copied immediately.
    let (scope_owned, message_owned): (String, String) =
        unsafe { (scope.as_str().to_owned(), message.as_str().to_owned()) };
    sink.lock()
        .expect("sink lock")
        .push((level, scope_owned, message_owned));
}

#[test]
fn raw_config_callback_receives_warn_event() {
    let tmp: TempDir = dir_with_malformed_manifest();

    let sink: Box<Mutex<Vec<(u32, String, String)>>> = Box::new(Mutex::new(Vec::new()));
    let config = RuntimeConfig {
        log: Some(capture_log),
        log_user_data: (&*sink) as *const Mutex<Vec<(u32, String, String)>> as *mut c_void,
        log_max_level: LogLevel::Trace as u32,
        ..Default::default()
    };

    let runtime: Arc<Runtime> = Runtime::builder()
        .config(config)
        .plugin_dir(tmp.path().to_path_buf())
        .build()
        .expect("build runtime");
    drop(runtime);

    let captured: Vec<(u32, String, String)> = sink.lock().expect("sink lock").clone();
    assert!(
        captured.iter().any(|(level, scope, msg)| {
            *level == LogLevel::Warn as u32 && scope == "builder" && msg.starts_with("scan:")
        }),
        "expected a (2, \"builder\", \"scan: ...\") record, got: {captured:?}"
    );
}

#[test]
fn raw_config_max_level_error_filters_warn() {
    let tmp: TempDir = dir_with_malformed_manifest();

    let sink: Box<Mutex<Vec<(u32, String, String)>>> = Box::new(Mutex::new(Vec::new()));
    let config = RuntimeConfig {
        log: Some(capture_log),
        log_user_data: (&*sink) as *const Mutex<Vec<(u32, String, String)>> as *mut c_void,
        log_max_level: LogLevel::Error as u32,
        ..Default::default()
    };

    let runtime: Arc<Runtime> = Runtime::builder()
        .config(config)
        .plugin_dir(tmp.path().to_path_buf())
        .build()
        .expect("build runtime");
    drop(runtime);

    let captured: Vec<(u32, String, String)> = sink.lock().expect("sink lock").clone();
    assert!(
        captured.is_empty(),
        "max_level=Error must filter the Warn scan diagnostic, got: {captured:?}"
    );
}

/// `HostApi.log` (guest-side logging) routes into the same funnel as
/// `RuntimeConfig::log`: a guest calling `host->log(host, ...)` is delivered to
/// the builder-installed callback; unknown level values are clamped to Error;
/// null views are legal and read as "".
#[test]
fn host_api_log_routes_guest_records_into_host_logger() {
    let sink: Box<Mutex<Vec<(u32, String, String)>>> = Box::new(Mutex::new(Vec::new()));
    let config = RuntimeConfig {
        log: Some(capture_log),
        log_user_data: (&*sink) as *const Mutex<Vec<(u32, String, String)>> as *mut c_void,
        log_max_level: LogLevel::Trace as u32,
        ..Default::default()
    };
    let runtime: Arc<Runtime> = Runtime::builder()
        .config(config)
        .build()
        .expect("build runtime");

    let host: *const polyplug_abi::HostApi = runtime.host_abi();
    let scope: &str = "guest.test_plugin";
    let message: &str = "hello from a guest";
    let scope_view = StringView {
        ptr: scope.as_ptr(),
        len: scope.len(),
    };
    let message_view = StringView {
        ptr: message.as_ptr(),
        len: message.len(),
    };
    // SAFETY: host is the runtime's live HostApi; both views borrow local UTF-8
    // string data valid for the duration of each synchronous call.
    unsafe {
        ((*host).log)(host, LogLevel::Info as u32, scope_view, message_view);
        // Unknown level (plugins are untrusted): must clamp to Error, not UB.
        ((*host).log)(host, 999, scope_view, message_view);
        // Null views are legal at the ABI boundary and read as "".
        ((*host).log)(
            host,
            LogLevel::Warn as u32,
            StringView::null(),
            StringView::null(),
        );
    }

    let captured: Vec<(u32, String, String)> = sink.lock().expect("sink lock").clone();
    assert_eq!(
        captured,
        vec![
            (
                LogLevel::Info as u32,
                String::from("guest.test_plugin"),
                String::from("hello from a guest"),
            ),
            (
                LogLevel::Error as u32,
                String::from("guest.test_plugin"),
                String::from("hello from a guest"),
            ),
            (LogLevel::Warn as u32, String::new(), String::new()),
        ]
    );
}
