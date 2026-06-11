//! Integration test: load a polyplugc-GENERATED C# bundle through the runtime
//! and dispatch a real call.
//!
//! Unlike `integration_dotnet.rs` (which loads the hand-written `csharp_plugin`
//! fixture), this exercises the *generated* guest glue end to end: the bundle in
//! `examples/plugins-csharp/csharp_transformer/` is produced by
//! `examples/build_all.sh`, which runs `polyplugc generate --lang csharp` and
//! `dotnet publish`. Loading it and calling `transform` proves the generated C#
//! ABI thunks, registration, and StringView marshalling work at runtime.
//!
//! The bundle is only built in the CI `examples` job (and locally via
//! `build_all.sh`), so the test skips cleanly when the artifact or `dotnet` is
//! absent — never a silent pass, always an explicit skip log.

#![allow(clippy::expect_used)]
#![allow(clippy::undocumented_unsafe_blocks)]

use polyplug::error::RuntimeError;
use polyplug::runtime::Runtime;
use polyplug_abi::AbiError;
use polyplug_abi::AbiErrorCode;
use polyplug_abi::DispatchType;
use polyplug_abi::GuestContractHandle;
use polyplug_abi::GuestContractInstance;
use polyplug_abi::GuestContractInterface;
use polyplug_abi::StringView;
use polyplug_dotnet::DotnetConfig;
use polyplug_dotnet::DotnetLoader;
use polyplug_dotnet::HostfxrLocation;
use polyplug_utils::guest_contract_id;
use std::path::PathBuf;
use std::sync::Arc;

/// Absolute path to the generated `csharp_transformer` bundle directory.
fn generated_bundle_dir() -> PathBuf {
    // CARGO_MANIFEST_DIR = <workspace>/tests/integration
    let manifest_dir: PathBuf = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root: &std::path::Path = manifest_dir
        .parent()
        .and_then(|p: &std::path::Path| p.parent())
        .expect("workspace root from tests/integration");
    workspace_root
        .join("examples")
        .join("plugins-csharp")
        .join("csharp_transformer")
}

/// True when `dotnet` is on PATH.
fn dotnet_available() -> bool {
    std::process::Command::new("dotnet")
        .arg("--version")
        .output()
        .map(|o: std::process::Output| o.status.success())
        .unwrap_or(false)
}

/// Returns the bundle dir if both `dotnet` and the built artifact are present,
/// otherwise logs a skip reason and returns None.
fn bundle_or_skip() -> Option<PathBuf> {
    if !dotnet_available() {
        eprintln!("SKIP: dotnet not available");
        return None;
    }
    let dir: PathBuf = generated_bundle_dir();
    if !dir.join("transformer.dll").exists() {
        eprintln!(
            "SKIP: generated C# bundle not built at {} (run examples/build_all.sh)",
            dir.display()
        );
        return None;
    }
    Some(dir)
}

fn create_runtime() -> Arc<Runtime> {
    Runtime::builder()
        .loader(DotnetLoader::new(DotnetConfig {
            min_framework: String::from("net10.0"),
            hostfxr: HostfxrLocation::Auto,
        }))
        .build()
        .expect("failed to build runtime")
}

#[test]
fn generated_csharp_bundle_loads() {
    let dir: PathBuf = match bundle_or_skip() {
        Some(d) => d,
        None => return,
    };
    let rt: Arc<Runtime> = create_runtime();
    let result: Result<(), RuntimeError> = rt.load_bundle(&dir);
    assert!(
        result.is_ok(),
        "generated C# bundle must load: {:?}",
        result.err()
    );
}

