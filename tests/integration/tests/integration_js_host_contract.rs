//! Integration test: a QuickJS guest calls a REAL host contract through the runtime.
//!
//! This proves the `callHostContract` JS trampoline end-to-end at runtime — not
//! just at compile/type level. The flow is:
//!
//!   1. The test registers a real `host.svc` `HostContractInterface` (native
//!      dispatch) on a live `Runtime`. It exposes two methods:
//!        - fn_id 0 — `version() -> u32`: returns the literal 42.
//!        - fn_id 1 — `describe(key: StringView) -> StringView`: records the key
//!          in a process-global slot and returns the static string "DESCRIBED".
//!   2. A self-contained inline `bundle.js` is written to a temp dir alongside a
//!      `manifest.toml` and loaded via `rt.load_bundle`. The guest registers one
//!      guest contract `test.probe` with one function `probe` (fn_id 0).
//!   3. `probe` calls `polyplug.callHostContract(svc_lo, svc_hi, 0, 0, ...)` (version)
//!      and `polyplug.callHostContract(svc_lo, svc_hi, 0, 1, ...)` (describe), then
//!      builds the result string `"v=42;d=DESCRIBED"` in the arena and writes it to `out`.
//!   4. The test dispatches `probe` with input `"hello"`, reads the returned StringView,
//!      and asserts:
//!        - the returned string equals `"v=42;d=DESCRIBED"` (u32 + StringView round-trips)
//!        - the RECEIVED_KEY static equals `Some("hello")` (StringView arg marshalling)

#![allow(clippy::expect_used)]
#![allow(clippy::undocumented_unsafe_blocks)]

use core::ffi::c_void;
use polyplug::runtime::Runtime;
use polyplug_abi::AbiError;
use polyplug_abi::AbiErrorCode;
use polyplug_abi::DispatchMechanisms;
use polyplug_abi::DispatchType;
use polyplug_abi::GuestContractHandle;
use polyplug_abi::GuestContractInstance;
use polyplug_abi::GuestContractInterface;
use polyplug_abi::HostContractInstance;
use polyplug_abi::HostContractInterface;
use polyplug_abi::NativeDispatch;
use polyplug_abi::StringView;
use polyplug_abi::Version;
use polyplug_js::JsConfig;
use polyplug_js::JsLoader;
use polyplug_utils::HostContractId;
use polyplug_utils::guest_contract_id;
use polyplug_utils::host_contract_id;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

/// Records the key the host `describe` thunk received from the guest.
static RECEIVED_KEY: Mutex<Option<String>> = Mutex::new(None);

/// The string `describe` always returns. Must be `'static` so the `StringView`
/// pointing at it remains valid for the lifetime of the call.
static DESCRIBED: &str = "DESCRIBED";

// ─── Real host.svc interface (native dispatch, 2 methods) ─────────────────────

/// fn_id 0: `version() -> u32` — writes 42 into the out buffer.
unsafe extern "C" fn svc_version_thunk(
    _impl_ptr: *const c_void,
    _args: *const (),
    out: *mut (),
) -> AbiError {
    // SAFETY: out is a valid *mut u32 per the host.svc ABI; the buffer is
    // caller-allocated (arena or host alloc) and sized for exactly one u32.
    unsafe {
        *(out as *mut u32) = 42_u32;
    }
    AbiError::ok()
}

/// fn_id 1: `describe(key: StringView) -> StringView` — records the key and
/// returns a `'static` view over the `DESCRIBED` constant.
unsafe extern "C" fn svc_describe_thunk(
    _impl_ptr: *const c_void,
    args: *const (),
    out: *mut (),
) -> AbiError {
    // SAFETY: per the host.svc ABI, args is a valid *const StringView for the
    // duration of this call.
    let key_sv: StringView = unsafe { *(args as *const StringView) };
    let key: String = if key_sv.ptr.is_null() || key_sv.len == 0 {
        String::new()
    } else {
        // SAFETY: ptr is valid for len UTF-8 bytes for the duration of the call.
        let slice: &[u8] = unsafe { core::slice::from_raw_parts(key_sv.ptr, key_sv.len) };
        String::from_utf8_lossy(slice).into_owned()
    };
    *RECEIVED_KEY.lock().expect("RECEIVED_KEY poisoned") = Some(key);

    // SAFETY: out is a valid *mut StringView per the host.svc ABI; DESCRIBED is
    // 'static so the returned view is valid for any future read during this call.
    unsafe {
        *(out as *mut StringView) = StringView {
            ptr: DESCRIBED.as_ptr(),
            len: DESCRIBED.len(),
        };
    }
    AbiError::ok()
}

