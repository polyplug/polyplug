#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

//! Integration test: end-to-end extension round-trip.
//!
//! The extension system (ROADMAP Goal 2) is a generic mechanism: the host
//! registers an opaque pointer by name via `Runtime::register_extension`, and a
//! plugin recovers it during `polyplug_init` via `host->get_extension(id)`, casts
//! it to the agreed `#[repr(C)]` struct, and calls through it. The lifetime
//! contract is: extension pointers are registered once at startup and valid for
//! the runtime's entire lifetime.
//!
//! This test drives the real path with an in-process loader simulating a plugin:
//!   1. The host registers a `TestExtension` (a fn pointer + data) by name.
//!   2. The loader's `load` (the simulated `polyplug_init`) calls
//!      `host->get_extension(fnv1a_32("test.ext"))`, casts the result, and calls
//!      the function pointer through it.
//!   3. The plugin registers a contract whose `contract_id` is the value returned
//!      by the extension call — so the contract being findable under that exact id
//!      proves the whole round-trip executed.
//!
//! It also asserts that `get_extension` with an unknown id returns null.

use core::sync::atomic::AtomicU64;
use core::sync::atomic::Ordering;
use std::path::PathBuf;
use std::sync::Arc;

use polyplug::error::RuntimeError;
use polyplug::loader::{BundleLoader, ManifestData};
use polyplug::runtime::Runtime;
use polyplug_abi::{
    DispatchMechanisms, DispatchType, GuestContractInstance, GuestContractInterface, HostApi,
    NativeDispatch, PluginDescriptor, StringView, Version,
};
use polyplug_utils::{BundleId, GuestContractId, fnv1a_32};

const MOCK_FNS_EMPTY: [*const (); 0] = [];

unsafe extern "C" fn noop_create_instance(
    _host: *const HostApi,
    _args: *const (),
) -> GuestContractInstance {
    GuestContractInstance::null()
}

unsafe extern "C" fn noop_destroy_instance(
    _host: *const HostApi,
    _instance: GuestContractInstance,
) {
}

/// The shared `#[repr(C)]` extension contract between host and plugin.
///
/// `compute` derives a contract id from a seed the host configured. The plugin
/// calls it during init and registers the resulting contract id.
#[repr(C)]
struct TestExtension {
    seed: u64,
    compute: unsafe extern "C" fn(this: *const TestExtension, salt: u64) -> u64,
}

/// Host-side extension implementation: `compute` returns `seed ^ salt`.
unsafe extern "C" fn test_extension_compute(this: *const TestExtension, salt: u64) -> u64 {
    // SAFETY: `this` is the TestExtension pointer the host registered; it is valid
    // for the runtime lifetime per the extension lifetime contract.
    let seed: u64 = unsafe { (*this).seed };
    seed ^ salt
}

const EXTENSION_NAME: &str = "test.ext";
const EXTENSION_SEED: u64 = 0x0F0F_0F0F_0F0F_0F0F;
const EXTENSION_SALT: u64 = 0x1234_5678_9ABC_DEF0;

/// In-process loader simulating a plugin that consumes the host extension during
/// `polyplug_init`.
struct ExtensionPluginLoader {
    /// Records the contract id the plugin derived from the extension call, or 0 if
    /// the extension was not found.
    derived_contract_id: Arc<AtomicU64>,
    /// Records whether `get_extension` for an unknown id returned null.
    unknown_returned_null: Arc<AtomicU64>,
}

