#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

//! Test for HR-06: Warning emission when Arc refs remain after Preparing callback.
//!
//! This test verifies that:
//! 1. The warning callback mechanism works correctly during reload
//! 2. The warning check happens AFTER Preparing callback, BEFORE loader.reload()
//! 3. The warning message contains expected content when it fires
//! 4. Reload works even without a warning callback (falls back to stderr)
//!
//! Run with:
//!   cargo test -p integration --test integration_hot_reload_warning -- --test-threads=1

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

use polyplug::ReloadPhase;
use polyplug::runtime::Runtime;
use polyplug_native::{NativeLoader, NativeConfig};

// ─── Environment variables emitted by build.rs ───────────────────────────────

const RELOAD_V1_DIR: &str = env!("RELOAD_PLUGIN_V1_DIR");
const RELOAD_V2_DIR: &str = env!("RELOAD_PLUGIN_V2_DIR");

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn v2_so_path() -> PathBuf {
    PathBuf::from(RELOAD_V2_DIR).join("libreload_plugin_v2.so")
}

fn hot_reload_config() -> polyplug::RuntimeConfig {
    polyplug::RuntimeConfig {
        hot_reload_enabled: true,
        hot_reload_max_retries: 3,
        hot_reload_retry_interval_ms: 1000,
        hot_reload_abort_on_max_retries: true,
        compatibility: polyplug::Compatibility::Strict,
    }
}

fn make_hot_reload_runtime() -> Runtime {
    Runtime::builder()
        .loader(NativeLoader::new(NativeConfig::default()))
        .config(hot_reload_config())
        .build()
        .expect("build runtime")
}

// ─── HR-06 Tests ─────────────────────────────────────────────────────────────

/// Test that warning callback mechanism works during reload.
///
/// HR-06: Host sees UB warning if Arc refs remain after Preparing callback returns.
///
/// This test verifies the warning callback receives messages during reload.
/// The Arc::strong_count check fires when get_interface_arc clones the Arc
/// for checking, resulting in strong_count > 1 temporarily.
#[test]
fn test_warning_callback_invoked_during_reload() {
    // Capture warning messages
    let warnings: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let warnings_clone: Arc<Mutex<Vec<String>>> = Arc::clone(&warnings);

    // Create runtime with warning callback
    let rt: Runtime = Runtime::builder()
        .loader(NativeLoader::new(NativeConfig::default()))
        .config(hot_reload_config())
        .on_warning(move |msg: &str| {
            warnings_clone
                .lock()
                .unwrap()
                .push(msg.to_owned());
        })
        .build()
        .expect("build runtime");

    // Load the v1 bundle - this registers interfaces in the registry
    rt.load_bundle(std::path::Path::new(RELOAD_V1_DIR))
        .expect("load v1");

    // Clear any existing warnings
    warnings.lock().unwrap().clear();

    // Call reload_bundle - the warning check happens after Preparing callback
    // Due to implementation (get_interface_arc clones Arc), strong_count > 1 triggers warning
    rt.reload_bundle(v2_so_path().as_path())
        .expect("reload should succeed");

    // Check that warnings were captured (callback mechanism works)
    let captured_warnings: Vec<String> =
        warnings.lock().unwrap().clone();

    // The warning should have been emitted with "Potential UB" substring
    let has_potential_ub_warning: bool = captured_warnings
        .iter()
        .any(|w| w.contains("Potential UB"));

    assert!(
        has_potential_ub_warning,
        "Expected 'Potential UB' warning. Captured warnings: {:?}",
        captured_warnings
    );
}