unsafe extern "C" fn svc_create_instance(
    this: *const HostContractInterface,
    _args: *const (),
) -> HostContractInstance {
    // SAFETY: `this` is the valid interface pointer per the ABI contract.
    HostContractInstance {
        data: unsafe { (*this).user_data },
    }
}

unsafe extern "C" fn svc_destroy_instance(
    _this: *const HostContractInterface,
    _instance: HostContractInstance,
) {
    // Stateless: nothing to clean up.
}

/// Build and leak a real `host.svc` native interface with two functions
/// (version at fn_id 0, describe at fn_id 1). Leaked for `'static` lifetime.
fn make_host_svc_interface() -> &'static HostContractInterface {
    // The function-pointer table must outlive the interface; leak a Box to give it
    // a stable 'static address (raw fn pointers are not `Sync`, so a `static` array
    // is rejected — leaking mirrors the generated interface_factories.rs pattern).
    let functions: &'static [*const (); 2] = Box::leak(Box::new([
        svc_version_thunk as *const (),
        svc_describe_thunk as *const (),
    ]));

    let interface: HostContractInterface = HostContractInterface {
        contract_id: HostContractId::from(host_contract_id("host.svc", 1)),
        contract_version: Version {
            major: 1,
            minor: 0,
            patch: 0,
        },
        singleton: false,
        dispatch_type: DispatchType::Native,
        runtime: core::ptr::null_mut(),
        user_data: core::ptr::null_mut(),
        create_instance: svc_create_instance,
        destroy_instance: svc_destroy_instance,
        dispatch: DispatchMechanisms {
            native: NativeDispatch {
                function_count: 2_u32,
                functions: functions.as_ptr(),
            },
        },
    };
    Box::leak(Box::new(interface))
}

// ─── JS bundle fixture (self-contained inline) ────────────────────────────────

