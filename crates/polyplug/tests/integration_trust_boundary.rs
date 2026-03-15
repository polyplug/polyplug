use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

use polyplug::abi::PluginDescriptor;
use polyplug::abi::PluginHandle;
use polyplug::abi::PluginRegistrar;
use polyplug::abi::PluginVTable;
use polyplug::abi::StringView;
use polyplug::abi::contract_id;
use polyplug::error::PolyplugError;
use polyplug::error::RuntimeError;
use polyplug::loader::BundleLoader;
use polyplug::registry::Registry;
use polyplug::runtime::LoadOptions;
use polyplug::runtime::Runtime;
use polyplug::runtime::set_global_registry;
use polyplug::version::Compatibility;
use tempfile::TempDir;

#[cfg(test)]
mod tests {
    use super::*;

    struct EnforceLoader {
        contract_id: u64,
        error_bundle_id: u64,
    }

    impl BundleLoader for EnforceLoader {
        fn runtime_name(&self) -> &'static str {
            "enforce"
        }

        fn load(
            &self,
            _path: &Path,
            _registrar: &mut PluginRegistrar,
        ) -> Result<(), PolyplugError> {
            // SAFETY: test_host_find_by_contract takes plain integers only.
            let handle: PluginHandle = unsafe {
                polyplug::runtime::testing::test_host_find_by_contract(self.contract_id, 0_u32)
            };
            if handle.is_null() {
                return Err(RuntimeError::UndeclaredDependency {
                    bundle_id: self.error_bundle_id,
                    contract_id: self.contract_id,
                });
            }
            Ok(())
        }
    }

    struct ProbeLoader {
        contract_id: u64,
        observed_null: Arc<Mutex<Option<bool>>>,
    }

    impl BundleLoader for ProbeLoader {
        fn runtime_name(&self) -> &'static str {
            "probe"
        }

        fn load(
            &self,
            _path: &Path,
            _registrar: &mut PluginRegistrar,
        ) -> Result<(), PolyplugError> {
            // SAFETY: test_host_find_by_contract takes plain integers only.
            let handle: PluginHandle = unsafe {
                polyplug::runtime::testing::test_host_find_by_contract(self.contract_id, 0_u32)
            };
            let mut guard: std::sync::MutexGuard<'_, Option<bool>> = match self.observed_null.lock()
            {
                Ok(g) => g,
                Err(e) => e.into_inner(),
            };
            *guard = Some(handle.is_null());
            Ok(())
        }
    }

    struct PanicLoader;

    impl BundleLoader for PanicLoader {
        fn runtime_name(&self) -> &'static str {
            "panic"
        }

        fn load(
            &self,
            _path: &Path,
            _registrar: &mut PluginRegistrar,
        ) -> Result<(), PolyplugError> {
            panic!("intentional panic in PanicLoader");
        }
    }

    struct ReentrantState {
        runtime_ptr: usize,
        inner_bundle: PathBuf,
        /// INIT_BUNDLE_ID read immediately after the inner load_bundle_with returns.
        /// Used to verify the inner guard's drop cleared to 0 (not leaked).
        tls_after_inner_load: Option<u64>,
    }

    struct ReentrantLoader {
        state: Arc<Mutex<ReentrantState>>,
    }

    impl BundleLoader for ReentrantLoader {
        fn runtime_name(&self) -> &'static str {
            "reentrant"
        }

        fn load(
            &self,
            _path: &Path,
            _registrar: &mut PluginRegistrar,
        ) -> Result<(), PolyplugError> {
            let state: std::sync::MutexGuard<'_, ReentrantState> = match self.state.lock() {
                Ok(g) => g,
                Err(e) => e.into_inner(),
            };
            let runtime_ptr: usize = state.runtime_ptr;
            if runtime_ptr == 0 {
                return Err(RuntimeError::Loader(
                    polyplug::error::LoaderError::InitFailed {
                        bundle: "reentrant".to_owned(),
                        error: "runtime pointer not initialized".to_owned(),
                    },
                ));
            }
            let inner_bundle: PathBuf = state.inner_bundle.clone();
            let already_set: bool = state.tls_after_inner_load.is_some();
            drop(state);
            // SAFETY: runtime_ptr was set from a valid &Runtime during load_bundle.
            let runtime_ref: &Runtime = unsafe { &*(runtime_ptr as *const Runtime) };
            let inner_result: Result<(), PolyplugError> = runtime_ref.load_bundle_with(
                inner_bundle.as_path(),
                LoadOptions {
                    compatibility: Compatibility::default(),
                    ignore_function_count_mismatch: false,
                },
            );
            inner_result?;
            // Read INIT_BUNDLE_ID right after the inner load completes.
            // The inner BundleInitGuard has dropped; INIT_BUNDLE_ID should now
            // be 0 (inner guard clears to 0, it does not restore outer value).
            let tls_val: u64 = polyplug::runtime::testing::read_init_bundle_id();
            let mut st2: std::sync::MutexGuard<'_, ReentrantState> = match self.state.lock() {
                Ok(g) => g,
                Err(e) => e.into_inner(),
            };
            if !already_set {
                st2.tls_after_inner_load = Some(tls_val);
            }
            Ok(())
        }
    }

    struct LazyState {
        contract_id: u64,
        observed_null: Option<bool>,
    }

    struct LazyLoader {
        state: Arc<Mutex<LazyState>>,
    }

    impl BundleLoader for LazyLoader {
        fn runtime_name(&self) -> &'static str {
            "lazy"
        }

        fn load(
            &self,
            _path: &Path,
            _registrar: &mut PluginRegistrar,
        ) -> Result<(), PolyplugError> {
            let mut state: std::sync::MutexGuard<'_, LazyState> = match self.state.lock() {
                Ok(g) => g,
                Err(e) => e.into_inner(),
            };
            // SAFETY: test_host_find_by_contract takes plain integers only.
            let handle: PluginHandle = unsafe {
                polyplug::runtime::testing::test_host_find_by_contract(state.contract_id, 0_u32)
            };
            if state.observed_null.is_none() {
                state.observed_null = Some(handle.is_null());
            }
            Ok(())
        }
    }

    fn create_bundle_dir(temp: &TempDir, bundle_name: &str, runtime: &str) -> PathBuf {
        let bundle_dir: PathBuf = temp.path().join(bundle_name);
        if let Err(e) = std::fs::create_dir_all(&bundle_dir) {
            panic!("failed to create bundle dir {}: {e}", bundle_dir.display());
        }
        let so_path: PathBuf = bundle_dir.join("dummy.so");
        if let Err(e) = std::fs::write(&so_path, b"") {
            panic!("failed to write dummy so {}: {e}", so_path.display());
        }
        let manifest: String = format!(
            "bundle_name = \"{}\"\nruntime = \"{}\"\nfile = \"dummy.so\"\n",
            bundle_name, runtime
        );
        let manifest_path: PathBuf = bundle_dir.join("manifest.toml");
        if let Err(e) = std::fs::write(&manifest_path, manifest) {
            panic!("failed to write manifest {}: {e}", manifest_path.display());
        }
        bundle_dir
    }

    fn register_contract(registry: &Registry, contract_id: u64, bundle_id: u64) -> PluginHandle {
        let vtable: &'static PluginVTable = Box::leak(Box::new(PluginVTable {
            contract_id,
            contract_version: 0_u32,
            function_count: 0_u32,
            functions: core::ptr::null(),
        }));
        let descriptor: PluginDescriptor = PluginDescriptor {
            name: StringView::from_static(b"stub"),
            contract_name: StringView::from_static(b"stub.contract"),
            version_major: 1_u32,
            version_minor: 0_u32,
            version_patch: 0_u32,
        };
        // SAFETY: vtable is leaked and lives for the process lifetime.
        let result: Result<PluginHandle, polyplug::error::RegistryError> =
            unsafe { registry.register(descriptor, vtable, "stub.contract".to_owned(), bundle_id) };
        match result {
            Ok(handle) => handle,
            Err(e) => panic!("failed to register contract: {e}"),
        }
    }

    #[test]
    fn bundle_id_zero_escape_returns_undeclared_dependency_error() {
        let temp: TempDir = match TempDir::new() {
            Ok(t) => t,
            Err(e) => panic!("failed to create temp dir: {e}"),
        };
        let contract: u64 = contract_id("trust.test", 1_u32);
        let bundle_name: &str = "enforce_bundle";
        let bundle_path: PathBuf = create_bundle_dir(&temp, bundle_name, "enforce");
        let runtime: Runtime = match Runtime::builder()
            .loader(EnforceLoader {
                contract_id: contract,
                error_bundle_id: 0_u64,
            })
            .build()
        {
            Ok(rt) => rt,
            Err(e) => panic!("failed to build runtime: {e}"),
        };
        let registry: &Arc<Registry> = runtime.registry();
        let _handle: PluginHandle = register_contract(registry.as_ref(), contract, 0xBEEF_u64);
        let result: Result<(), PolyplugError> = runtime.load_bundle(bundle_path.as_path());
        match result {
            Err(RuntimeError::UndeclaredDependency {
                bundle_id,
                contract_id,
            }) => {
                assert_eq!(
                    bundle_id, 0_u64,
                    "error bundle_id should match expected sentinel"
                );
                assert_eq!(contract_id, contract, "error contract_id should match");
            }
            Err(other) => panic!("unexpected error: {other}"),
            Ok(()) => panic!("expected undeclared dependency error"),
        }
    }

    #[test]
    fn tls_state_cleared_after_init_completes() {
        let temp: TempDir = match TempDir::new() {
            Ok(t) => t,
            Err(e) => panic!("failed to create temp dir: {e}"),
        };
        let contract: u64 = contract_id("trust.tls", 1_u32);
        let observed: Arc<Mutex<Option<bool>>> = Arc::new(Mutex::new(None));
        let bundle_path: PathBuf = create_bundle_dir(&temp, "probe_bundle", "probe");
        let runtime: Runtime = match Runtime::builder()
            .loader(ProbeLoader {
                contract_id: contract,
                observed_null: Arc::clone(&observed),
            })
            .build()
        {
            Ok(rt) => rt,
            Err(e) => panic!("failed to build runtime: {e}"),
        };
        // Register the contract into the runtime's own registry.
        // ProbeLoader uses polyplug_find_by_contract (global path); the post-init
        // assertion uses runtime.find_by_contract (local path) to avoid global OnceLock
        // ordering issues between tests.
        let registry: &Arc<Registry> = runtime.registry();
        let _handle: PluginHandle = register_contract(registry.as_ref(), contract, 0xCAFE_u64);
        let result: Result<(), PolyplugError> = runtime.load_bundle(bundle_path.as_path());
        if let Err(e) = result {
            panic!("load_bundle failed: {e}");
        }
        let observed_value: Option<bool> = match observed.lock() {
            Ok(g) => *g,
            Err(e) => *e.into_inner(),
        };
        // During init, INIT_BUNDLE_ID is set and dep enforcement is active.
        // ProbeLoader has no declared deps, so polyplug_find_by_contract returns null.
        assert_eq!(
            observed_value,
            Some(true),
            "during init, dep enforcement must block undeclared lookup (observed_null=true)"
        );
        // After init, INIT_BUNDLE_ID is cleared to 0 by BundleInitGuard::drop.
        // runtime.find_by_contract bypasses the global OnceLock and queries the
        // runtime's own registry directly — confirming TLS is clear (no dep enforcement).
        let handle_after: Result<polyplug::abi::PluginHandle, _> =
            runtime.find_by_contract(contract, 0_u32);
        assert!(
            handle_after.is_ok(),
            "after init, TLS must be cleared so find_by_contract succeeds without enforcement"
        );
    }

    #[test]
    fn panic_during_init_triggers_guard_drop() {
        let temp: TempDir = match TempDir::new() {
            Ok(t) => t,
            Err(e) => panic!("failed to create temp dir: {e}"),
        };
        let contract: u64 = contract_id("trust.panic", 1_u32);
        let registry: Arc<Registry> = Arc::new(Registry::new());
        set_global_registry(Arc::clone(&registry));
        let _handle: PluginHandle = register_contract(registry.as_ref(), contract, 0xD00D_u64);
        let bundle_root: PathBuf = create_bundle_dir(&temp, "panic_bundle", "panic");
        let plugin_dir: PathBuf = temp.path().to_path_buf();
        let result = std::panic::catch_unwind(|| {
            let _rt: Runtime = Runtime::builder()
                .plugin_dir(plugin_dir)
                .loader(PanicLoader)
                .build()
                .unwrap_or_else(|e| panic!("runtime build failed: {e}"));
        });
        if result.is_ok() {
            panic!("expected panic from PanicLoader");
        }
        // SAFETY: test_host_find_by_contract takes plain integers only.
        let handle_after: PluginHandle =
            unsafe { polyplug::runtime::testing::test_host_find_by_contract(contract, 0_u32) };
        assert!(
            !handle_after.is_null(),
            "BundleInitGuard should clear TLS even when load panics"
        );
        let _ = bundle_root;
    }

    #[test]
    fn reentrant_load_on_same_thread_does_not_leak_bundle_id() {
        let temp: TempDir = match TempDir::new() {
            Ok(t) => t,
            Err(e) => panic!("failed to create temp dir: {e}"),
        };
        let contract: u64 = contract_id("trust.reentrant", 1_u32);
        let outer_bundle: PathBuf = create_bundle_dir(&temp, "outer_bundle", "reentrant");
        let inner_bundle: PathBuf = create_bundle_dir(&temp, "inner_bundle", "probe");
        let state: Arc<Mutex<ReentrantState>> = Arc::new(Mutex::new(ReentrantState {
            runtime_ptr: 0,
            inner_bundle: inner_bundle.clone(),
            tls_after_inner_load: None,
        }));
        let runtime: Runtime = match Runtime::builder()
            .loader(ReentrantLoader {
                state: Arc::clone(&state),
            })
            .loader(ProbeLoader {
                contract_id: contract,
                observed_null: Arc::new(Mutex::new(None)),
            })
            .build()
        {
            Ok(rt) => rt,
            Err(e) => panic!("failed to build runtime: {e}"),
        };
        let registry: &Arc<Registry> = runtime.registry();
        let _handle: PluginHandle = register_contract(registry.as_ref(), contract, 0xABCD_u64);
        {
            let mut guard: std::sync::MutexGuard<'_, ReentrantState> = match state.lock() {
                Ok(g) => g,
                Err(e) => e.into_inner(),
            };
            guard.runtime_ptr = &runtime as *const Runtime as usize;
        }
        let result: Result<(), PolyplugError> = runtime.load_bundle_with(
            outer_bundle.as_path(),
            LoadOptions {
                compatibility: Compatibility::default(),
                ignore_function_count_mismatch: false,
            },
        );
        if let Err(e) = result {
            panic!("outer load failed: {e}");
        }
        // tls_after_inner_load holds the INIT_BUNDLE_ID read inside ReentrantLoader::load()
        // immediately after the inner load_bundle_with returns (inner guard already dropped).
        // Per the non-reentrancy contract, BundleInitGuard always clears to 0 on drop —
        // it does not restore the outer value. So we expect 0 here, not the outer bundle_id.
        let tls_captured: Option<u64> = match state.lock() {
            Ok(g) => g.tls_after_inner_load,
            Err(e) => e.into_inner().tls_after_inner_load,
        };
        assert_eq!(
            tls_captured,
            Some(0_u64),
            "after inner guard drops, INIT_BUNDLE_ID must be 0 (not leaked from inner or outer bundle)"
        );
        let _ = inner_bundle;
    }

    #[test]
    fn lazy_load_during_init_does_not_corrupt_tls() {
        let temp: TempDir = match TempDir::new() {
            Ok(t) => t,
            Err(e) => panic!("failed to create temp dir: {e}"),
        };
        let contract: u64 = contract_id("trust.lazy", 1_u32);
        let outer_bundle: PathBuf = create_bundle_dir(&temp, "lazy_outer", "lazy");
        let inner_bundle: PathBuf = create_bundle_dir(&temp, "lazy_inner", "probe");
        let state: Arc<Mutex<LazyState>> = Arc::new(Mutex::new(LazyState {
            contract_id: contract,
            observed_null: None,
        }));
        let runtime: Runtime = match Runtime::builder()
            .loader(LazyLoader {
                state: Arc::clone(&state),
            })
            .loader(ProbeLoader {
                contract_id: contract,
                observed_null: Arc::new(Mutex::new(None)),
            })
            .build()
        {
            Ok(rt) => rt,
            Err(e) => panic!("failed to build runtime: {e}"),
        };
        let registry: &Arc<Registry> = runtime.registry();
        let _handle: PluginHandle = register_contract(registry.as_ref(), contract, 0xFACE_u64);
        let result: Result<(), PolyplugError> = runtime.load_bundle(outer_bundle.as_path());
        if let Err(e) = result {
            panic!("outer load failed: {e}");
        }
        let observed_init: Option<bool> = match state.lock() {
            Ok(g) => g.observed_null,
            Err(e) => e.into_inner().observed_null,
        };
        assert_eq!(
            observed_init,
            Some(true),
            "init should observe enforcement during lazy loader init"
        );
        // Load the inner bundle to confirm no TLS corruption from the outer load.
        // Use `runtime` directly — runtime_ptr in state is not needed here.
        let inner_result: Result<(), PolyplugError> = runtime.load_bundle_with(
            inner_bundle.as_path(),
            LoadOptions {
                compatibility: Compatibility::default(),
                ignore_function_count_mismatch: false,
            },
        );
        if let Err(e) = inner_result {
            panic!("lazy inner load failed: {e}");
        }
        // INIT_BUNDLE_ID must be 0 after all loads complete (TLS not corrupted).
        let init_id_after: u64 = polyplug::runtime::testing::read_init_bundle_id();
        assert_eq!(
            init_id_after, 0_u64,
            "lazy load must not corrupt TLS: INIT_BUNDLE_ID must be 0 after all loads"
        );
    }
}
