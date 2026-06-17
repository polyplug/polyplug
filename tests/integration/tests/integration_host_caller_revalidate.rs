//! Runtime test: the GENERATED rust host caller's auto-revalidation across
//! registry mutations.
//!
//! This mirrors the revalidation logic emitted into
//! `examples/hosts/rust/generated/host/host_callers.rs` (cache the resolved
//! interface + the runtime revision counter; before each dispatch poll the
//! counter through `HostApi.revision_counter` and re-resolve on a change) and
//! drives the real `test.add` native contract through the real `HostApi` FFI.
//!
//! It proves the safety property the auto-cache feature delivers — that a cached
//! interface pointer never dangles after a reload/unload:
//!   - a **reload** bumps the registry revision and reclaims the old interface →
//!     the caller observes the change, re-resolves, and still dispatches
//!     correctly (it never touches the reclaimed interface);
//!   - an **unload** bumps the revision and vacates the slot → the caller's
//!     re-resolve fails and it returns `NotFound` instead of dereferencing a
//!     dangling pointer.

#![allow(clippy::expect_used)]

use core::sync::atomic::AtomicU64;
use core::sync::atomic::Ordering;

use polyplug::runtime::Runtime;
use polyplug_abi::AbiError;
use polyplug_abi::AbiErrorCode;
use polyplug_abi::DispatchType;
use polyplug_abi::GuestContractHandle;
use polyplug_abi::GuestContractInstance;
use polyplug_abi::GuestContractInterface;
use polyplug_abi::HostApi;
use polyplug_abi::RuntimeConfig;
use polyplug_native::NativeConfig;
use polyplug_native::NativeLoader;
use polyplug_utils::BundleId;

fn test_add_contract_id() -> u64 {
    polyplug_utils::guest_contract_id("test.add", 1)
}

#[repr(C)]
struct AddArgs {
    a: u32,
    b: u32,
}

/// Faithful mirror of the generated rust host→guest caller's revalidation path.
///
/// Caches the resolved interface, instance, contract handle, and the runtime
/// revision counter pointer. Each dispatch polls the counter (one acquire load)
/// and re-resolves through the retained handle when it changed.
struct RevalidatingAddCaller {
    interface: *const GuestContractInterface,
    instance: GuestContractInstance,
    host: *const HostApi,
    handle: GuestContractHandle,
    revision_ptr: *const u64,
    cached_revision: u64,
}

impl RevalidatingAddCaller {
    fn new(handle: GuestContractHandle, host: *const HostApi) -> Option<Self> {
        // SAFETY: host is a valid HostApi pointer obtained from Runtime::host_abi.
        let host_api: &HostApi = unsafe { host.as_ref()? };
        // SAFETY: resolve_guest_contract is an ABI fn ptr safe to call with a valid host/handle.
        let interface: *const GuestContractInterface =
            unsafe { (host_api.resolve_guest_contract)(host, handle) };
        if interface.is_null() {
            return None;
        }
        let mut instance: GuestContractInstance = GuestContractInstance::null();
        // SAFETY: interface is non-null (checked); create_guest_instance writes the out-param.
        unsafe {
            (host_api.create_guest_instance)(host, interface, core::ptr::null(), &mut instance);
        }
        // SAFETY: revision_counter is an ABI fn ptr returning a pointer to the runtime counter.
        let revision_ptr: *const u64 = unsafe { (host_api.revision_counter)(host) };
        let cached_revision: u64 = read_revision(revision_ptr, 0);
        Some(Self {
            interface,
            instance,
            host,
            handle,
            revision_ptr,
            cached_revision,
        })
    }

    fn live_revision(&self) -> u64 {
        read_revision(self.revision_ptr, self.cached_revision)
    }

    fn cached_revision(&self) -> u64 {
        self.cached_revision
    }

    /// Re-resolve through the retained handle after the registry changed. Returns
    /// `false` when the contract is gone (unload), abandoning the dead instance.
    fn revalidate(&mut self) -> bool {
        // SAFETY: self.host is the stored, valid HostApi pointer.
        let host_api: &HostApi = match unsafe { self.host.as_ref() } {
            Some(api) => api,
            None => return false,
        };
        // SAFETY: resolve_guest_contract is safe to call with a valid host and any handle.
        let interface: *const GuestContractInterface =
            unsafe { (host_api.resolve_guest_contract)(self.host, self.handle) };
        if interface.is_null() {
            return false;
        }
        let mut instance: GuestContractInstance = GuestContractInstance::null();
        // SAFETY: interface is freshly resolved and non-null; create writes the out-param.
        unsafe {
            (host_api.create_guest_instance)(
                self.host,
                interface,
                core::ptr::null(),
                &mut instance,
            );
        }
        self.interface = interface;
        self.instance = instance;
        self.cached_revision = self.live_revision();
        true
    }