/// Build the self-contained `bundle.js` source string.
///
/// The guest registers contract `test.probe` (one function, `probe`). `probe`:
///   1. Calls `polyplug.callHostContract(svc_lo, svc_hi, 0, 0, 0, vPtr)` to get
///      the u32 `version` (42) from `host.svc::version`.
///   2. Calls `polyplug.callHostContract(svc_lo, svc_hi, 0, 1, aPtr, dPtr)` to
///      get the StringView `"DESCRIBED"` from `host.svc::describe(input)`.
///   3. Builds the result `"v=42;d=DESCRIBED"` in the arena and writes it to `out`
///      as a StringView `{ ptr_lo, ptr_hi, len }`.
///
/// In the JS, StringView args/out follow the generator convention: three u32s
/// (ptr_lo@+0, ptr_hi@+4, len@+8) split from the 8-byte pointer and 8-byte len.
fn make_bundle_js(svc_lo: u32, svc_hi: u32, probe_lo: u32, probe_hi: u32) -> String {
    format!(
        r#"
function polyplug_init(rt_ctx, host_vtable, ctx) {{
    var polyplug = globalThis.polyplug;
    function probe(args, out) {{
        var polyplug = globalThis.polyplug;
        // Read input StringView from args (ptr_lo@0, ptr_hi@4, len@8).
        var inLo = polyplug.readU32(args);
        var inHi = polyplug.readU32(args + 4);
        var inLen = polyplug.readU32(args + 8);
        // 1) Call host.svc.version() -> u32 (fn_id 0, no args).
        var vBuf = polyplug.arenaAlloc(8);
        var vPtr = vBuf[0] + vBuf[1] * 4294967296;
        polyplug.writeU32(vPtr, 0); polyplug.writeU32(vPtr + 4, 0);
        var e1 = polyplug.callHostContract({svc_lo}, {svc_hi}, 0, 0, 0, vPtr);
        if (e1 !== 0) {{ return e1; }}
        var version = polyplug.readU32(vPtr);
        // 2) Call host.svc.describe(input) -> StringView (fn_id 1).
        var aBuf = polyplug.arenaAlloc(16);
        var aPtr = aBuf[0] + aBuf[1] * 4294967296;
        polyplug.writeU32(aPtr,      inLo);
        polyplug.writeU32(aPtr + 4,  inHi);
        polyplug.writeU32(aPtr + 8,  inLen);
        polyplug.writeU32(aPtr + 12, 0);
        var dBuf = polyplug.arenaAlloc(16);
        var dPtr = dBuf[0] + dBuf[1] * 4294967296;
        polyplug.writeU32(dPtr,      0);
        polyplug.writeU32(dPtr + 4,  0);
        polyplug.writeU32(dPtr + 8,  0);
        polyplug.writeU32(dPtr + 12, 0);
        var e2 = polyplug.callHostContract({svc_lo}, {svc_hi}, 0, 1, aPtr, dPtr);
        if (e2 !== 0) {{ return e2; }}
        var dLo  = polyplug.readU32(dPtr);
        var dHi  = polyplug.readU32(dPtr + 4);
        var dLen = polyplug.readU32(dPtr + 8);
        var dAddr = dLo + dHi * 4294967296;
        var dStr = "";
        for (var i = 0; i < dLen; i++) {{
            dStr += String.fromCharCode(polyplug.readByte(dAddr + i));
        }}
        // 3) Build result "v=<version>;d=<dStr>" and write it to out as StringView.
        var res = "v=" + version + ";d=" + dStr;
        var rBuf = polyplug.arenaAlloc(res.length);
        var rPtr = rBuf[0] + rBuf[1] * 4294967296;
        for (var j = 0; j < res.length; j++) {{
            polyplug.writeByte(rPtr + j, res.charCodeAt(j));
        }}
        polyplug.writeU32(out,      rBuf[0]);
        polyplug.writeU32(out + 4,  rBuf[1]);
        polyplug.writeU32(out + 8,  res.length);
        polyplug.writeU32(out + 12, 0);
        return 0;
    }}
    var vtable = {{
        contractLo: {probe_lo},
        contractHi: {probe_hi},
        fnCount: 1,
        contractName: "test.probe",
        version: 0x00010000,
        functions: [probe]
    }};
    polyplug.registerVtable(
        vtable.contractLo,
        vtable.contractHi,
        vtable,
        vtable.fnCount,
        vtable.contractName,
        vtable.version
    );
}}
"#,
        svc_lo = svc_lo,
        svc_hi = svc_hi,
        probe_lo = probe_lo,
        probe_hi = probe_hi,
    )
}

/// Write `bundle.js` + `manifest.toml` into a temp dir and return the dir path.
fn write_temp_bundle(js_source: &str, bundle_name: &str) -> (tempfile::TempDir, PathBuf) {
    let dir: tempfile::TempDir = tempfile::tempdir().expect("tempdir");
    let bundle_path: PathBuf = dir.path().join("bundle.js");
    std::fs::write(&bundle_path, js_source).expect("write bundle.js");

    let bundle_id_val: u64 = polyplug_utils::bundle_id(bundle_name);
    let manifest: String = format!(
        "id = {}\nname = \"{}\"\nruntime = \"js-quickjs\"\nfile = \"bundle.js\"\n",
        bundle_id_val, bundle_name,
    );
    std::fs::write(dir.path().join("manifest.toml"), &manifest).expect("write manifest.toml");

    (dir, bundle_path)
}

// ─── Test ─────────────────────────────────────────────────────────────────────

