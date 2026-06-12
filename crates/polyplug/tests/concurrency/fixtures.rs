#![allow(clippy::expect_used)]
// Shared fixtures for the concurrency suite. Each module uses only the subset it
// needs, so unused items are expected when viewed from any single module.
#![allow(dead_code)]

//! Fixtures shared across the concurrency suite modules: no-op instance
//! callbacks, a descriptor builder, the native-interface constructor macro, and
//! the hot-reload runtime/path helpers backed by the build-script-emitted
//! reload-plugin directories.

use std::path::PathBuf;
use std::sync::Arc;

use polyplug::runtime::Runtime;
use polyplug_abi::{
    GuestContractInstance, GuestContractInterface, HostApi, RuntimeConfig, StringView, Version,
};

use crate::common::TestNativeLoader;

// ─── Reload-plugin directories (emitted by build.rs) ──────────────────────────

pub(crate) const RELOAD_V1_DIR: &str = env!("RELOAD_PLUGIN_V1_DIR");
pub(crate) const RELOAD_V2_DIR: &str = env!("RELOAD_PLUGIN_V2_DIR");

// ─── Empty native function table shared by mock interfaces ────────────────────

pub(crate) const MOCK_FNS_EMPTY: [*const (); 0] = [];

// ─── No-op instance lifecycle callbacks ───────────────────────────────────────

/// No-op `create_instance` callback returning a null instance.
///
/// # Safety
/// Matches the ABI `create_instance` signature; reads neither argument.
pub(crate) unsafe extern "C" fn noop_create_instance(
    _host: *const HostApi,
    _args: *const (),
) -> GuestContractInstance {
    GuestContractInstance::null()
}

/// No-op `destroy_instance` callback.
///
/// # Safety
/// Matches the ABI `destroy_instance` signature; owns no instance data.
pub(crate) unsafe extern "C" fn noop_destroy_instance(
    _host: *const HostApi,
    _instance: GuestContractInstance,
) {
}

// ─── Descriptor + interface builders ──────────────────────────────────────────

pub(crate) fn make_descriptor(
    name: &'static str,
    contract_name: &'static str,
) -> polyplug_abi::PluginDescriptor {
    polyplug_abi::PluginDescriptor {
        name: StringView::from_static(name.as_bytes()),
        contract_name: StringView::from_static(contract_name.as_bytes()),
        version: Version {
            major: 1,
            minor: 0,
            patch: 0,
        },
    }
}

/// Build a native `GuestContractInterface` with the no-op lifecycle callbacks and
/// an empty function table — usable in both `static` and `const`/`Arc::new`
/// position.
macro_rules! make_interface {
    ($contract_id:expr, $version:expr) => {
        polyplug_abi::GuestContractInterface {
            contract_id: $contract_id,
            contract_version: $version,
            dispatch_type: polyplug_abi::DispatchType::Native,
            create_instance: crate::fixtures::noop_create_instance,
            destroy_instance: crate::fixtures::noop_destroy_instance,
            dispatch: polyplug_abi::DispatchMechanisms {
                native: polyplug_abi::NativeDispatch {
                    function_count: 0,
                    functions: crate::fixtures::MOCK_FNS_EMPTY.as_ptr(),
                },
            },
        }
    };
}

// ─── Hot-reload runtime + path helpers ────────────────────────────────────────

pub(crate) fn v1_so_path() -> PathBuf {
    let filename: &str = if cfg!(target_os = "macos") {
        "libreload_plugin_v1.dylib"
    } else if cfg!(target_os = "windows") {
        "reload_plugin_v1.dll"
    } else {
        "libreload_plugin_v1.so"
    };
    PathBuf::from(RELOAD_V1_DIR).join(filename)
}

pub(crate) fn v2_so_path() -> PathBuf {
    let filename: &str = if cfg!(target_os = "macos") {
        "libreload_plugin_v2.dylib"
    } else if cfg!(target_os = "windows") {
        "reload_plugin_v2.dll"
    } else {
        "libreload_plugin_v2.so"
    };
    PathBuf::from(RELOAD_V2_DIR).join(filename)
}

pub(crate) fn hot_reload_config() -> RuntimeConfig {
    RuntimeConfig {
        compatibility: polyplug_abi::runtime::Compatibility::Strict,
        hot_reload_enabled: true,
        on_reload: None,
        on_reload_user_data: core::ptr::null_mut(),
        ..Default::default()
    }
}

pub(crate) fn make_hot_reload_runtime() -> Arc<Runtime> {
    Runtime::builder()
        .config(hot_reload_config())
        .loader(TestNativeLoader::new())
        .build()
        .expect("runtime build must succeed")
}

/// Resolve `functions[0]` of a contract's native interface as the
/// `extern "C" fn() -> u32` version function the reload test plugins export.
///
/// Pins the epoch across resolve + deref so the interface stays alive while its
/// native function table is read, even if a reload republishes concurrently.
pub(crate) fn resolve_version_fn(rt: &Runtime, contract_id: u64) -> Option<extern "C" fn() -> u32> {
    let _epoch_guard: crossbeam_epoch::Guard = crossbeam_epoch::pin();
    let handle: polyplug_abi::GuestContractHandle = rt.find_guest_contract(contract_id, 0).ok()?;
    let interface_ptr: *const GuestContractInterface = rt.resolve_guest_contract(handle).ok()?;
    // SAFETY: the epoch guard pinned above keeps the resolved interface alive across this
    // deref. dispatch.native is the active variant for these native test interfaces, and
    // functions[0] is the version fn matching the `extern "C" fn() -> u32` signature the
    // test plugins export.
    let fn_ptr: extern "C" fn() -> u32 = unsafe {
        let fns: *const *const () = (*interface_ptr).dispatch.native.functions;
        core::mem::transmute(*fns)
    };
    Some(fn_ptr)
}
