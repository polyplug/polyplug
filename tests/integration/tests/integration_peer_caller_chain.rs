//! Integration tests: guest→guest peer-caller chains and error paths.
//!
//! Complements the per-language happy-path peer tests (#72) by exercising the
//! routing edges of `host->call_guest_method`:
//!   1. **Multi-hop chain A→B→C** — A peer-calls B, which peer-calls C; proves
//!      the host-mediated path composes (a guest is simultaneously a callee of
//!      one bundle and the caller of another). Asserts `A:B:C:hi`.
//!   2. **Peer not loaded** — the target contract was never registered;
//!      `resolve_guest_contract` returns null and the guest degrades gracefully.
//!   3. **Version too new** — the provider is major 1 but the caller requires
//!      `min_version = 2`; `find_guest_contract` filters it out (NotFound), so
//!      resolve returns null. Proves the min_version floor is enforced on the
//!      peer path.
//!
//! All guests are inline Lua (VM dispatch), so no external toolchain is needed.

#![allow(clippy::expect_used)]
#![allow(clippy::undocumented_unsafe_blocks)]

use polyplug::runtime::Runtime;
use polyplug_abi::AbiError;
use polyplug_abi::AbiErrorCode;
use polyplug_abi::GuestContractHandle;
use polyplug_abi::GuestContractInstance;
use polyplug_abi::GuestContractInterface;
use polyplug_abi::StringView;
use polyplug_lua::LuaConfig;
use polyplug_lua::LuaLoader;
use polyplug_utils::bundle_id;
use polyplug_utils::guest_contract_id;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

// ─── Bundle writing ──────────────────────────────────────────────────────────────

/// Write a `manifest.toml` + Lua script into a fresh subdir of `tmp` and copy the
/// Lua SDK so `require("polyplug_guest")` resolves. Returns the bundle dir.
fn write_lua_bundle(tmp: &Path, name: &str, contract: &str, src: &str) -> PathBuf {
    let dir: PathBuf = tmp.join(name);
    std::fs::create_dir_all(&dir).expect("create bundle dir");

    let id_val: u64 = bundle_id(name);
    let manifest: String = format!(
        "name = \"{name}\"\n\
         id = {id_val}\n\
         bundle_name = \"{name}\"\n\
         version = \"1.0.0\"\n\
         runtime = \"lua\"\n\
         file = \"plugin.lua\"\n\
         provides = [\"{contract}@1\"]\n\
         needs_reinit_on_dep_reload = false\n\n\
         [function_count]\n\
         \"{contract}@1\" = 1\n",
    );
    std::fs::write(dir.join("manifest.toml"), manifest).expect("write manifest.toml");
    std::fs::write(dir.join("plugin.lua"), src).expect("write plugin.lua");

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

// ─── Lua sources ─────────────────────────────────────────────────────────────────

/// A provider whose fn 0 returns `<prefix>` + input.
fn provider_src(contract: &str, prefix: &str) -> String {
    format!(
        r#"
local ffi = require("ffi")
local polyplug_guest = require("polyplug_guest")
local polyplug_abi = require("polyplug_abi")

local function impl_echo(args_ptr, out_ptr)
    local in_sv = ffi.cast("const StringView*", ffi.cast("uintptr_t", args_ptr))
    local s = polyplug_abi.to_str(in_sv[0])
    local out_view = polyplug_guest.alloc_string_arena("{prefix}" .. s)
    local out_sv = ffi.cast("StringView*", ffi.cast("uintptr_t", out_ptr))
    out_sv[0] = out_view
end

function polyplug_init(registrar_ptr, ctx_ptr)
    polyplug_guest.store_host_interface(registrar_ptr)
    _G._polyplug_handlers = {{
        ["{contract}"] = {{
            contract_version = 1,
            plugin_name = "{contract}-provider",
            functions = {{ [0] = impl_echo }},
        }},
    }}
end
"#
    )
}

/// A consumer whose fn 0 peer-calls `peer_id` (at `min_version`), forwarding its
/// own input, and returns `<prefix>` + the peer result — or `<prefix>NO-PEER` if
/// the peer cannot be resolved, `<prefix>ERR` if the call itself fails.
fn consumer_src(contract: &str, prefix: &str, peer_id: u64, min_version: u32) -> String {
    format!(
        r#"
local ffi = require("ffi")
local polyplug_guest = require("polyplug_guest")
local polyplug_abi = require("polyplug_abi")

local PEER_ID = 0x{peer_id:016X}ULL

local function impl_call(args_ptr, out_ptr)
    local out_sv = ffi.cast("StringView*", ffi.cast("uintptr_t", out_ptr))
    local host_ptr = polyplug_guest.get_host_interface()
    if host_ptr == nil then
        out_sv[0] = polyplug_guest.alloc_string_arena("{prefix}NO-HOST")
        return
    end
    local host = ffi.cast("HostApi*", ffi.cast("uintptr_t", host_ptr))
    local handle = host.find_guest_contract(host, PEER_ID, {min_version})
    local interface = host.resolve_guest_contract(host, handle)
    if interface == nil then
        out_sv[0] = polyplug_guest.alloc_string_arena("{prefix}NO-PEER")
        return
    end
    local instance = interface.create_instance(host, nil)
    instance.contract_id = PEER_ID
    local in_ptr = ffi.cast("const void*", ffi.cast("uintptr_t", args_ptr))
    local peer_out = ffi.new("StringView[1]")
    local err = host.call_guest_method(host, instance, 0, in_ptr, ffi.cast("void*", peer_out), nil)
    interface.destroy_instance(host, instance)
    if err.code ~= 0 then
        out_sv[0] = polyplug_guest.alloc_string_arena("{prefix}ERR")
        return
    end
    local peer_result = polyplug_abi.to_str(peer_out[0])
    out_sv[0] = polyplug_guest.alloc_string_arena("{prefix}" .. peer_result)
end

function polyplug_init(registrar_ptr, ctx_ptr)
    polyplug_guest.store_host_interface(registrar_ptr)
    _G._polyplug_handlers = {{
        ["{contract}"] = {{
            contract_version = 1,
            plugin_name = "{contract}-consumer",
            functions = {{ [0] = impl_call }},
        }},
    }}
end
"#
    )
}

// ─── Dispatch helper ─────────────────────────────────────────────────────────────

/// Dispatch fn 0 of `contract` with `input` and return the result string.
fn dispatch(rt: &Runtime, contract: &str, input: &[u8]) -> String {
    let id: u64 = guest_contract_id(contract, 1);
    let handle: GuestContractHandle = rt
        .find_guest_contract(id, 0)
        .unwrap_or_else(|_| panic!("{contract} must be registered"));
    let vtable_ptr: *const GuestContractInterface = rt
        .resolve_guest_contract(handle)
        .expect("handle must resolve");
    // SAFETY: vtable_ptr is live for the runtime lifetime.
    let vtable: &GuestContractInterface = unsafe { &*vtable_ptr };

    let input_view: StringView = StringView {
        ptr: input.as_ptr(),
        len: input.len(),
    };
    let mut out_view: StringView = StringView::null();
    // SAFETY: VM dispatch; args is a *const StringView, out a *mut StringView;
    // null arena selects the host->alloc fallback for the return.
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
        "{contract} dispatch must return Ok; got code={}",
        err.code
    );
    assert!(
        !out_view.ptr.is_null(),
        "{contract} returned a null StringView"
    );
    // SAFETY: out_view points to out_view.len valid UTF-8 bytes.
    let bytes: &[u8] = unsafe { core::slice::from_raw_parts(out_view.ptr, out_view.len) };
    core::str::from_utf8(bytes)
        .expect("result is UTF-8")
        .to_owned()
}