#[test]
fn js_guest_calls_real_host_contract() {
    // Reset the global capture before this test.
    *RECEIVED_KEY.lock().expect("RECEIVED_KEY poisoned") = None;

    // Compute all IDs once so they can be embedded in the JS source.
    let svc_id: u64 = host_contract_id("host.svc", 1);
    let svc_lo: u32 = svc_id as u32;
    let svc_hi: u32 = (svc_id >> 32) as u32;
    let probe_id: u64 = guest_contract_id("test.probe", 1);
    let probe_lo: u32 = probe_id as u32;
    let probe_hi: u32 = (probe_id >> 32) as u32;

    let js_source: String = make_bundle_js(svc_lo, svc_hi, probe_lo, probe_hi);
    let (tmp_dir, _bundle_js_path) = write_temp_bundle(&js_source, "js.host.contract.test");

    let rt: Arc<Runtime> = Runtime::builder()
        .loader(JsLoader::new(JsConfig {}))
        .build()
        .expect("build runtime");

    // Register the REAL host.svc interface BEFORE loading the guest bundle.
    let svc_iface: &'static HostContractInterface = make_host_svc_interface();
    rt.register_host_contract(host_contract_id("host.svc", 1), svc_iface)
        .expect("register host.svc");

    rt.load_bundle(tmp_dir.path()).expect("js bundle must load");

    // Resolve the guest contract `test.probe`.
    let handle: GuestContractHandle = rt
        .find_guest_contract(probe_id, 0)
        .expect("test.probe must be registered after load");
    let vtable_ptr: *const GuestContractInterface = rt
        .resolve_guest_contract(handle)
        .expect("handle must resolve");
    // SAFETY: vtable_ptr is a live interface for the runtime lifetime.
    let vtable: &GuestContractInterface = unsafe { &*vtable_ptr };
    assert_eq!(
        vtable.dispatch_type,
        DispatchType::VirtualMachine,
        "JS guests use VM dispatch"
    );

    // Build the StringView input ("hello") to pass to probe.
    let input: &[u8] = b"hello";
    let input_view: StringView = StringView {
        ptr: input.as_ptr(),
        len: input.len(),
    };
    let mut out_view: StringView = StringView::null();

    // SAFETY: dispatch_type is VirtualMachine so the vm union variant is active;
    // args/out match probe's ABI (StringView in, StringView out); null arena
    // selects the host->alloc fallback for arenaAlloc inside the JS guest.
    let err: AbiError = unsafe {
        (vtable.dispatch.vm.call)(
            vtable.dispatch.vm.loader_data,
            GuestContractInstance::null(),
            0_u32,
            &input_view as *const StringView as *const (),
            &mut out_view as *mut StringView as *mut (),
            core::ptr::null_mut(),
        )
    };
    assert_eq!(
        err.code,
        AbiErrorCode::Ok as u32,
        "probe must return AbiErrorCode::Ok; got code={}",
        err.code
    );

    // Decode the returned StringView into an owned String.
    assert!(
        !out_view.ptr.is_null(),
        "returned StringView must not be null"
    );
    // SAFETY: out_view.ptr is valid for out_view.len bytes; the bytes were written
    // by the guest into arena memory that is valid until the next arena reset (this
    // test uses a null arena, so arenaAlloc falls back to host->alloc, meaning the
    // pointer is valid until the host frees it — the runtime frees on bundle unload,
    // which has not happened yet).
    let result_bytes: &[u8] = unsafe { core::slice::from_raw_parts(out_view.ptr, out_view.len) };
    let result: String = String::from_utf8_lossy(result_bytes).into_owned();

    assert_eq!(
        result, "v=42;d=DESCRIBED",
        "probe must return 'v=42;d=DESCRIBED' to prove u32 and StringView round-trips"
    );

    // The describe thunk must have recorded the key "hello" — proves StringView
    // arg marshalling reached the native host thunk intact.
    let received: Option<String> = RECEIVED_KEY.lock().expect("RECEIVED_KEY poisoned").clone();
    assert_eq!(
        received.as_deref(),
        Some("hello"),
        "host.svc::describe must have received the guest's key 'hello'"
    );
}
