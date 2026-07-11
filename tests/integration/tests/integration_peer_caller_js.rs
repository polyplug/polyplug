//! Integration test: JS (QuickJS) guest→guest peer caller at runtime.
//!
//! This proves the JS `dispatchPeer` peer-caller bridge executes end-to-end
//! — not just that the generated `peer_callers.ts` text contains the right
//! strings. The flow is:
//!
//!   1. A **provider** JS bundle registers contract `test.peer@1`, whose single
//!      function `echo` (fn_id 0) reads a `StringView` arg and returns it
//!      prefixed with `"PEER:"`, written as a `StringView` via `arenaAlloc`.
//!   2. A **consumer** JS bundle registers contract `test.consumer@1`. Its
//!      single function `invoke` (fn_id 0) uses the peer-caller idiom from the
//!      generated `peer_callers.ts` — calling `polyplug.dispatchPeer` — to
//!      invoke `test.peer@1::echo`, then writes the result to `out`.
//!   3. Both bundles are loaded into the same `Runtime` (JsLoader). The provider
//!      is loaded first so `test.peer@1` is registered when the consumer resolves
//!      it inside `dispatchPeer`.
//!   4. The test dispatches the consumer's `invoke` with input `"hello"`, reads
//!      the returned `StringView`, and asserts the value equals `"PEER:hello"`.
//!
//! This exercises the JS `dispatchPeer` bridge end-to-end, the
//! borrowed-view `StringView` return path (arenaAlloc), and the full
//! guest→host→guest dispatch chain at runtime.

#![allow(clippy::expect_used)]
#![allow(clippy::undocumented_unsafe_blocks)]

use polyplug::runtime::Runtime;
use polyplug_abi::AbiError;
use polyplug_abi::AbiErrorCode;
use polyplug_abi::DispatchType;
use polyplug_abi::GuestContractHandle;
use polyplug_abi::GuestContractInstance;
use polyplug_abi::GuestContractInterface;

use polyplug_js::JsLoader;
use polyplug_utils::bundle_id;
use polyplug_utils::guest_contract_id;
use std::path::PathBuf;
use std::sync::Arc;

// ─── Bundle helpers ────────────────────────────────────────────────────────────

/// Write a `bundle.js` + `manifest.toml` into `dir`.
///
/// `js_src` is the complete bundle JS source.
/// `provides` is the contract name (without version suffix).
/// `fn_count` is the number of functions the contract exposes.
fn write_js_bundle(dir: &std::path::Path, name: &str, js_src: &str, provides: &str, fn_count: u32) {
    let id_val: u64 = bundle_id(name);
    let contract_version: u32 = 1;
    let manifest: String = format!(
        "name = \"{name}\"\n\
         id = {id_val}\n\
         bundle_name = \"{name}\"\n\
         version = \"1.0.0\"\n\
         loader = \"js-quickjs\"\n\
         file = \"bundle.js\"\n\
         provides = [\"{provides}@{contract_version}\"]\n\
         needs_reinit_on_dep_reload = false\n\n\
         [function_count]\n\
         \"{provides}@{contract_version}\" = {fn_count}\n",
    );
    std::fs::write(dir.join("manifest.toml"), manifest).expect("write manifest.toml");
    std::fs::write(dir.join("bundle.js"), js_src).expect("write bundle.js");
}

// ─── Provider JS bundle ────────────────────────────────────────────────────────

/// Build the provider `bundle.js` source.
///
/// Registers `test.peer@1` (contract_id = 0x8402886CF97A6E7D) with a single
/// function `echo` (fn_id 0):
///   - Reads the input `StringView` (ptr_lo@+0, ptr_hi@+4, len@+8 in args).
///   - Copies the bytes and prepends `"PEER:"` via `arenaAlloc`.
///   - Writes the result `StringView` to `out`.
fn provider_js_src(peer_lo: u32, peer_hi: u32) -> String {
    format!(
        r#"
function echo(impl, argsPtr, outPtr, arena, bridge) {{
    try {{
        var inLo  = bridge.readU32(argsPtr);
        var inHi  = bridge.readU32(argsPtr + 4);
        var inLen = bridge.readU32(argsPtr + 8);
        var inAddr = inHi * 0x100000000 + inLo;

        var prefix = "PEER:";
        var totalLen = prefix.length + inLen;
        var outBuf = bridge.arenaAlloc(totalLen === 0 ? 1 : totalLen, arena);
        var outAddr = outBuf[1] * 0x100000000 + outBuf[0];

        for (var i = 0; i < prefix.length; i++) {{
            bridge.writeByte(outAddr + i, prefix.charCodeAt(i));
        }}
        for (var j = 0; j < inLen; j++) {{
            bridge.writeByte(outAddr + prefix.length + j, bridge.readByte(inAddr + j));
        }}

        bridge.writeU32(outPtr,      outBuf[0]);
        bridge.writeU32(outPtr + 4,  outBuf[1]);
        bridge.writeU32(outPtr + 8,  totalLen);
        bridge.writeU32(outPtr + 12, 0);
        return 0;
    }} catch (e) {{
        return 1;
    }}
}}

function polyplug_init(host_lo, host_hi, ctx_lo, ctx_hi, bridge) {{
    var registrations = [{{
        contractLo: {peer_lo} >>> 0, contractHi: {peer_hi} >>> 0,
        interface: {{ contractLo: {peer_lo} >>> 0, contractHi: {peer_hi} >>> 0, fnCount: 1,
           contractName: "test.peer", version: 0x10000,
           factory: function(bridge, hostLo, hostHi) {{ return {{}}; }},
           functions: [echo] }},
        fnCount: 1, contractName: "test.peer", version: 0x10000
    }}];
    return [registrations, {{ code: 0, message: "" }}];
}}
"#,
        peer_lo = peer_lo,
        peer_hi = peer_hi,
    )
}

