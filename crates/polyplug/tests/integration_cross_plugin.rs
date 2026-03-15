//! Integration tests: cross-plugin lookup, multi-impl registry, stale handle detection,
//! and dependency enforcement via the new Epic 9.7 ABI.
//!
//! Tests a–d: pure Registry API (find_by_contract, find_by_bundle, find_all, resolve_guard).
//! Tests e–g: dependency enforcement — see `crates/polyplug/src/runtime/mod.rs`
//!            unit tests (`cross_plugin_dep_tests` submodule) because `INIT_BUNDLE_ID`
//!            is `pub(crate)` and cannot be accessed from an external crate.
//!

use polyplug::abi::PluginDescriptor;
use polyplug::abi::PluginVTable;
use polyplug::abi::StringView;

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Allocate a `'static` `PluginVTable` with the given contract_id.
///
/// Intentional leak — test vtables are pointer-sized and tests are short-lived.
/// The vtable must be `'static` because `Registry::register` stores a raw pointer
/// that must remain valid for the registry's lifetime.
fn make_static_vtable(cid: u64) -> &'static PluginVTable {
    Box::leak(Box::new(PluginVTable {
        contract_id: cid,
        contract_version: 0,
        function_count: 0,
        functions: core::ptr::null(),
    }))
}

fn make_desc(plugin_name: &'static str, contract_name: &'static str) -> PluginDescriptor {
    PluginDescriptor {
        name: StringView::from_static(plugin_name.as_bytes()),
        contract_name: StringView::from_static(contract_name.as_bytes()),
        version_major: 1,
        version_minor: 0,
        version_patch: 0,
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::make_desc;
    use super::make_static_vtable;
    use polyplug::abi::PluginDescriptor;
    use polyplug::abi::PluginHandle;
    use polyplug::abi::PluginVTable;
    use polyplug::abi::bundle_id;
    use polyplug::abi::contract_id;
    use polyplug::error::RegistryError;
    use polyplug::registry::Registry;

    // ── Test a ───────────────────────────────────────────────────────────────

    /// Single plugin registered for a contract — find_by_contract returns a valid handle.
    #[test]
    fn find_by_contract_single_plugin() {
        let registry: Registry = Registry::new();
        let cid: u64 = contract_id("audio.Decoder", 0);
        let bid: u64 = bundle_id("audio-engine");
        let vtable: &'static PluginVTable = make_static_vtable(cid);
        let desc: PluginDescriptor = make_desc("decoder", "audio.Decoder");
        // SAFETY: vtable is 'static and valid for the duration of this test.
        unsafe { registry.register(desc, vtable, "audio.Decoder".to_owned(), bid) }
            .expect("register should succeed");

        let handle: PluginHandle = registry
            .find_by_contract(cid, 0)
            .expect("find_by_contract should return Ok");

        assert!(!handle.is_null(), "returned handle must not be null");
    }

    // ── Test b ───────────────────────────────────────────────────────────────

    /// Two plugins from different bundles implement the same contract —
    /// find_all_by_contract returns both.
    #[test]
    fn find_all_returns_two_impls() {
        let registry: Registry = Registry::new();
        let cid: u64 = contract_id("audio.Decoder", 0);
        let vtable_a: &'static PluginVTable = make_static_vtable(cid);
        let vtable_b: &'static PluginVTable = make_static_vtable(cid);

        // SAFETY: vtables are 'static.
        unsafe {
            registry
                .register(
                    make_desc("decoder-a", "audio.Decoder"),
                    vtable_a,
                    "audio.Decoder".to_owned(),
                    bundle_id("bundle-a"),
                )
                .expect("register bundle-a")
        };
        // SAFETY: vtable_b is 'static.
        unsafe {
            registry
                .register(
                    make_desc("decoder-b", "audio.Decoder"),
                    vtable_b,
                    "audio.Decoder".to_owned(),
                    bundle_id("bundle-b"),
                )
                .expect("register bundle-b")
        };

        let mut handles: [PluginHandle; 4] = [PluginHandle {
            index: 0u32,
            generation: 0u32,
        }; 4];
        let count: usize = registry.find_all_by_contract(cid, 0, &mut handles);
        assert_eq!(count, 2, "must find exactly 2 providers");
    }

    // ── Test c ───────────────────────────────────────────────────────────────

    /// find_by_bundle returns the handle for the specific requested bundle,
    /// not the first-registered one.
    #[test]
    fn find_by_bundle_specificity() {
        let registry: Registry = Registry::new();
        let cid: u64 = contract_id("audio.Decoder", 0);
        let bid_a: u64 = bundle_id("bundle-a");
        let bid_b: u64 = bundle_id("bundle-b");
        let vtable_a: &'static PluginVTable = make_static_vtable(cid);
        let vtable_b: &'static PluginVTable = make_static_vtable(cid);

        // SAFETY: vtables are 'static.
        unsafe {
            registry
                .register(
                    make_desc("decoder-a", "audio.Decoder"),
                    vtable_a,
                    "audio.Decoder".to_owned(),
                    bid_a,
                )
                .expect("register bundle-a")
        };
        // SAFETY: vtable_b is 'static.
        unsafe {
            registry
                .register(
                    make_desc("decoder-b", "audio.Decoder"),
                    vtable_b,
                    "audio.Decoder".to_owned(),
                    bid_b,
                )
                .expect("register bundle-b")
        };

        let found: PluginHandle = registry
            .find_by_bundle(bid_b, cid, 0)
            .expect("find_by_bundle(bundle-b) should succeed");

        // Resolve and verify the vtable pointer belongs to bundle-b.
        let guard = registry
            .resolve_guard(found)
            .expect("resolve_guard must succeed for a freshly registered handle");
        let resolved_ptr: *const PluginVTable = guard.vtable();

        assert_eq!(
            resolved_ptr, vtable_b as *const PluginVTable,
            "resolved vtable must be bundle-b's vtable, not bundle-a's"
        );
    }

    // ── Test d ───────────────────────────────────────────────────────────────

    /// A handle with a wrong generation is rejected by resolve_guard with StaleHandle.
    #[test]
    fn stale_handle_rejected() {
        let registry: Registry = Registry::new();
        let cid: u64 = contract_id("audio.Decoder", 0);
        let vtable: &'static PluginVTable = make_static_vtable(cid);

        // SAFETY: vtable is 'static.
        unsafe {
            registry
                .register(
                    make_desc("decoder", "audio.Decoder"),
                    vtable,
                    "audio.Decoder".to_owned(),
                    bundle_id("audio-engine"),
                )
                .expect("register should succeed")
        };

        // Construct a handle pointing at slot 0 but with a wrong generation.
        let stale: PluginHandle = PluginHandle {
            index: 0,
            generation: 99,
        };

        let result = registry.resolve_guard(stale);
        assert!(
            matches!(result, Err(RegistryError::StaleHandle { .. })),
            "stale handle must return Err(StaleHandle)"
        );
    }
}