#[test]
fn generated_csharp_bundle_transform_dispatches() {
    let dir: PathBuf = match bundle_or_skip() {
        Some(d) => d,
        None => return,
    };
    let rt: Arc<Runtime> = create_runtime();
    rt.load_bundle(&dir).expect("generated C# bundle must load");

    // data.Transformer@1 — `transform(StringView) -> StringView`, function id 0.
    let contract_id: u64 = guest_contract_id("data.Transformer", 1);
    let handle: GuestContractHandle = rt
        .find_guest_contract(contract_id, 0)
        .expect("data.Transformer must be registered after load");
    let vtable_ptr: *const GuestContractInterface = rt
        .resolve_guest_contract(handle)
        .expect("handle must resolve");
    // SAFETY: CLR keeps the assembly loaded for the process lifetime.
    let vtable: &GuestContractInterface = unsafe { &*vtable_ptr };

    // C# guests are native-dispatch: the generated interface stores real
    // [UnmanagedCallersOnly] function pointers in `dispatch.native.functions` and
    // is tagged `DispatchType.Native` (a host caller reads the native union
    // variant). This is the parity invariant for C#/Python guests — see CLAUDE.md.
    assert_eq!(
        vtable.dispatch_type,
        DispatchType::Native,
        "C# generated guest must register NATIVE dispatch"
    );
    // SAFETY: dispatch_type is Native, so the native union member is active.
    let function_count: u32 = unsafe { vtable.dispatch.native.function_count };
    assert!(
        function_count >= 1,
        "data.Transformer must expose at least transform()"
    );

    // The guest strips a "DECODED:" prefix, splits on '|', uppercases the name,
    // annotates the value, and increments the trailing count.
    let input: &[u8] = b"DECODED:widget|payload|2";
    let input_view: StringView = StringView {
        ptr: input.as_ptr(),
        len: input.len(),
    };
    let mut out_view: StringView = StringView::null();

    // SAFETY: function 0 is `transform`; args is a *const StringView, out a
    // *mut StringView, matching the generated ABI thunk signature.
    let fn_ptr: *const () = unsafe { *vtable.dispatch.native.functions.add(0) };
    // Native dispatch signature: fn(GuestContractInstance, *const (), *mut ()) -> AbiError.
    let dispatch_fn: unsafe extern "C" fn(GuestContractInstance, *const (), *mut ()) -> AbiError =
        // SAFETY: transmute *const () to the canonical native dispatch fn pointer.
        unsafe { core::mem::transmute(fn_ptr) };
    // The generated CreateInstance constructs the C# implementation via the
    // author factory and carries it in instance.Data (GCHandle); dispatch
    // requires a real instance.
    // SAFETY: host_abi is the runtime's live HostApi pointer; create_instance
    // is the generated factory thunk on the resolved interface.
    let host_abi: *const polyplug_abi::HostApi = rt.host_abi() as *const polyplug_abi::HostApi;
    let instance: GuestContractInstance =
        unsafe { (vtable.create_instance)(host_abi, core::ptr::null()) };
    assert!(
        !instance.data.is_null(),
        "create_instance must produce a non-null instance payload"
    );
    // SAFETY: input_view/out_view are valid and correctly typed for transform;
    // instance was created above.
    let result: AbiError = unsafe {
        dispatch_fn(
            instance,
            &input_view as *const StringView as *const (),
            &mut out_view as *mut StringView as *mut (),
        )
    };
    assert_eq!(
        result.code,
        AbiErrorCode::Ok as u32,
        "transform must return Ok"
    );
    // SAFETY: instance was created by create_instance; destroy exactly once.
    // out_view points to guest/host-owned bytes that outlive the instance.
    unsafe { (vtable.destroy_instance)(host_abi, instance) };

    // SAFETY: out_view points to out_view.len UTF-8 bytes owned by the guest.
    let out_bytes: &[u8] = unsafe { core::slice::from_raw_parts(out_view.ptr, out_view.len) };
    let out_str: &str = core::str::from_utf8(out_bytes).expect("transform output is UTF-8");
    assert_eq!(
        out_str, "TRANSFORMED:WIDGET|payload (transformed)|3",
        "transform must apply the documented transformation"
    );
}
