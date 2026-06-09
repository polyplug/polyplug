//! Integration test: Rust guest→guest peer caller pattern at runtime.
//!
//! The generated `peer_callers.rs` inside a native cdylib calls
//! `host->find_guest_contract → resolve_guest_contract → create_instance →
//! call_guest_method` to cross-dispatch to another loaded contract.
//!
//! This test replicates that EXACT pattern inline — without needing to compile
//! a second cdylib — by calling the same sequence of `HostApi` function pointers
//! through `Runtime::host_abi()`. This is equivalent to what a loaded Rust
//! peer-caller cdylib would do: both paths go through the same `call_guest_method`
//! implementation in the runtime.
//!
//! The flow:
//!   1. A Lua bundle registers `test.peer@1` with one function:
//!      `echo(StringView) -> StringView` that returns `"PEER:"` + input.
//!   2. The test obtains the live `HostApi` pointer from the runtime.
//!   3. Using only `HostApi` function pointers (exactly as the generated
//!      `peer_callers.rs` does), it:
//!        - calls `find_guest_contract` to get a handle for `test.peer@1`
//!        - calls `resolve_guest_contract` to get `*const GuestContractInterface`
//!        - calls `create_instance` on the interface
//!        - constructs a `CallArena` over a stack buffer (512 bytes)
//!        - calls `call_guest_method` with a `StringView` arg + `StringView` out
//!   4. The test asserts the returned `StringView` equals `"PEER:hello"`.
//!
//! This directly exercises the `call_guest_method` path and arena lifetime that
//! the generated Rust peer caller relies on, covering the StringView borrowed-view
//! return fix from #70/#71.
//!
//! LuaJIT is always vendored, so no skip path is needed.

#![allow(clippy::expect_used)]
#![allow(clippy::undocumented_unsafe_blocks)]

use polyplug::runtime::Runtime;
use polyplug_abi::AbiError;
use polyplug_abi::AbiErrorCode;
use polyplug_abi::CallArena;
use polyplug_abi::GuestContractHandle;
use polyplug_abi::GuestContractInstance;
use polyplug_abi::GuestContractInterface;
use polyplug_abi::HostApi;
use polyplug_abi::StringView;
use polyplug_lua::LuaConfig;
use polyplug_lua::LuaLoader;
use polyplug_utils::bundle_id;
use polyplug_utils::guest_contract_id;
use std::path::PathBuf;
use std::sync::Arc;

// ─── Provider Lua bundle ────────────────────────────────────────────────────────

fn provider_lua_src() -> &'static str {
    r#"
local ffi = require("ffi")
local polyplug_guest = require("polyplug_guest")

local function impl_echo(args_ptr, out_ptr)
    local in_sv = ffi.cast("const StringView*", ffi.cast("uintptr_t", args_ptr))
    local s = polyplug_guest.to_str(in_sv[0])
    local result = "PEER:" .. s
    local out_view = polyplug_guest.alloc_string_arena(result)
    local out_sv = ffi.cast("StringView*", ffi.cast("uintptr_t", out_ptr))
    out_sv[0] = out_view
end

function polyplug_init(registrar_ptr, ctx_ptr)
    _G._polyplug_handlers = {
        ["test.peer"] = {
            contract_version = 1,
            plugin_name = "test-peer-provider-for-rust",
            functions = { [0] = impl_echo },
        },
    }
end
"#
}

