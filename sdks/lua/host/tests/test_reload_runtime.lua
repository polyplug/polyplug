-- sdks/lua/host/tests/test_reload_runtime.lua
-- REAL-runtime hot-reload notification test (mirrors sdks/js/host/tests/reload_runtime_test.ts).
--
-- `test_reload_notification.lua` covers the SDK-side ReloadPhase type only —
-- it builds local tables and asserts on them, which can never catch a broken
-- FFI path. This test drives the actual flow: create a runtime through the
-- Lua host SDK with an on_reload callback (a real LuaJIT FFI callback for the
-- `void(*)(void*, const ReloadPhase*)` ABI signature), load the native reload
-- fixture bundle, trigger a reload through the runtime, and assert the
-- callback fired with REAL phase data delivered across the C ABI.
--
-- Skip-honestly policy (matches sdks/js/host/tests/reload_runtime_test.ts):
-- when POLYPLUG_LIB is absent the test FAILS LOUDLY with instructions — a
-- runtime test that silently passes hides exactly the never-run breakage
-- class it exists to catch.
--
-- Run from repo root:
--   cargo build --release -p polyplug -p polyplug_native
--   bash tests/fixtures/build_all.sh
--   POLYPLUG_LIB=$PWD/target/release/libpolyplug.so \
--   POLYPLUG_NATIVE_LIB=$PWD/target/release/libpolyplug_native.so \
--   luajit sdks/lua/host/tests/test_reload_runtime.lua

-- ─── Path setup ──────────────────────────────────────────────────────────────
-- The working directory may be anywhere; resolve everything from this script's
-- own directory (sdks/lua/host/tests/).
local script_dir = debug.getinfo(1, "S").source:match("^@(.+/)[^/]+$") or "./"
package.path = script_dir .. "../?.lua;"
           .. script_dir .. "../../abi/?.lua;"
           .. script_dir .. "../../loaders/native/?.lua;"
           .. package.path

-- ─── Skip-honestly: a runtime test must never silently pass ──────────────────
local polyplug_lib = os.getenv("POLYPLUG_LIB")
if not polyplug_lib or polyplug_lib == "" then
    io.stderr:write(
        "FATAL: POLYPLUG_LIB not set — this runtime test must not silently pass.\n"
        .. "Build the core and point the test at it:\n"
        .. "  cargo build --release -p polyplug -p polyplug_native\n"
        .. "  export POLYPLUG_LIB=$PWD/target/release/libpolyplug.so\n"
        .. "  export POLYPLUG_NATIVE_LIB=$PWD/target/release/libpolyplug_native.so\n")
    os.exit(1)
end

local fixtures_dir = script_dir .. "../../../../tests/fixtures"
local v1_dir = fixtures_dir .. "/reload_plugin_v1"
-- The reload target is the v2 .so INSIDE its bundle dir — the runtime reads
-- the sibling manifest.toml during reload (mirrors integration_reload.rs).
local v2_so = fixtures_dir .. "/reload_plugin_v2/libreload_plugin_v2.so"

local function require_fixture(path)
    local f = io.open(path, "r")
    if not f then
        error("reload fixture missing: " .. path
            .. " — run `bash tests/fixtures/build_all.sh` first")
    end
    f:close()
end

require_fixture(v1_dir .. "/manifest.toml")
require_fixture(v1_dir .. "/libreload_plugin_v1.so")
require_fixture(v2_so)

-- ─── Load modules under test ─────────────────────────────────────────────────
local abi = require("polyplug_abi")
local polyplug = require("polyplug")
local reload_phase = require("polyplug.reload_phase")
local native_loader = require("polyplug.loaders.native")

-- name from tests/fixtures/reload_plugin_v1/manifest.toml; the bundle id is
-- FNV-1a 64 of the name (TRUST_MODEL §2), compared as uint64 cdata.
local v1_bundle_id = abi.bundle_id("reload_plugin_v1")

-- ─── Test harness ────────────────────────────────────────────────────────────
local tests_passed = 0
local tests_failed = 0

local function check(ok, message)
    if ok then
        print("  PASS: " .. message)
        tests_passed = tests_passed + 1
    else
        print("  FAIL: " .. message)
        tests_failed = tests_failed + 1
    end
end

-- ─── on_reload fires with real phase data on a real runtime reload ──────────
print("=== on_reload fires with real phase data on a real runtime reload ===")

polyplug.load_lib(polyplug_lib)

local phases = {}
local rt = polyplug.Runtime.new({
    config = { hot_reload_enabled = true },
    on_reload = function(phase)
        phases[#phases + 1] = phase
    end,
})

native_loader.register(rt)
rt:load_bundle(v1_dir)
check(#phases == 0, "no reload phases before the reload")

rt:reload_bundle(v2_so)

check(#phases >= 2,
    "reload must deliver at least Preparing + Reloaded (got " .. #phases .. ")")

local first = phases[1]
check(first ~= nil and reload_phase.is_preparing(first),
    "first phase must be Preparing (got type "
    .. tostring(first and first.type) .. ")")
check(first ~= nil and first.bundle_id == v1_bundle_id,
    "Preparing phase must carry the real bundle id from the manifest (got "
    .. tostring(first and first.bundle_id) .. ", want " .. tostring(v1_bundle_id) .. ")")
check(first ~= nil and first.bundle_name == "reload_plugin_v1",
    "Preparing phase must carry the real bundle name (got "
    .. tostring(first and first.bundle_name) .. ")")
check(first ~= nil and first.reason == "",
    "non-Failed phase must carry the null-view reason as empty string (got "
    .. tostring(first and first.reason) .. ")")

local saw_reloaded = false
for _, phase in ipairs(phases) do
    if reload_phase.is_reloaded(phase) then
        saw_reloaded = true
    end
end
check(saw_reloaded, "a Reloaded phase must follow")

rt:destroy()

-- ─── Summary ─────────────────────────────────────────────────────────────────
print("")
print(string.format("Results: %d passed, %d failed", tests_passed, tests_failed))
if tests_failed > 0 then
    os.exit(1)
end
