#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

//! Integration test: a bundle in one language depends on a provider bundle in a
//! DIFFERENT language.
//!
//! The dependency machinery is id-based: `RuntimeStore` tracks bundles and
//! contracts by `BundleId`/`GuestContractId` and never inspects the bundle's
//! language. This test proves that cross-language dependency resolution and
//! load-order enforcement work by pairing a Rust-runtime provider with a
//! Lua-runtime depender (distinct `loader_name`s, distinct loaders).
//!
//! Two cases are covered:
//!   (a) Provider (rust) loaded first, then depender (lua): the depender resolves
//!       the cross-language provider's contract during its init window.
//!   (b) Provider absent: the depender's declared dependency does not resolve
//!       during init (null handle), and a host lookup afterwards fails — the
//!       missing-provider case is reported, not silently satisfied.

use core::ffi::c_void;
use core::ptr;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

use polyplug::error::LoaderError;
use polyplug::loader::BundleLoader;
use polyplug::loader::BundleSource;
use polyplug::runtime::Runtime;

use polyplug_abi::dispatch::VmLoaderData;
use polyplug_abi::{
    AbiError, DispatchMechanisms, DispatchType, GuestContractHandle, GuestContractInstance,
    GuestContractInterface, HostApi, NativeDispatch, PluginDescriptor, StringView, Version,
};
use polyplug_common::ManifestData;
use polyplug_utils::bundle_id;
use polyplug_utils::guest_contract_id;
use polyplug_utils::{BundleId, GuestContractId};
use std::fs;
use tempfile::TempDir;

const MOCK_FNS_EMPTY: [*const (); 0] = [];

unsafe extern "C" fn noop_create_instance(
    _adapter_context: *mut c_void,
    _loader_data: VmLoaderData,
    _host: *const HostApi,
    _args: *const (),
    out_instance: *mut GuestContractInstance,
) {
    if !out_instance.is_null() {
        // SAFETY: out_instance is non-null (just checked) and writable per the ABI contract.
        unsafe { out_instance.write(GuestContractInstance::null()) };
    }
}

unsafe extern "C" fn noop_destroy_instance(
    _adapter_context: *mut c_void,
    _loader_data: VmLoaderData,
    _host: *const HostApi,
    _instance: GuestContractInstance,
) {
}

/// Provider loader labelled as the "rust" runtime. Registers `contract_id`.
struct RustProviderLoader {
    contract_id: u64,
}

impl BundleLoader for RustProviderLoader {
    fn loader_name(&self) -> &'static str {
        "rust-provider"
    }

    fn supports_hot_reload(&self) -> bool {
        false
    }

    fn load(
        &self,
        manifest: &ManifestData,
        _source: &BundleSource,
        runtime: &Runtime,
    ) -> Result<(), LoaderError> {
        let interface: &'static GuestContractInterface =
            Box::leak(Box::new(GuestContractInterface {
                contract_id: GuestContractId::from_u64(self.contract_id),
                contract_version: Version {
                    major: 1,
                    minor: 0,
                    patch: 0,
                },
                dispatch_type: DispatchType::Native,
                adapter_context: ptr::null_mut(),
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
            name: StringView::from_static(b"rust_provider"),
            contract_name: StringView::from_static(b"provider.contract"),
            version: Version {
                major: 1,
                minor: 0,
                patch: 0,
            },
        };
        let bundle_id: BundleId = BundleId::new(&manifest.name);
        runtime.push_init_bundle_id(bundle_id.id());
        let host: *const HostApi = runtime.host_abi();
        let mut registration: AbiError = AbiError::ok();
        // SAFETY: the runtime owns `host`; the descriptor and leaked interface remain
        // valid for this synchronous registration callback.
        unsafe {
            ((*host).register_guest_contract)(host, &descriptor, interface, &mut registration);
        }
        runtime.pop_init_bundle_id();
        if registration.is_ok() {
            Ok(())
        } else {
            Err(LoaderError::InitFailed {
                bundle: manifest.name.clone(),
                error: format!(
                    "guest registration failed with ABI code {}",
                    registration.code
                ),
            })
        }
    }

    fn reload(&self, _manifest: &ManifestData, _runtime: &Runtime) -> Result<(), LoaderError> {
        Err(LoaderError::HotReloadUnsupported {
            loader_name: self.loader_name().to_owned(),
        })
    }
}

/// Depender loader labelled as the "lua" runtime. During its init window it
/// probes the host for the cross-language provider's declared contract and
/// records whether it resolved.
struct LuaDependerLoader {
    provider_contract_id: u64,
    resolved_non_null: Arc<Mutex<Option<bool>>>,
}

impl BundleLoader for LuaDependerLoader {
    fn loader_name(&self) -> &'static str {
        "lua-depender"
    }

    fn supports_hot_reload(&self) -> bool {
        false
    }

    fn load(
        &self,
        manifest: &ManifestData,
        _source: &BundleSource,
        runtime: &Runtime,
    ) -> Result<(), LoaderError> {
        let host: *const HostApi = runtime.host_abi();
        let bundle_id: BundleId = BundleId::new(&manifest.name);

        // Enter the enforcement window, exactly as a real loader does.
        runtime.push_init_bundle_id(bundle_id.id());

        // SAFETY: host is a valid HostApi pointer from the runtime.
        let handle: GuestContractHandle =
            unsafe { ((*host).find_guest_contract)(host, self.provider_contract_id, 0_u32) };

        runtime.pop_init_bundle_id();

        *self
            .resolved_non_null
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(!handle.is_null());
        Ok(())
    }

    fn reload(&self, _manifest: &ManifestData, _runtime: &Runtime) -> Result<(), LoaderError> {
        Err(LoaderError::HotReloadUnsupported {
            loader_name: self.loader_name().to_owned(),
        })
    }
}