// ─── Consumer JS bundle ────────────────────────────────────────────────────────

/// Build the consumer `bundle.js` source.
///
/// Registers `test.consumer@1` with a single function `invoke` (fn_id 0)
/// that calls the peer `test.peer@1::echo` through `polyplug.dispatchPeer`,
/// which is the exact bridge tested by the generated `peer_callers.ts`.
///
/// Arguments:
///   - `peer_lo` / `peer_hi`: low/high halves of `test.peer@1` contract id.
///   - `consumer_lo` / `consumer_hi`: low/high halves of `test.consumer@1` id.
fn consumer_js_src(peer_lo: u32, peer_hi: u32, consumer_lo: u32, consumer_hi: u32) -> String {
    format!(
        r#"
function invoke(impl, argsPtr, outPtr, arena, bridge) {{
    try {{
        if (!bridge || !bridge.dispatchPeer) {{ return 1; }}

        // Read the input StringView (ptr_lo@0, ptr_hi@4, len@8).
        var inLo  = bridge.readU32(argsPtr);
        var inHi  = bridge.readU32(argsPtr + 4);
        var inLen = bridge.readU32(argsPtr + 8);

        // Allocate args buffer for the peer call (16 bytes = StringView + padding)
        // from THIS call's arena (threaded in as `arena` — no global).
        var aBuf = bridge.arenaAlloc(16, arena);
        var aPtr = aBuf[0] + aBuf[1] * 4294967296;
        bridge.writeU32(aPtr,      inLo);
        bridge.writeU32(aPtr + 4,  inHi);
        bridge.writeU32(aPtr + 8,  inLen);
        bridge.writeU32(aPtr + 12, 0);

        // Allocate out buffer for the peer's StringView result.
        var oBuf = bridge.arenaAlloc(16, arena);
        var oPtr = oBuf[0] + oBuf[1] * 4294967296;
        bridge.writeU32(oPtr,      0);
        bridge.writeU32(oPtr + 4,  0);
        bridge.writeU32(oPtr + 8,  0);
        bridge.writeU32(oPtr + 12, 0);

        // Call test.peer@1 fn_id 0 (echo) through the direct-dispatch bridge.
        var errCode = bridge.dispatchPeer({peer_lo}, {peer_hi}, 1, 0, aPtr, oPtr);
        if (errCode !== 0) {{ return errCode; }}

        // Read back the StringView from the peer's output and write it to our out.
        var rLo  = bridge.readU32(oPtr);
        var rHi  = bridge.readU32(oPtr + 4);
        var rLen = bridge.readU32(oPtr + 8);
        bridge.writeU32(outPtr,      rLo);
        bridge.writeU32(outPtr + 4,  rHi);
        bridge.writeU32(outPtr + 8,  rLen);
        bridge.writeU32(outPtr + 12, 0);
        return 0;
    }} catch (e) {{
        return 1;
    }}
}}

function polyplug_init(host_lo, host_hi, ctx_lo, ctx_hi, bridge) {{
    var registrations = [{{
        contractLo: {consumer_lo} >>> 0, contractHi: {consumer_hi} >>> 0,
        interface: {{ contractLo: {consumer_lo} >>> 0, contractHi: {consumer_hi} >>> 0, fnCount: 1,
           contractName: "test.consumer", version: 0x10000,
           factory: function(bridge, hostLo, hostHi) {{ return {{}}; }},
           functions: [invoke] }},
        fnCount: 1, contractName: "test.consumer", version: 0x10000
    }}];
    return [registrations, {{ code: 0, message: "" }}];
}}
"#,
        peer_lo = peer_lo,
        peer_hi = peer_hi,
        consumer_lo = consumer_lo,
        consumer_hi = consumer_hi,
    )
}

