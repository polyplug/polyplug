//! Integration test: Lua guest→guest peer caller at runtime.
//!
//! This proves the Lua peer-caller generated code executes end-to-end — not
//! just that the generated text contains the right strings. The flow is:
//!
//!   1. A **provider** Lua bundle registers contract `test.peer@1`, whose single
//!      function `echo(StringView) -> StringView` returns its input prefixed with
//!      `"PEER:"`.
//!   2. A **consumer** Lua bundle registers contract `test.consumer@1`. Its single
//!      function `invoke(StringView) -> StringView` uses the peer-caller idiom
//!      from `peer_callers.lua` (inline in the test bundle script) to call
//!      `test.peer::echo` by dispatching DIRECTLY through the cached peer
//!      interface (`interface.dispatch.vm.call`), then returns the result.
//!   3. Both bundles are loaded into the same `Runtime` (LuaLoader). The provider
//!      is loaded first so `test.peer@1` is registered when the consumer resolves it.
//!   4. The test dispatches the consumer's `invoke` with input `"hello"`, reads
//!      the returned `StringView`, and asserts the value equals `"PEER:hello"`.
//!
//! This exercises the borrowed-view StringView return path (#70/#71 fix) and
//! the full guest→guest direct-dispatch chain at runtime.
//!
//! LuaJIT is always vendored, so no skip path is needed.

#![allow(clippy::expect_used)]
#![allow(clippy::undocumented_unsafe_blocks)]

use polyplug::runtime::Runtime;
use polyplug_abi::AbiError;
use polyplug_abi::AbiErrorCode;
use polyplug_abi::DispatchType;
use polyplug_abi::GuestContractHandle;
use polyplug_abi::GuestContractInstance;
use polyplug_abi::GuestContractInterface;
use polyplug_abi::StringView;
use polyplug_lua::LuaConfig;
use polyplug_lua::LuaLoader;
use polyplug_utils::bundle_id;
use polyplug_utils::guest_contract_id;
use std::path::PathBuf;
use std::sync::Arc;

// ─── Bundle helpers ────────────────────────────────────────────────────────────

/// Write a manifest.toml + Lua script into `dir` and return `dir`.
fn write_lua_bundle(
    dir: &std::path::Path,
    name: &str,
    runtime: &str,
    file: &str,
    src: &str,
    provides: &str,
    fn_count: u32,
) {
    let id_val: u64 = bundle_id(name);
    let contract_version: u32 = 1;
    let manifest: String = format!(
        "name = \"{name}\"\n\
         id = {id_val}\n\
         bundle_name = \"{name}\"\n\
         version = \"1.0.0\"\n\
         loader = \"{runtime}\"\n\
         file = \"{file}\"\n\
         provides = [\"{provides}@{contract_version}\"]\n\
         needs_reinit_on_dep_reload = false\n\n\
         [function_count]\n\
         \"{provides}@{contract_version}\" = {fn_count}\n",
    );
    std::fs::write(dir.join("manifest.toml"), manifest).expect("write manifest.toml");
    std::fs::write(dir.join(file), src).expect("write lua bundle file");
}

// ─── Provider bundle source ────────────────────────────────────────────────────

/// Lua source for the provider bundle: registers `test.peer@1` with one function
/// `echo(StringView) -> StringView` that prepends `"PEER:"` to its input.
///
/// The implementation follows the same return-the-registrations protocol as
/// `test_plugin.lua`: `polyplug_init` RETURNS `(registrations, abi_error)` and the
/// LuaLoader wraps each Lua function in a native trampoline.
fn provider_lua_src() -> &'static str {
    r#"
local ffi = require("ffi")
local polyplug_guest = require("polyplug_guest")
local polyplug_abi = require("polyplug_abi")

-- echo(input: StringView) -> StringView
-- Prepends "PEER:" and returns the result sourced from the per-call arena.
local function impl_echo(instance, args_ptr, out_ptr, arena_ptr, arena_alloc)
    local in_sv = ffi.cast("const StringView*", ffi.cast("uintptr_t", args_ptr))
    local s = polyplug_abi.to_str(in_sv[0])
    local result = "PEER:" .. s
    local out_view = polyplug_guest.alloc_string_arena(arena_alloc, arena_ptr, result)
    local out_sv = ffi.cast("StringView*", ffi.cast("uintptr_t", out_ptr))
    out_sv[0] = out_view