/// Write a provider bundle for the `rust-provider` runtime.
fn write_provider(temp: &TempDir, bundle_name: &str) -> PathBuf {
    let bundle_dir: PathBuf = temp.path().join(bundle_name);
    fs::create_dir_all(&bundle_dir).expect("create bundle dir");
    fs::write(bundle_dir.join("dummy.so"), b"").expect("write dummy so");
    let bundle_id_val: u64 = bundle_id(bundle_name);
    let manifest: String = format!(
        "id = {bundle_id_val}\n\
         name = \"{bundle_name}\"\n\
         loader = \"rust-provider\"\n\
         file = \"dummy.so\"\n\
         version = \"1.0\"\n\
         provides = [\"provider.contract@1\"]\n\
         function_count = {{ \"provider.contract@1\" = 0 }}\n"
    );
    fs::write(bundle_dir.join("manifest.toml"), manifest).expect("write manifest");
    bundle_dir
}

/// Write a depender bundle for the `lua-depender` runtime that declares a
/// `[[dependency]]` on the provider's contract.
fn write_depender(temp: &TempDir, bundle_name: &str, provider_contract_id: u64) -> PathBuf {
    let bundle_dir: PathBuf = temp.path().join(bundle_name);
    fs::create_dir_all(&bundle_dir).expect("create bundle dir");
    fs::write(bundle_dir.join("dummy.lua"), b"").expect("write dummy lua");
    let bundle_id_val: u64 = bundle_id(bundle_name);
    let manifest: String = format!(
        "id = {bundle_id_val}\n\
         name = \"{bundle_name}\"\n\
         loader = \"lua-depender\"\n\
         file = \"dummy.lua\"\n\
         version = \"1.0\"\n\n\
         [[dependency]]\n\
         kind = \"contract\"\n\
         contract = \"provider.contract@1\"\n\
         min_version = \"1.0\"\n\
         contract_id = {provider_contract_id}\n"
    );
    fs::write(bundle_dir.join("manifest.toml"), manifest).expect("write manifest");
    bundle_dir
}

#[test]
fn lua_depender_resolves_rust_provider_across_languages() {
    let temp: TempDir = TempDir::new().expect("temp dir");
    let provider_contract_id: u64 = guest_contract_id("provider.contract", 1_u32);
    let resolved: Arc<Mutex<Option<bool>>> = Arc::new(Mutex::new(None));

    let runtime: Arc<Runtime> = Runtime::builder()
        .loader(RustProviderLoader {
            contract_id: provider_contract_id,
        })
        .loader(LuaDependerLoader {
            provider_contract_id,
            resolved_non_null: Arc::clone(&resolved),
        })
        .build()
        .expect("runtime build should succeed");

    let provider_path: PathBuf = write_provider(&temp, "rust_provider_bundle");
    let depender_path: PathBuf = write_depender(&temp, "lua_depender_bundle", provider_contract_id);

    // Load the provider (rust) first, then the depender (lua).
    runtime
        .load_bundle(provider_path.as_path())
        .expect("load provider");
    runtime
        .load_bundle(depender_path.as_path())
        .expect("load depender");

    assert_eq!(
        *resolved.lock().unwrap_or_else(|e| e.into_inner()),
        Some(true),
        "lua depender must resolve the rust provider's contract across languages during init"
    );
}

#[test]
fn lua_depender_missing_rust_provider_does_not_resolve() {
    let temp: TempDir = TempDir::new().expect("temp dir");
    let provider_contract_id: u64 = guest_contract_id("provider.contract", 1_u32);
    let resolved: Arc<Mutex<Option<bool>>> = Arc::new(Mutex::new(None));

    // Note: the provider loader is NOT registered and the provider bundle is NOT
    // loaded — the cross-language dependency is unsatisfiable.
    let runtime: Arc<Runtime> = Runtime::builder()
        .loader(LuaDependerLoader {
            provider_contract_id,
            resolved_non_null: Arc::clone(&resolved),
        })
        .build()
        .expect("runtime build should succeed");

    let depender_path: PathBuf = write_depender(&temp, "lua_depender_bundle", provider_contract_id);
    runtime
        .load_bundle(depender_path.as_path())
        .expect("load depender");

    assert_eq!(
        *resolved.lock().unwrap_or_else(|e| e.into_inner()),
        Some(false),
        "with no provider loaded, the declared cross-language dependency must not resolve"
    );

    // A host lookup after init must also fail: the contract was never provided.
    assert!(
        runtime
            .find_guest_contract(provider_contract_id, 0)
            .is_err(),
        "missing cross-language provider must surface as a not-found lookup"
    );
}