// ─── Test ──────────────────────────────────────────────────────────────────────

#[test]
fn js_peer_caller_echo_roundtrip() {
    // Compute the contract IDs once; embed them in the JS source.
    let peer_id: u64 = guest_contract_id("test.peer", 1);
    let peer_lo: u32 = peer_id as u32;
    let peer_hi: u32 = (peer_id >> 32) as u32;

    let consumer_id: u64 = guest_contract_id("test.consumer", 1);
    let consumer_lo: u32 = consumer_id as u32;
    let consumer_hi: u32 = (consumer_id >> 32) as u32;

    let tmp: tempfile::TempDir = tempfile::tempdir().expect("tempdir");

    // Write the provider bundle.
    let provider_dir: PathBuf = tmp.path().join("test_peer_provider_js");
    std::fs::create_dir_all(&provider_dir).expect("create provider dir");
    write_js_bundle(
        &provider_dir,
        "test_peer_provider_js",
        &provider_js_src(peer_lo, peer_hi),
        "test.peer",
        1,
    );

    // Write the consumer bundle.
    let consumer_dir: PathBuf = tmp.path().join("test_peer_consumer_js");
    std::fs::create_dir_all(&consumer_dir).expect("create consumer dir");
    write_js_bundle(
        &consumer_dir,
        "test_peer_consumer_js",
        &consumer_js_src(peer_lo, peer_hi, consumer_lo, consumer_hi),
        "test.consumer",
        1,
    );

    let rt: Arc<Runtime> = Runtime::builder()
        .loader(JsLoader::new())
        .build()
        .expect("build runtime");

    // Load provider first so test.peer@1 is registered when the consumer resolves it.
    rt.load_bundle(&provider_dir)
        .expect("provider JS bundle must load");
    rt.load_bundle(&consumer_dir)
        .expect("consumer JS bundle must load");

    // Resolve the consumer contract (test.consumer@1).
    let handle: GuestContractHandle = rt
        .find_guest_contract(consumer_id, 0)
        .expect("test.consumer must be registered after load");
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

    // Dispatch the consumer's fn 0 (invoke) with input "hello".
    // The JS StringView layout is { ptr_lo: u32, ptr_hi: u32, len: u32, pad: u32 }.
    // We pass a 16-byte buffer matching that layout.
    #[repr(C)]
    struct JsStringView {
        ptr_lo: u32,
        ptr_hi: u32,
        len: u32,
        pad: u32,
    }

    let input: &[u8] = b"hello";
    let input_view: JsStringView = JsStringView {
        ptr_lo: input.as_ptr() as usize as u32,
        ptr_hi: (input.as_ptr() as usize >> 32) as u32,
        len: input.len() as u32,
        pad: 0,
    };
    let mut out_view: JsStringView = JsStringView {
        ptr_lo: 0,
        ptr_hi: 0,
        len: 0,
        pad: 0,
    };

    // SAFETY: dispatch_type is VirtualMachine so the vm union is active;
    // args / out are 16-byte JS StringView buffers matching the consumer's ABI;
    // null arena selects the host->alloc fallback inside arenaAlloc.
    let mut err: AbiError = AbiError::ok();
    unsafe {
        (vtable.dispatch.vm.call)(
            vtable.adapter_context,
            vtable.dispatch.vm.loader_data,
            GuestContractInstance::null(),
            0_u32,
            &input_view as *const JsStringView as *const (),
            &mut out_view as *mut JsStringView as *mut (),
            core::ptr::null_mut(),
            &mut err as *mut AbiError,
        )
    };
    assert_eq!(
        err.code,
        AbiErrorCode::Ok as u32,
        "invoke must return AbiErrorCode::Ok; got code={}",
        err.code
    );

    // Decode the StringView the consumer forwarded from the provider.
    let result_addr: usize = (out_view.ptr_hi as usize) << 32 | out_view.ptr_lo as usize;
    assert_ne!(
        result_addr, 0,
        "returned StringView pointer must not be null"
    );
    // SAFETY: result_addr points to out_view.len UTF-8 bytes written by the
    // provider into arena memory. The null-arena fallback used host->alloc, so
    // the bytes are valid until the runtime frees them on bundle unload (not yet).
    let result_bytes: &[u8] =
        unsafe { core::slice::from_raw_parts(result_addr as *const u8, out_view.len as usize) };
    let result: &str = core::str::from_utf8(result_bytes).expect("result is UTF-8");

    assert_eq!(
        result, "PEER:hello",
        "invoke must return 'PEER:hello' proving dispatchPeer StringView round-trip"
    );
}