end

function polyplug_init(registrar_ptr, ctx_ptr)
    return {
        ["test.peer"] = {
            contract_version = 1,
            plugin_name = "test-peer-provider-lua",
            factory = function(_host) return {} end,
            functions = {
                [0] = impl_echo,
            },
        },
    }, { code = 0 }
end
"#
}

// ─── Consumer bundle source ────────────────────────────────────────────────────

/// Lua source for the consumer bundle: registers `test.consumer@1` with one
/// function `invoke(StringView) -> StringView` that uses the peer-caller idiom
/// to call `test.peer@1::echo` by dispatching DIRECTLY through the cached peer
/// interface, then returns the result.
///
/// The peer-caller idiom mirrors what `peer_callers.lua` generates:
///   resolve via find_guest_contract → resolve_guest_contract → create_guest_instance,
///   then dispatch DIRECTLY through `interface.dispatch.vm.call` (no host-mediated
///   call_guest_method round-trip) → return out_val.
///
/// test.peer@1 contract id: fnv1a_64("guest_contract:test.peer@1")
///                        = 0x8402886CF97A6E7D
fn consumer_lua_src() -> &'static str {
    r#"
local ffi = require("ffi")
local polyplug_guest = require("polyplug_guest")
local polyplug_abi = require("polyplug_abi")

-- Contract ID for test.peer@1: fnv1a_64("guest_contract:test.peer@1")
-- = 0x8402886CF97A6E7D
local TEST_PEER_ID = 0x8402886CF97A6E7DULL

-- invoke(input: StringView) -> StringView
-- Peer-calls test.peer@1::echo(input) via DIRECT cached-interface dispatch and
-- returns the result (the borrowed StringView from the peer call).
-- The author factory captures the threaded host pointer on the instance; the
-- peer-caller idiom reads it from `self` — no per-VM global (Rule 12). Mirrors the
-- generated peer_callers.lua `resolve(host_ptr)` + `validate(input)`.
local function new_consumer(host)
    local self = { _host = host }
    function self:invoke(args_ptr, out_ptr)
        local host_ptr = self._host
        if host_ptr == nil or host_ptr == 0 then
            return
        end
        local host = ffi.cast("HostApi*", ffi.cast("uintptr_t", host_ptr))

        -- Resolve the peer contract.
        local handle = host.find_guest_contract(host, TEST_PEER_ID, 1)
        local interface = host.resolve_guest_contract(host, handle)
        if interface == nil then
            return
        end

        -- Create a peer instance through the host so the runtime tracks it
        -- (stateless contract: instance.data may be null). Out-param ABI:
        -- create_guest_instance writes the instance through a trailing pointer.
        local instance = ffi.new("GuestContractInstance")
        host.create_guest_instance(host, interface, nil, instance)

        -- Forward args directly: both contracts share StringView in / StringView out.
        local args_ptr_v = ffi.cast("const void*", ffi.cast("uintptr_t", args_ptr))
        local out_ptr_v = ffi.cast("void*", ffi.cast("uintptr_t", out_ptr))

        -- Dispatch DIRECTLY through the cached interface. The Lua peer is a VM
        -- contract, so the vm.call branch runs (null arena: host-alloc fallback).
        -- Out-param ABI: the AbiError is written through a trailing pointer.
        local err = ffi.new("AbiError")
        if interface.dispatch_type == 1 then
            interface.dispatch.vm.call(interface.dispatch.vm.loader_data, instance, 0, args_ptr_v, out_ptr_v, nil, err)
        end

        -- Destroy instance through the host (stateless no-op, but honour the lifecycle).
        host.destroy_guest_instance(host, interface, instance)
    end
    return self
end

function polyplug_init(registrar_ptr, ctx_ptr)
    return {
        ["test.consumer"] = {
            contract_version = 1,
            plugin_name = "test-peer-consumer-lua",
            factory = new_consumer,
            functions = {
                [0] = function(instance, a, o) instance:invoke(a, o) end,
            },
        },
    }, { code = 0 }
end
"#
}

// ─── Test ──────────────────────────────────────────────────────────────────────