fn write_lua_provider(tmp: &std::path::Path) -> PathBuf {
    let dir: PathBuf = tmp.join("rust_peer_test_provider");
    std::fs::create_dir_all(&dir).expect("create provider dir");

    let id_val: u64 = bundle_id("rust_peer_test_provider");
    let manifest: String = format!(
        "name = \"rust_peer_test_provider\"\n\
         id = {id_val}\n\
         bundle_name = \"rust_peer_test_provider\"\n\
         version = \"1.0.0\"\n\
         runtime = \"lua\"\n\
         file = \"provider.lua\"\n\
         provides = [\"test.peer@1\"]\n\
         needs_reinit_on_dep_reload = false\n\n\
         [function_count]\n\
         \"test.peer@1\" = 1\n",
    );
    std::fs::write(dir.join("manifest.toml"), manifest).expect("write manifest.toml");
    std::fs::write(dir.join("provider.lua"), provider_lua_src()).expect("write provider.lua");

    // Copy the Lua SDK files so `require("polyplug_guest")` resolves.
    let fixtures_lua: PathBuf = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("parent of tests/integration")
        .join("fixtures")
        .join("test_plugin_lua");

    let polyplug_dir: PathBuf = dir.join("polyplug");
    std::fs::create_dir_all(&polyplug_dir).expect("create polyplug dir");
    let fixture_polyplug: PathBuf = fixtures_lua.join("polyplug");
    for entry in std::fs::read_dir(&fixture_polyplug).expect("read fixture polyplug dir") {
        let entry: std::fs::DirEntry = entry.expect("dir entry");
        std::fs::copy(entry.path(), polyplug_dir.join(entry.file_name()))
            .expect("copy polyplug sdk file");
    }
    std::fs::copy(
        fixtures_lua.join("polyplug_abi.lua"),
        dir.join("polyplug_abi.lua"),
    )
    .expect("copy polyplug_abi.lua");
    std::fs::copy(
        fixtures_lua.join("polyplug_guest.lua"),
        dir.join("polyplug_guest.lua"),
    )
    .expect("copy polyplug_guest.lua");

    dir
}

// ─── Inline Rust peer caller (mirrors generated peer_callers.rs) ────────────────

/// Peer caller for `test.peer@1` implemented inline following the exact pattern
/// that `polyplugc` generates in `peer_callers.rs`.
///
/// Compare with `examples/guests/rust/transformer/generated/guest/peer_callers.rs`:
///   - `resolve()` calls `find_guest_contract` + `resolve_guest_contract` + `create_instance`
///   - `echo()` resets the arena, marshals the `StringView` arg, calls `call_guest_method`
///   - `Drop` calls `arena.reset()` + `destroy_instance`
struct TestPeerCaller {
    interface: *const GuestContractInterface,
    instance: GuestContractInstance,
    host: *const HostApi,
    /// Stable-address backing buffer for the per-call arena.
    _arena_buf: Box<[u8; 512]>,
    arena: CallArena,
}

impl TestPeerCaller {
    /// Resolve the peer contract through the host — mirrors `PipelineValidatorContractPeer::resolve()`.
    fn resolve(host: *const HostApi) -> Option<Self> {
        // SAFETY: host is non-null; `as_ref()` returns None for null pointers.
        let iface_api: &HostApi = unsafe { host.as_ref()? };
        let peer_id: u64 = guest_contract_id("test.peer", 1);
        let handle: GuestContractHandle =
            unsafe { (iface_api.find_guest_contract)(host, peer_id, 1_u32) };
        let interface: *const GuestContractInterface =
            unsafe { (iface_api.resolve_guest_contract)(host, handle) };
        if interface.is_null() {
            return None;
        }
        // SAFETY: interface is non-null and valid for the runtime lifetime.
        let created: GuestContractInstance =
            unsafe { ((*interface).create_instance)(host, core::ptr::null()) };
        // Stamp the peer contract id so `call_guest_method` routes by it — the VM
        // provider's create_instance returns a null-id handle. Mirrors the fix in
        // the generated `peer_callers.rs`.
        let instance: GuestContractInstance = GuestContractInstance {
            data: created.data,
            contract_id: polyplug_utils::GuestContractId::from_u64(peer_id),
        };
        let mut arena_buf: Box<[u8; 512]> = Box::new([0u8; 512]);
        let arena: CallArena = CallArena::new(arena_buf.as_mut_slice(), host);
        Some(TestPeerCaller {
            interface,
            instance,
            host,
            _arena_buf: arena_buf,
            arena,
        })
    }