impl BundleLoader for ExtensionPluginLoader {
    fn runtime_name(&self) -> &'static str {
        "ext-plugin"
    }

    fn load(&self, manifest: &ManifestData, runtime: &Runtime) -> Result<(), RuntimeError> {
        let host: &'static HostApi = runtime.host_abi();

        // Recover the extension by id (fnv1a_32 of its name), exactly as a real
        // plugin would inside polyplug_init.
        let ext_id: u32 = fnv1a_32(EXTENSION_NAME.as_bytes());
        // SAFETY: host is a valid HostApi from the runtime.
        let ext_ptr: *const () = unsafe { (host.get_extension)(host as *const HostApi, ext_id) };
        assert!(!ext_ptr.is_null(), "registered extension must resolve");

        // An unknown id must resolve to null.
        // SAFETY: host is a valid HostApi from the runtime.
        let unknown_ptr: *const () =
            unsafe { (host.get_extension)(host as *const HostApi, fnv1a_32(b"no.such.extension")) };
        self.unknown_returned_null
            .store(u64::from(unknown_ptr.is_null()), Ordering::SeqCst);

        // Cast and call through the extension.
        let ext: *const TestExtension = ext_ptr as *const TestExtension;
        // SAFETY: ext_ptr is the TestExtension the host registered; the call uses
        // the agreed #[repr(C)] layout and the extension is valid for the runtime
        // lifetime.
        let derived: u64 = unsafe { ((*ext).compute)(ext, EXTENSION_SALT) };
        self.derived_contract_id.store(derived, Ordering::SeqCst);

        // Register a contract whose id is the derived value — being findable under
        // that exact id proves the round-trip.
        let interface: &'static GuestContractInterface =
            Box::leak(Box::new(GuestContractInterface {
                contract_id: GuestContractId::from_u64(derived),
                contract_version: Version {
                    major: 1,
                    minor: 0,
                    patch: 0,
                },
                dispatch_type: DispatchType::Native,
                create_instance: noop_create_instance,
                destroy_instance: noop_destroy_instance,
                dispatch: DispatchMechanisms {
                    native: NativeDispatch {
                        function_count: 0,
                        functions: MOCK_FNS_EMPTY.as_ptr(),
                    },
                },
            }));
        let descriptor: PluginDescriptor = PluginDescriptor {
            name: StringView::from_static(b"ext_plugin"),
            contract_name: StringView::from_static(b"ext.contract"),
            version: Version {
                major: 1,
                minor: 0,
                patch: 0,
            },
        };
        let bundle_id: BundleId = BundleId::new(&manifest.name);
        // SAFETY: interface is leaked and lives for the process lifetime.
        unsafe {
            runtime.registry().register_guest_contract(
                descriptor,
                interface,
                "ext.contract".to_owned(),
                bundle_id,
            )
        }
        .expect("contract registration should succeed");
        Ok(())
    }

    fn reload(&self, _manifest: &ManifestData, _runtime: &Runtime) -> Result<(), RuntimeError> {
        Err(RuntimeError::HotReloadDisabled)
    }
}

fn write_bundle(temp: &tempfile::TempDir, bundle_name: &str) -> PathBuf {
    let bundle_dir: PathBuf = temp.path().join(bundle_name);
    std::fs::create_dir_all(&bundle_dir).expect("create bundle dir");
    std::fs::write(bundle_dir.join("dummy.so"), b"").expect("write dummy so");
    let bundle_id: u64 = polyplug_utils::bundle_id(bundle_name);
    let manifest: String = format!(
        "id = {bundle_id}\n\
         name = \"{bundle_name}\"\n\
         runtime = \"ext-plugin\"\n\
         file = \"dummy.so\"\n\
         version = \"1.0\"\n"
    );
    std::fs::write(bundle_dir.join("manifest.toml"), manifest).expect("write manifest");
    bundle_dir
}

#[test]
fn extension_round_trip_through_get_extension() {
    let temp: tempfile::TempDir = tempfile::TempDir::new().expect("temp dir");

    let derived_contract_id: Arc<AtomicU64> = Arc::new(AtomicU64::new(0));
    let unknown_returned_null: Arc<AtomicU64> = Arc::new(AtomicU64::new(0));

    let runtime: Arc<Runtime> = Runtime::builder()
        .loader(ExtensionPluginLoader {
            derived_contract_id: Arc::clone(&derived_contract_id),
            unknown_returned_null: Arc::clone(&unknown_returned_null),
        })
        .build()
        .expect("runtime build should succeed");

    // The host registers the extension before loading any plugin. The pointer is
    // leaked so it stays valid for the runtime's entire lifetime.
    let extension: &'static TestExtension = Box::leak(Box::new(TestExtension {
        seed: EXTENSION_SEED,
        compute: test_extension_compute,
    }));
    // SAFETY: the extension pointer is leaked and valid for the runtime lifetime,
    // satisfying the extension lifetime contract.
    unsafe {
        runtime.register_extension(
            EXTENSION_NAME,
            extension as *const TestExtension as *const (),
        );
    }

    let bundle_path: PathBuf = write_bundle(&temp, "ext_bundle");
    runtime.load_bundle(bundle_path.as_path()).expect("load");

    // The plugin computed `seed ^ salt` through the extension and registered it.
    let expected: u64 = EXTENSION_SEED ^ EXTENSION_SALT;
    assert_eq!(
        derived_contract_id.load(Ordering::SeqCst),
        expected,
        "plugin must derive the contract id via the extension call"
    );
    assert_eq!(
        unknown_returned_null.load(Ordering::SeqCst),
        1,
        "get_extension with an unknown id must return null"
    );

    // The contract is findable under the extension-derived id — the round-trip ran.
    assert!(
        runtime.find_guest_contract(expected, 0).is_ok(),
        "contract registered with the extension-derived id must be findable"
    );
}