#[test]
fn lua_peer_caller_echo_roundtrip() {
    let tmp: tempfile::TempDir = tempfile::tempdir().expect("tempdir");

    // Write the provider bundle.
    let provider_dir: PathBuf = tmp.path().join("test_peer_provider");
    std::fs::create_dir_all(&provider_dir).expect("create provider dir");
    write_lua_bundle(
        &provider_dir,
        "test_peer_provider",
        "lua",
        "provider.lua",
        provider_lua_src(),
        "test.peer",
        1,
    );

    // Write the consumer bundle.
    let consumer_dir: PathBuf = tmp.path().join("test_peer_consumer_lua");
    std::fs::create_dir_all(&consumer_dir).expect("create consumer dir");
    write_lua_bundle(
        &consumer_dir,
        "test_peer_consumer_lua",
        "lua",
        "consumer.lua",
        consumer_lua_src(),
        "test.consumer",
        1,
    );

    // Copy the polyplug_guest.lua and polyplug_abi.lua SDKs into each bundle dir
    // so `require("polyplug_guest")` resolves from the bundle's working directory.
    // The LuaLoader sets the working directory to the bundle dir at load time.
    let fixtures_lua: PathBuf = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("parent of tests/integration")
        .join("fixtures")
        .join("test_plugin_lua");

    for bundle_dir in [&provider_dir, &consumer_dir] {
        let polyplug_dir: PathBuf = bundle_dir.join("polyplug");
        std::fs::create_dir_all(&polyplug_dir).expect("create polyplug dir");
        // Copy abi.lua and guest.lua from the fixture's polyplug/ subdir.
        let fixture_polyplug: PathBuf = fixtures_lua.join("polyplug");
        for entry in std::fs::read_dir(&fixture_polyplug).expect("read fixture polyplug dir") {
            let entry: std::fs::DirEntry = entry.expect("dir entry");
            std::fs::copy(entry.path(), polyplug_dir.join(entry.file_name()))
                .expect("copy polyplug sdk file");
        }
        std::fs::copy(
            fixtures_lua.join("polyplug_abi.lua"),
            bundle_dir.join("polyplug_abi.lua"),
        )
        .expect("copy polyplug_abi.lua");
        std::fs::copy(
            fixtures_lua.join("polyplug_guest.lua"),
            bundle_dir.join("polyplug_guest.lua"),
        )
        .expect("copy polyplug_guest.lua");
    }

    // Build the runtime with the Lua loader.
    let rt: Arc<Runtime> = Runtime::builder()
        .loader(LuaLoader::new(LuaConfig::default()))
        .build()
        .expect("build runtime");

    // Load the provider FIRST so test.peer@1 is registered when the consumer resolves it.
    rt.load_bundle(&provider_dir)
        .expect("provider bundle must load");

    // Load the consumer.
    rt.load_bundle(&consumer_dir)
        .expect("consumer bundle must load");

    // Resolve the consumer contract (test.consumer@1).
    let consumer_id: u64 = guest_contract_id("test.consumer", 1);
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
        "Lua guests use VM dispatch"
    );

    // Dispatch the consumer's fn 0 (invoke) with input "hello".
    let input: &[u8] = b"hello";
    let input_view: StringView = StringView {
        ptr: input.as_ptr(),
        len: input.len(),
    };
    let mut out_view: StringView = StringView::null();

    // SAFETY: dispatch_type is VirtualMachine so the vm union is active;
    // args is a *const StringView, out is a *mut StringView (invoke's ABI);
    // null arena falls back to host->alloc for cross-bundle StringView returns.
    let mut err: AbiError = AbiError::ok();
    unsafe {
        (vtable.dispatch.vm.call)(
            vtable.dispatch.vm.loader_data,
            GuestContractInstance::null(),
            0_u32,
            &input_view as *const StringView as *const (),
            &mut out_view as *mut StringView as *mut (),
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

    assert!(
        !out_view.ptr.is_null(),
        "returned StringView must not be null"
    );
    // SAFETY: out_view.ptr is valid for out_view.len bytes; the bytes were
    // written by the provider guest into host-allocated memory (null-arena
    // fallback to host->alloc) that stays valid until the runtime frees it
    // on bundle unload — which has not happened yet.
    let result_bytes: &[u8] = unsafe { core::slice::from_raw_parts(out_view.ptr, out_view.len) };
    let result: &str = core::str::from_utf8(result_bytes).expect("result is UTF-8");

    assert_eq!(
        result, "PEER:hello",
        "invoke must return 'PEER:hello' proving the peer-caller StringView round-trip"
    );
}
