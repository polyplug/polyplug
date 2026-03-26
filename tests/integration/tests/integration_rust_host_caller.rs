#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use polyplug::registry::PluginGuard;
use polyplug::runtime::Runtime;
use polyplug_abi::AbiError;
use polyplug_abi::PluginHandle;
use polyplug_abi::PluginInterface;
use polyplug_abi::ABI_OK;
use polyplug_native::NativeLoader;

const TEST_ADD_CONTRACT_ID: u64 = 0xCC4232FAB0410D2B_u64;

#[repr(C)]
pub struct AddArgs {
    pub a: u32,
    pub b: u32,
}

#[derive(Debug)]
pub struct ContractError {
    pub code: u32,
    pub message: String,
}

impl ContractError {
    pub fn new(code: u32) -> Self {
        Self {
            code,
            message: String::new(),
        }
    }
}

pub struct TestAddContract {
    guard: PluginGuard,
}

impl TestAddContract {
    pub fn create(runtime: &'static Runtime, min_version: u32) -> Option<Self> {
        let handle: PluginHandle = runtime
            .find_by_contract(TEST_ADD_CONTRACT_ID, min_version)
            .ok()?;
        let guard: PluginGuard = runtime.registry().resolve_guard(handle).ok()?;
        Some(Self { guard })
    }

    pub fn is_valid(&self) -> bool {
        true
    }

    pub fn reset(&mut self) {}

    #[allow(clippy::absurd_extreme_comparisons)]
    pub fn add(&self, a: u32, b: u32) -> Result<u32, ContractError> {
        let args: AddArgs = AddArgs { a, b };
        // SAFETY: u32 is a primitive type with no invalid bit patterns, so zeroed() is safe.
        let mut out_val: u32 = unsafe { core::mem::zeroed() };
        // SAFETY: args_ptr points to a valid AddArgs and out_ptr to a valid u32.
        let args_ptr: *const () = &args as *const AddArgs as *const ();
        let out_ptr: *mut () = &mut out_val as *mut u32 as *mut ();
        let vtable_ptr: *const PluginInterface = self.guard.vtable();
        // SAFETY: vtable_ptr is valid for the duration of the call.
        let err: AbiError = unsafe {
            let vtable: &PluginInterface = &*vtable_ptr;
            if 0_u32 >= vtable.function_count {
                AbiError {
                    code: polyplug_abi::ABI_FUNCTION_NOT_AVAIL,
                    message: polyplug_abi::StringView::null(),
                }
            } else if vtable.dispatch_type != polyplug_abi::DispatchType::Native {
                AbiError {
                    code: polyplug_abi::ABI_ERROR_GENERIC,
                    message: polyplug_abi::StringView::null(),
                }
            } else {
                let fn_ptr: *const () = *vtable.dispatch.native.functions.add(0_usize);
                let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
                    core::mem::transmute(fn_ptr);
                dispatch_fn(args_ptr, out_ptr)
            }
        };
        if err.code != ABI_OK {
            return Err(ContractError {
                code: err.code,
                message: String::new(),
            });
        }
        Ok(out_val)
    }
}

fn create_static_runtime() -> &'static Runtime {
    let rt: Runtime = Runtime::builder()
        .loader(NativeLoader::new(polyplug_native::NativeConfig::default()))
        .build()
        .expect("build runtime");
    Box::leak(Box::new(rt))
}

#[test]
fn test_host_caller_factory_method_returns_some_when_plugin_exists() {


    let rt: &'static Runtime = create_static_runtime();

    let plugin_dir: &str = env!("TEST_PLUGIN_DIR");
    rt.load_bundle(std::path::Path::new(plugin_dir))
        .expect("load plugin");

    let caller: Option<TestAddContract> = TestAddContract::create(rt, 0);
    assert!(
        caller.is_some(),
        "create() should return Some when plugin exists"
    );
}

#[test]
fn test_host_caller_factory_method_returns_none_when_plugin_not_found() {


    let rt: &'static Runtime = create_static_runtime();

    let caller: Option<TestAddContract> = TestAddContract::create(rt, 0);
    assert!(
        caller.is_none(),
        "create() should return None when no plugin loaded"
    );
}

#[test]
fn test_host_caller_is_valid_returns_true() {


    let rt: &'static Runtime = create_static_runtime();

    let plugin_dir: &str = env!("TEST_PLUGIN_DIR");
    rt.load_bundle(std::path::Path::new(plugin_dir))
        .expect("load plugin");

    let caller: TestAddContract = TestAddContract::create(rt, 0).expect("caller should exist");

    assert!(caller.is_valid(), "is_valid() should return true");
}

#[test]
fn test_host_caller_method_call_works() {


    let rt: &'static Runtime = create_static_runtime();

    let plugin_dir: &str = env!("TEST_PLUGIN_DIR");
    rt.load_bundle(std::path::Path::new(plugin_dir))
        .expect("load plugin");

    let caller: TestAddContract = TestAddContract::create(rt, 0).expect("caller should exist");

    let result: u32 = caller.add(10, 32).expect("add should succeed");
    assert_eq!(result, 42_u32, "add(10, 32) should return 42");
}

#[test]
fn test_host_caller_reset_is_noop() {


    let rt: &'static Runtime = create_static_runtime();

    let plugin_dir: &str = env!("TEST_PLUGIN_DIR");
    rt.load_bundle(std::path::Path::new(plugin_dir))
        .expect("load plugin");

    let mut caller: TestAddContract = TestAddContract::create(rt, 0).expect("caller should exist");

    caller.reset();
    assert!(
        caller.is_valid(),
        "is_valid() should still return true after reset"
    );

    let result: u32 = caller.add(5, 7).expect("add should work after reset");
    assert_eq!(result, 12_u32, "add(5, 7) should return 12");
}