fn build_runtime() -> Arc<Runtime> {
    Runtime::builder()
        .loader(LuaLoader::new(LuaConfig::default()))
        .build()
        .expect("build runtime")
}

// ─── Tests ───────────────────────────────────────────────────────────────────────

#[test]
fn lua_peer_chain_three_hops() {
    let tmp: tempfile::TempDir = tempfile::tempdir().expect("tempdir");

    let c_id: u64 = guest_contract_id("test.peerc", 1);
    let b_id: u64 = guest_contract_id("test.peerb", 1);

    // C provides; B peer-calls C; A peer-calls B.
    let c_dir: PathBuf = write_lua_bundle(
        tmp.path(),
        "chain_c",
        "test.peerc",
        &provider_src("test.peerc", "C:"),
    );
    let b_dir: PathBuf = write_lua_bundle(
        tmp.path(),
        "chain_b",
        "test.peerb",
        &consumer_src("test.peerb", "B:", c_id, 1),
    );
    let a_dir: PathBuf = write_lua_bundle(
        tmp.path(),
        "chain_a",
        "test.peera",
        &consumer_src("test.peera", "A:", b_id, 1),
    );

    let rt: Arc<Runtime> = build_runtime();
    // Load providers before consumers so each peer resolves at load time.
    rt.load_bundle(&c_dir).expect("C must load");
    rt.load_bundle(&b_dir).expect("B must load");
    rt.load_bundle(&a_dir).expect("A must load");

    let result: String = dispatch(&rt, "test.peera", b"hi");
    assert_eq!(
        result, "A:B:C:hi",
        "A→B→C chain must compose through call_guest_method twice"
    );
}

#[test]
fn lua_peer_resolve_returns_no_peer_when_absent() {
    let tmp: tempfile::TempDir = tempfile::tempdir().expect("tempdir");

    // Consumer targets test.peerc@1, which is NEVER loaded.
    let absent_peer_id: u64 = guest_contract_id("test.peerc", 1);
    let consumer_dir: PathBuf = write_lua_bundle(
        tmp.path(),
        "absent_consumer",
        "test.absent",
        &consumer_src("test.absent", "OUT:", absent_peer_id, 1),
    );

    let rt: Arc<Runtime> = build_runtime();
    rt.load_bundle(&consumer_dir).expect("consumer must load");

    let result: String = dispatch(&rt, "test.absent", b"hi");
    assert_eq!(
        result, "OUT:NO-PEER",
        "resolving an unloaded peer must degrade to NO-PEER, not crash"
    );
}

#[test]
fn lua_peer_resolve_returns_no_peer_on_version_too_new() {
    let tmp: tempfile::TempDir = tempfile::tempdir().expect("tempdir");

    // Provider registers test.peerc at major 1; consumer requires min_version 2.
    let peer_id: u64 = guest_contract_id("test.peerc", 1);
    let provider_dir: PathBuf = write_lua_bundle(
        tmp.path(),
        "vm_provider",
        "test.peerc",
        &provider_src("test.peerc", "C:"),
    );
    let consumer_dir: PathBuf = write_lua_bundle(
        tmp.path(),
        "vm_consumer",
        "test.vmismatch",
        &consumer_src("test.vmismatch", "OUT:", peer_id, 2),
    );

    let rt: Arc<Runtime> = build_runtime();
    rt.load_bundle(&provider_dir).expect("provider must load");
    rt.load_bundle(&consumer_dir).expect("consumer must load");

    let result: String = dispatch(&rt, "test.vmismatch", b"hi");
    assert_eq!(
        result, "OUT:NO-PEER",
        "a v1 provider must not satisfy a min_version=2 peer request"
    );
}