/// Test that warning check happens AFTER Preparing callback, BEFORE loader.reload().
///
/// The warning check is at reload.rs lines 128-140:
/// - After Preparing callback fires
/// - Before loader.reload() is called
///
/// This test verifies the ordering by capturing both reload phases and warnings.
#[test]
fn test_warning_timing_after_preparing_before_reloaded() {
    // Capture both phases and warnings in order
    let events: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

    // Create runtime with both callbacks
    let rt: Runtime = Runtime::builder()
        .loader(NativeLoader::new(NativeConfig::default()))
        .config(hot_reload_config())
        .on_warning({
            let events = Arc::clone(&events);
            move |msg: &str| {
                events
                    .lock()
                    .unwrap()
                    .push(format!("WARNING: {}", msg));
            }
        })
        .on_reload({
            let events = Arc::clone(&events);
            move |phase: ReloadPhase| {
                let label: &str = match phase {
                    ReloadPhase::Preparing { .. } => "Preparing",
                    ReloadPhase::Reloaded { .. } => "Reloaded",
                    ReloadPhase::Failed { .. } => "Failed",
                };
                events
                    .lock()
                    .unwrap()
                    .push(format!("PHASE: {}", label));
            }
        })
        .build()
        .expect("build runtime");

    // Load the v1 bundle
    rt.load_bundle(std::path::Path::new(RELOAD_V1_DIR))
        .expect("load v1");

    // Clear events
    events.lock().unwrap().clear();

    // Call reload_bundle
    rt.reload_bundle(v2_so_path().as_path())
        .expect("reload should succeed");

    // Check event order
    let captured_events: Vec<String> =
        events.lock().unwrap().clone();

    // Find indices of key events
    let preparing_idx: Option<usize> = captured_events
        .iter()
        .position(|e| e.starts_with("PHASE: Preparing"));

    let warning_idx: Option<usize> = captured_events
        .iter()
        .position(|e| e.starts_with("WARNING: Potential UB"));

    let reloaded_idx: Option<usize> = captured_events
        .iter()
        .position(|e| e.starts_with("PHASE: Reloaded"));

    // Preparing should always fire
    assert!(
        preparing_idx.is_some(),
        "Preparing phase should have fired. Events: {:?}",
        captured_events
    );

    // Reloaded should fire after successful reload
    assert!(
        reloaded_idx.is_some(),
        "Reloaded phase should have fired. Events: {:?}",
        captured_events
    );

    // Warning should fire between Preparing and Reloaded
    if let (Some(prep_idx), Some(warn_idx), Some(reload_idx)) =
        (preparing_idx, warning_idx, reloaded_idx)
    {
        assert!(
            prep_idx < warn_idx,
            "Warning should come AFTER Preparing. Events: {:?}",
            captured_events
        );

        assert!(
            warn_idx < reload_idx,
            "Reloaded should come AFTER Warning. Events: {:?}",
            captured_events
        );
    }
}

/// Test that warning message contains expected content when it fires.
///
/// The warning message should:
/// - Mention "Potential UB"
/// - Mention the bundle name
/// - Be informational (reload proceeds anyway)
#[test]
fn test_warning_message_content_structure() {
    // Capture warning messages
    let warnings: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let warnings_clone: Arc<Mutex<Vec<String>>> = Arc::clone(&warnings);

    // Create runtime with warning callback
    let rt: Runtime = Runtime::builder()
        .loader(NativeLoader::new(NativeConfig::default()))
        .config(hot_reload_config())
        .on_warning(move |msg: &str| {
            warnings_clone
                .lock()
                .unwrap()
                .push(msg.to_owned());
        })
        .build()
        .expect("build runtime");

    // Load the v1 bundle
    rt.load_bundle(std::path::Path::new(RELOAD_V1_DIR))
        .expect("load v1");

    // Clear warnings
    warnings.lock().unwrap().clear();

    // Call reload_bundle
    let result = rt.reload_bundle(v2_so_path().as_path());

    // Reload should succeed regardless of warning (informational only)
    assert!(
        result.is_ok(),
        "reload should succeed even if warning fires (informational only)"
    );

    // Check captured warnings
    let captured_warnings: Vec<String> =
        warnings.lock().unwrap().clone();

    // Find the Potential UB warning
    let ub_warning: Option<&String> = captured_warnings
        .iter()
        .find(|w| w.contains("Potential UB"));

    if let Some(warning) = ub_warning {
        // Check for expected message components:
        // 1. "Potential UB" substring
        // 2. Bundle name reference
        // 3. "Proceeding" indicating informational nature
        assert!(
            warning.contains("Potential UB"),
            "Warning should contain 'Potential UB'"
        );
        assert!(
            warning.contains("reload_plugin_v1"),
            "Warning should mention bundle name. Got: {}", warning
        );
        assert!(
            warning.contains("Proceeding"),
            "Warning should indicate reload proceeds anyway. Got: {}", warning
        );
    }
}

/// Test that reload works without a warning callback.
///
/// When no warning callback is registered, emit_warning falls back to stderr.
/// This test verifies that reload works even without a warning callback.
#[test]
fn test_reload_works_without_warning_callback() {
    // Create runtime WITHOUT warning callback
    let rt: Runtime = make_hot_reload_runtime();

    // Load the v1 bundle
    rt.load_bundle(std::path::Path::new(RELOAD_V1_DIR))
        .expect("load v1");

    // Call reload_bundle - should work even without warning callback
    let result = rt.reload_bundle(v2_so_path().as_path());

    // Reload should succeed - warning callback is optional
    // Note: The warning about Arc refs may still be printed to stderr
    assert!(
        result.is_ok(),
        "reload should succeed without warning callback: {:?}",
        result.err()
    );
}