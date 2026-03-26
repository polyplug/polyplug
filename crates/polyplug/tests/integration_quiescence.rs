//! Integration test: hot-reload quiescence timeout.
//! Verifies that reload_bundle returns Err(QuiescenceTimeout) when an in-flight
//! guard is held past the timeout window.
//!
//! Takes ~7 seconds due to timeout verification.

#![allow(clippy::expect_used)]

use polyplug::error::PolyplugError;
use polyplug::registry::Registry;
use polyplug::runtime::Runtime;

#[test]
fn test_quiescence_timeout() {
    // Build runtime and load v1.
    let rt: Runtime = Runtime::builder()
        .build()
        .expect("runtime build must succeed");
    let v1_dir: &str = env!("RELOAD_PLUGIN_V1_DIR");
    rt.load_bundle(std::path::Path::new(v1_dir))
        .expect("load v1 must succeed");

    // Get contract_id for the reload fixture from build.rs env var.
    // RELOAD_PLUGIN_CONTRACT_ID = fnv1a_64("reload.test@1") = 0xE55B8A5A3DC7C061
    let contract_id_str: &str = env!("RELOAD_PLUGIN_CONTRACT_ID");
    let contract_id: u64 = contract_id_str
        .parse::<u64>()
        .expect("RELOAD_PLUGIN_CONTRACT_ID must be a valid u64");

    // Find a handle for the loaded plugin.
    let mut handles: [polyplug_abi::PluginHandle; 4] = [polyplug_abi::PluginHandle {
        index: 0u32,
        generation: 0u32,
    }; 4];
    let count: usize = rt.find_all_by_contract(contract_id, 0_u32, &mut handles);
    assert!(
        count > 0,
        "must find at least one plugin for contract_id={:#x}",
        contract_id
    );
    let handle: polyplug_abi::PluginHandle = handles[0];

    // Pass PluginHandle fields (both Copy u32) to the background thread.
    // PluginHandle is a plain struct — both fields are Send/Copy.
    let index: u32 = handle.index;
    let generation: u32 = handle.generation;

    // Clone registry Arc for the background thread (Arc<Registry> is Send).
    let registry_arc: std::sync::Arc<Registry> = std::sync::Arc::clone(rt.registry());

    let hold_thread: std::thread::JoinHandle<()> = std::thread::spawn(move || {
        // Reconstruct handle on this thread.
        let h: polyplug_abi::PluginHandle = polyplug_abi::PluginHandle { index, generation };
        // Resolve guard HERE — PluginGuard is !Send, must stay on this thread.
        let guard: polyplug::registry::PluginGuard = registry_arc
            .resolve_guard(h)
            .expect("resolve_guard must succeed for loaded plugin");
        // Hold for 7s — longer than the 5s QUIESCENCE_TIMEOUT.
        std::thread::sleep(core::time::Duration::from_secs(7_u64));
        drop(guard);
    });

    // Give the background thread time to acquire the guard before attempting reload.
    std::thread::sleep(core::time::Duration::from_millis(100));

    let v2_dir: &str = env!("RELOAD_PLUGIN_V2_DIR");
    let result: Result<(), PolyplugError> = rt.reload_bundle(std::path::Path::new(v2_dir));

    // Join the background thread (it will finish after QUIESCENCE_TIMEOUT fires).
    hold_thread.join().expect("hold thread must not panic");

    match result {
        Err(PolyplugError::QuiescenceTimeout { .. }) => {
            // Expected — test passes.
        }
        Err(e) => panic!("Expected QuiescenceTimeout, got: {:?}", e),
        Ok(()) => panic!("Expected QuiescenceTimeout, got Ok(())"),
    }

    // Verify runtime is healthy: retry reload now that guard is dropped.
    let result2: Result<(), PolyplugError> = rt.reload_bundle(std::path::Path::new(v2_dir));
    assert!(
        result2.is_ok(),
        "second reload must succeed after guard is released: {:?}",
        result2
    );
}
