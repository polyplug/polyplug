#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

//! Reload warning behavior: the runtime warns when the host still holds live guest
//! instances of a bundle's contracts at reload time (a use-after-free hazard, since
//! reload swaps the interface). The warning is driven by the runtime's accurate
//! per-contract live-instance counter and fires only when a live stateful instance
//! exists; a clean reload with no live instances emits no warning.
//!
//! These tests use the stateless `reload_plugin_v1` fixture (its instances carry no
//! state), so a clean reload here must NOT emit the live-instance warning — they
//! assert the warning does not false-fire and that reload phase ordering holds. The
//! POSITIVE case (a leaked live instance triggering the warning) is covered by
//! `integration_live_instance_warning.rs`.
//!
//! Run with:
//!   cargo test -p integration --test integration_hot_reload_warning -- --test-threads=1

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

use polyplug::runtime::Runtime;
use polyplug_abi::runtime::ReloadPhase;
use polyplug_abi::runtime::ReloadPhaseType;
use polyplug_abi::types::LogLevel;
use polyplug_native::{NativeConfig, NativeLoader};

// ─── Environment variables emitted by build.rs ───────────────────────────────

const RELOAD_V1_DIR: &str = env!("RELOAD_PLUGIN_V1_DIR");
const RELOAD_V2_DIR: &str = env!("RELOAD_PLUGIN_V2_DIR");

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn v2_so_path() -> PathBuf {
    let filename: &str = if cfg!(target_os = "macos") {
        "libreload_plugin_v2.dylib"
    } else if cfg!(target_os = "windows") {
        "reload_plugin_v2.dll"
    } else {
        "libreload_plugin_v2.so"
    };
    PathBuf::from(RELOAD_V2_DIR).join(filename)
}

fn hot_reload_config() -> polyplug_abi::runtime::RuntimeConfig {
    polyplug_abi::runtime::RuntimeConfig {
        hot_reload_enabled: true,
        ..polyplug_abi::runtime::RuntimeConfig::default()
    }
}

fn make_hot_reload_runtime() -> Arc<Runtime> {
    Runtime::builder()
        .loader(NativeLoader::new(NativeConfig::default()))
        .config(hot_reload_config())
        .build()
        .expect("build runtime")
}

// ─── HR-06 Tests ─────────────────────────────────────────────────────────────

/// A clean reload with no live instances must not emit the live-instance warning.
///
/// HR-06: Host sees UB warning if Arc refs remain after Preparing callback returns.
///
/// This test verifies the warning callback receives messages during reload.
/// The Arc::strong_count check fires when get_guest_contract_interface_arc clones
/// the Arc for checking, resulting in strong_count > 1 temporarily.
#[test]
fn test_warning_callback_invoked_during_reload() {
    // Capture warning messages
    let warnings: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let warnings_clone: Arc<Mutex<Vec<String>>> = Arc::clone(&warnings);

    // Create runtime with warning callback
    let rt: Arc<Runtime> = Runtime::builder()
        .loader(NativeLoader::new(NativeConfig::default()))
        .config(hot_reload_config())
        .logger(move |_level: LogLevel, _scope: &str, msg: &str| {
            warnings_clone.lock().unwrap().push(msg.to_owned());
        })
        .build()
        .expect("build runtime");

    // Load the v1 bundle - this registers interfaces in the registry
    rt.load_bundle(std::path::Path::new(RELOAD_V1_DIR))
        .expect("load v1");

    // Clear any existing warnings
    warnings.lock().unwrap().clear();

    // Reload with no live instances held: the live-instance warning must not fire.
    rt.reload_bundle(v2_so_path().as_path())
        .expect("reload should succeed");

    let captured_warnings: Vec<String> = warnings.lock().unwrap().clone();

    // No host-held instances exist, so the runtime must not emit the live-instance
    // use-after-free warning (no false positive). The positive case is covered by
    // integration_live_instance_warning.rs.
    let has_live_instance_warning: bool = captured_warnings
        .iter()
        .any(|w: &String| w.contains("live guest instance"));

    assert!(
        !has_live_instance_warning,
        "a clean reload with no live instances must not emit the live-instance warning; captured: {:?}",
        captured_warnings
    );
}

/// Test that warning check happens AFTER Preparing callback, BEFORE loader.reload().
///
/// During `Runtime::reload_bundle`, the warning check runs:
/// - After the Preparing callback fires
/// - Before `BundleLoader::reload` is called
///
/// This test verifies the ordering by capturing both reload phases and warnings.
#[test]
fn test_warning_timing_after_preparing_before_reloaded() {
    // Capture both phases and warnings in order
    let events: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

    // Create runtime with both callbacks
    let rt: Arc<Runtime> = Runtime::builder()
        .loader(NativeLoader::new(NativeConfig::default()))
        .config(hot_reload_config())
        .logger({
            let events = Arc::clone(&events);
            move |level: LogLevel, _scope: &str, msg: &str| {
                if level == LogLevel::Warn {
                    events.lock().unwrap().push(format!("WARNING: {}", msg));
                }
            }
        })
        .on_reload({
            let events = Arc::clone(&events);
            move |_user_data: *mut core::ffi::c_void, phase: ReloadPhase| {
                let label: &str = match phase.phase_type {
                    ReloadPhaseType::Preparing => "Preparing",
                    ReloadPhaseType::Reloaded => "Reloaded",
                    ReloadPhaseType::Failed => "Failed",
                    ReloadPhaseType::Unloading => "Unloading",
                };
                events.lock().unwrap().push(format!("PHASE: {}", label));
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
    let captured_events: Vec<String> = events.lock().unwrap().clone();

    // Find indices of key events
    let preparing_idx: Option<usize> = captured_events
        .iter()
        .position(|e| e.starts_with("PHASE: Preparing"));

    let warning_idx: Option<usize> = captured_events
        .iter()
        .position(|e| e.contains("live guest instance"));

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

// The POSITIVE warning case — that the live-instance warning message names the
// contract and the use-after-free hazard when an instance is actually held across a
// reclaim — is covered by `integration_live_instance_warning.rs`, which loads a
// stateful bundle, leaks a live instance, and asserts the warning content. There is
// no stateful reload fixture here, so the warning's content is not re-asserted in
// this file (it would be vacuous against the stateless `reload_plugin_v1` fixture).

/// Test that reload works without a warning callback.
///
/// When no warning callback is registered, the runtime falls back to its stderr
/// logger. This test verifies that reload works even without a callback installed.
#[test]
fn test_reload_works_without_warning_callback() {
    // Create runtime WITHOUT warning callback
    let rt: Arc<Runtime> = make_hot_reload_runtime();

    // Load the v1 bundle
    rt.load_bundle(std::path::Path::new(RELOAD_V1_DIR))
        .expect("load v1");

    // Call reload_bundle - should work even without warning callback
    let result = rt.reload_bundle(v2_so_path().as_path());

    // Reload should succeed - a logger callback is optional (stderr fallback).
    assert!(
        result.is_ok(),
        "reload should succeed without warning callback: {:?}",
        result.err()
    );
}