    /// Call `test.peer@1::echo(input)` via `call_guest_method`.
    ///
    /// Mirrors `PipelineValidatorContractPeer::validate(&mut self, input: StringView)`.
    /// Returns a `StringView` that borrows the caller's arena; it stays valid until
    /// the next call that resets the arena.
    fn echo(&mut self, input: StringView) -> Result<StringView, AbiErrorCode> {
        if self.interface.is_null() {
            return Err(AbiErrorCode::NotFound);
        }
        // Reset at call start: rewinds the arena, invalidating prior views.
        self.arena.reset();
        let input_val: StringView = input;
        let args_ptr: *const core::ffi::c_void =
            &input_val as *const StringView as *const core::ffi::c_void;
        let mut out_val: StringView = unsafe { core::mem::zeroed() };
        let out_ptr: *mut core::ffi::c_void =
            &mut out_val as *mut StringView as *mut core::ffi::c_void;
        // SAFETY: host is non-null (set in resolve()); interface and instance are
        // valid for the runtime lifetime. args_ptr/out_ptr are valid StringView slots.
        let err: AbiError = unsafe {
            let iface_api: &HostApi = &*self.host;
            (iface_api.call_guest_method)(
                self.host,
                self.instance,
                0_u32,
                args_ptr,
                out_ptr,
                &mut self.arena as *mut CallArena,
            )
        };
        if err.code != AbiErrorCode::Ok as u32 {
            return Err(AbiErrorCode::from_u32(err.code));
        }
        Ok(out_val)
    }
}

impl Drop for TestPeerCaller {
    fn drop(&mut self) {
        // Free arena overflow blocks, then destroy the instance.
        self.arena.reset();
        if !self.instance.data.is_null() {
            // SAFETY: interface is valid; instance was created by this caller.
            unsafe {
                ((*self.interface).destroy_instance)(self.host, self.instance);
            }
        }
    }
}

// ─── Test ──────────────────────────────────────────────────────────────────────

#[test]
fn rust_peer_caller_echo_roundtrip() {
    let tmp: tempfile::TempDir = tempfile::tempdir().expect("tempdir");
    let provider_dir: PathBuf = write_lua_provider(tmp.path());

    let rt: Arc<Runtime> = Runtime::builder()
        .loader(LuaLoader::new(LuaConfig::default()))
        .build()
        .expect("build runtime");

    rt.load_bundle(&provider_dir)
        .expect("provider bundle must load");

    // Obtain the live HostApi pointer from the runtime — this is exactly what
    // a loaded native cdylib would receive via `polyplug_guest::get_host_vtable()`.
    let host: *const HostApi = rt.host_abi() as *const HostApi;
    assert!(!host.is_null(), "host_abi must be non-null");

    // Resolve the peer contract using the inline peer caller — mirrors generated code.
    let mut caller: TestPeerCaller =
        TestPeerCaller::resolve(host).expect("test.peer@1 must resolve after load");

    // Call echo with input "hello".
    let input: &[u8] = b"hello";
    let input_view: StringView = StringView {
        ptr: input.as_ptr(),
        len: input.len(),
    };
    let result_view: StringView = caller
        .echo(input_view)
        .expect("echo must return Ok");

    assert!(
        !result_view.ptr.is_null(),
        "returned StringView must not be null"
    );
    assert_eq!(result_view.len, 10, "PEER:hello is 10 bytes");

    // SAFETY: result_view.ptr points to arena memory (or host-alloc fallback)
    // valid until the next call that resets the arena. Drop of caller has not
    // happened yet, so the view is still live.
    let result_bytes: &[u8] =
        unsafe { core::slice::from_raw_parts(result_view.ptr, result_view.len) };
    let result: &str = core::str::from_utf8(result_bytes).expect("result is UTF-8");

    assert_eq!(
        result, "PEER:hello",
        "echo must return 'PEER:hello' through call_guest_method"
    );
}