    fn add(&mut self, a: u32, b: u32) -> Result<u32, AbiErrorCode> {
        if self.live_revision() != self.cached_revision && !self.revalidate() {
            return Err(AbiErrorCode::NotFound);
        }
        let args: AddArgs = AddArgs { a, b };
        let mut out_val: u32 = 0;
        let args_ptr: *const () = &args as *const AddArgs as *const ();
        let out_ptr: *mut () = &mut out_val as *mut u32 as *mut ();
        // SAFETY: interface is current (revalidated above if the registry changed); the
        // test.add contract is native dispatch with the canonical out-param signature.
        let err: AbiError = unsafe {
            let interface: &GuestContractInterface = &*self.interface;
            if interface.dispatch_type != DispatchType::Native
                || interface.dispatch.native.function_count < 1
            {
                AbiError {
                    code: AbiErrorCode::FunctionNotAvailable as u32,
                    message: polyplug_abi::StringView::null(),
                }
            } else {
                let fn_ptr: *const () = *interface.dispatch.native.functions.add(0_usize);
                let dispatch_fn: unsafe extern "C" fn(
                    GuestContractInstance,
                    *const (),
                    *mut (),
                    *mut AbiError,
                ) = core::mem::transmute(fn_ptr);
                let mut dispatch_err: AbiError = AbiError::ok();
                dispatch_fn(
                    self.instance,
                    args_ptr,
                    out_ptr,
                    &mut dispatch_err as *mut AbiError,
                );
                dispatch_err
            }
        };
        if err.code != AbiErrorCode::Ok as u32 {
            return Err(AbiErrorCode::from_u32(err.code));
        }
        Ok(out_val)
    }
}

/// Read the revision counter through the cached pointer with an acquire load,
/// returning `fallback` when the pointer is null (no runtime).
fn read_revision(revision_ptr: *const u64, fallback: u64) -> u64 {
    if revision_ptr.is_null() {
        return fallback;
    }
    // SAFETY: revision_ptr was returned by HostApi.revision_counter and points to the
    // runtime's revision counter (an AtomicU64) whose address is stable for the runtime's life.
    unsafe { (*(revision_ptr as *const AtomicU64)).load(Ordering::Acquire) }
}

fn hot_reload_runtime() -> &'static Runtime {
    let config: RuntimeConfig = RuntimeConfig {
        hot_reload_enabled: true,
        ..RuntimeConfig::default()
    };
    let rt: std::sync::Arc<Runtime> = Runtime::builder()
        .config(config)
        .loader(NativeLoader::new(NativeConfig::default()))
        .build()
        .expect("build runtime");
    Box::leak(Box::new(rt))
}

fn resolve_caller(rt: &'static Runtime) -> RevalidatingAddCaller {
    let handle: GuestContractHandle = rt
        .find_guest_contract(test_add_contract_id(), 0)
        .expect("find test.add contract");
    RevalidatingAddCaller::new(handle, rt.host_abi()).expect("build caller")
}

/// A reload bumps the registry revision and reclaims the old interface. The
/// caller must observe the change, re-resolve through its retained handle, and
/// still dispatch correctly — never touching the reclaimed old interface.
#[test]
fn caller_revalidates_after_reload_and_still_dispatches() {
    let rt: &'static Runtime = hot_reload_runtime();
    let plugin_dir: &str = env!("TEST_PLUGIN_DIR");
    rt.load_bundle(std::path::Path::new(plugin_dir))
        .expect("load test_plugin");

    let mut caller: RevalidatingAddCaller = resolve_caller(rt);
    assert_eq!(caller.add(10, 32).expect("add before reload"), 42_u32);
    let revision_before: u64 = caller.cached_revision();

    // Reload the bundle: this republishes the registry (bumping the revision) and
    // epoch-reclaims the previously resolved interface.
    rt.reload_bundle(std::path::Path::new(plugin_dir))
        .expect("reload test_plugin");

    assert_ne!(
        caller.live_revision(),
        revision_before,
        "reload must bump the registry revision so the caller knows to re-resolve"
    );

    // The dispatch observes the revision change, revalidates (re-resolving the
    // swapped-in interface and creating a fresh instance), and still returns 42.
    assert_eq!(
        caller
            .add(100, 1)
            .expect("add after reload must auto-revalidate and succeed"),
        101_u32
    );
    assert_ne!(
        caller.cached_revision(),
        revision_before,
        "revalidate() must update the cached revision to the post-reload value"
    );
}

/// An unload bumps the revision and vacates the slot. The caller's re-resolve
/// through the (now stale) handle returns null, so it reports `NotFound` instead
/// of dereferencing the reclaimed interface.
#[test]
fn caller_returns_not_found_after_unload() {
    let rt: &'static Runtime = hot_reload_runtime();
    let plugin_dir: &str = env!("TEST_PLUGIN_DIR");
    rt.load_bundle(std::path::Path::new(plugin_dir))
        .expect("load test_plugin");

    let mut caller: RevalidatingAddCaller = resolve_caller(rt);
    assert_eq!(caller.add(10, 32).expect("add before unload"), 42_u32);
    let revision_before: u64 = caller.cached_revision();

    rt.unload_bundle(BundleId::new("test_plugin"))
        .expect("unload test_plugin");

    assert_ne!(
        caller.live_revision(),
        revision_before,
        "unload must bump the registry revision"
    );

    // The dispatch observes the change, tries to revalidate, finds the contract
    // gone, and returns NotFound — never dereferencing the reclaimed interface.
    match caller.add(10, 32) {
        Err(AbiErrorCode::NotFound) => {}
        other => panic!("expected NotFound after unload, got {other:?}"),
    }
}
