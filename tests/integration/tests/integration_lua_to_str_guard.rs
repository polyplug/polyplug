//! Integration test: the validated Lua `to_str` helper fails loudly on misuse.
//!
//! Regression guard for the A3 footgun: `to_str`/`strip_prefix`/`split` used to
//! silently return `""` when handed an already-converted Lua string instead of a
//! `StringView` cdata (a Lua string has no `.ptr` field). That silent failure hid
//! a real bug in three example guests (they returned `INVALID:*`). The validated
//! helper (`sdks/lua/abi/abi.lua`, per `checks/sdk_validator.yaml`) now raises a clear
//! error on non-cdata input.
//!
//! This test loads a Lua guest whose function `probe()` `pcall`s
//! `polyplug_abi.to_str("plain string")` and returns `"RAISED:<msg>"` if it
//! errored or `"SILENT:<value>"` if it did not — then asserts the result is
//! `RAISED:` and names a StringView, proving the loud-failure behavior.
//!
//! LuaJIT is always vendored, so no skip path is needed.

#![allow(clippy::expect_used)]
#![allow(clippy::undocumented_unsafe_blocks)]

use polyplug::runtime::Runtime;
use polyplug_abi::AbiError;
use polyplug_abi::AbiErrorCode;
use polyplug_abi::GuestContractHandle;
use polyplug_abi::GuestContractInstance;
use polyplug_abi::GuestContractInterface;
use polyplug_abi::StringView;

use polyplug_lua::LuaLoader;
use polyplug_utils::bundle_id;
use polyplug_utils::guest_contract_id;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

/// Lua source: `probe()` deliberately passes a plain Lua string to `to_str`
/// (the double-conversion misuse) under `pcall`, and reports whether it raised.
fn probe_lua_src() -> &'static str {
    r#"
local ffi = require("ffi")
local polyplug_guest = require("polyplug_guest")
local polyplug_abi = require("polyplug_abi")

local function impl_probe(instance, args_ptr, out_ptr, arena_ptr, arena_alloc)
    -- Misuse on purpose: a Lua string is NOT a StringView cdata.
    local ok, err = pcall(polyplug_abi.to_str, "already a Lua string")
    local result
    if ok then
        result = "SILENT:" .. tostring(err)
    else
        result = "RAISED:" .. tostring(err)
    end
    local out_view = polyplug_guest.alloc_string_arena(arena_alloc, arena_ptr, result)
    local out_sv = ffi.cast("StringView*", ffi.cast("uintptr_t", out_ptr))
    out_sv[0] = out_view
end

function polyplug_init(registrar_ptr, ctx_ptr)
    return {
        ["test.probe"] = {
            contract_version = 1,
            plugin_name = "to-str-guard-probe",
            factory = function(_host) return {} end,
            functions = { [0] = impl_probe },
        },
    }, { code = polyplug_guest.AbiErrorCode.Ok }
end
"#
}

fn build_probe_bundle(tmp: &Path) -> PathBuf {
    let dir: PathBuf = tmp.join("to_str_guard_probe");
    std::fs::create_dir_all(&dir).expect("create probe dir");

    let id_val: u64 = bundle_id("to_str_guard_probe");
    let manifest: String = format!(
        "name = \"to_str_guard_probe\"\n\
         id = {id_val}\n\
         bundle_name = \"to_str_guard_probe\"\n\
         version = \"1.0.0\"\n\
         loader = \"lua\"\n\
         file = \"probe.lua\"\n\
         provides = [\"test.probe@1\"]\n\
         needs_reinit_on_dep_reload = false\n\n\
         [function_count]\n\
         \"test.probe@1\" = 1\n",
    );
    std::fs::write(dir.join("manifest.toml"), manifest).expect("write manifest.toml");
    std::fs::write(dir.join("probe.lua"), probe_lua_src()).expect("write probe.lua");

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
    // Vendor the CURRENT guest SDK (alloc_string / alloc_string_arena / log,
    // all host-threaded — no module globals); the hardened to_str itself lives
    // in the vendored abi module above.
    std::fs::copy(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root")
            .join("sdks")
            .join("lua")
            .join("guest")
            .join("polyplug_guest.lua"),
        dir.join("polyplug_guest.lua"),
    )
    .expect("vendor current polyplug_guest.lua");

    dir
}

#[test]
fn lua_to_str_raises_on_plain_string() {
    let tmp: tempfile::TempDir = tempfile::tempdir().expect("tempdir");
    let bundle_dir: PathBuf = build_probe_bundle(tmp.path());

    let rt: Arc<Runtime> = Runtime::builder()
        .loader(LuaLoader::new())
        .build()
        .expect("build runtime");
    rt.load_bundle(&bundle_dir).expect("probe bundle must load");

    let contract_id: u64 = guest_contract_id("test.probe", 1);
    let handle: GuestContractHandle = rt
        .find_guest_contract(contract_id, 0)
        .expect("test.probe must be registered after load");
    let vtable_ptr: *const GuestContractInterface = rt
        .resolve_guest_contract(handle)
        .expect("handle must resolve");
    // SAFETY: vtable_ptr is live for the runtime lifetime.
    let vtable: &GuestContractInterface = unsafe { &*vtable_ptr };

    let mut out_view: StringView = StringView::null();
    // SAFETY: VM dispatch; probe() reads no input so a null args pointer is fine;
    // out is a *mut StringView; null arena selects the host->alloc fallback.
    let mut err: AbiError = AbiError::ok();
    unsafe {
        (vtable.dispatch.vm.call)(
            vtable.adapter_context,
            vtable.dispatch.vm.loader_data,
            GuestContractInstance::null(),
            0_u32,
            core::ptr::null::<()>(),
            &mut out_view as *mut StringView as *mut (),
            core::ptr::null_mut(),
            &mut err as *mut AbiError,
        )
    };
    assert_eq!(
        err.code,
        AbiErrorCode::Ok as u32,
        "probe() itself must return Ok (it pcall-catches the error internally)"
    );
    assert!(
        !out_view.ptr.is_null(),
        "probe() must return a non-null StringView"
    );

    // SAFETY: out_view points to valid UTF-8 bytes for out_view.len bytes.
    let bytes: &[u8] = unsafe { core::slice::from_raw_parts(out_view.ptr, out_view.len) };
    let result: &str = core::str::from_utf8(bytes).expect("result is UTF-8");

    assert!(
        result.starts_with("RAISED:"),
        "to_str(plain string) must raise, not silently return \"\"; got: {result}"
    );
    assert!(
        result.contains("StringView"),
        "the error must name StringView so the misuse is obvious; got: {result}"
    );
}
