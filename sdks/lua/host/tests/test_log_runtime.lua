-- sdks/lua/host/tests/test_log_runtime.lua
-- REAL-runtime custom-logger test for Runtime.new{ log = ... }.
--
-- LuaJIT FFI callbacks cannot receive structs by value, so a Lua host cannot
-- install RuntimeConfig.log (by-value StringViews) directly; the SDK routes
-- through the polyplug_lua loader cdylib's exported
-- `polyplug_lua_log_trampoline` and a PolyplugLuaLogBridge carried in
-- log_user_data. This test drives the WHOLE delivery chain on a real runtime:
--
--   lua guest VM (_polyplug_log) -> loader log bridge -> runtime logging
--   funnel (LoggerHandle) -> RuntimeConfig.log = polyplug_lua_log_trampoline
--   -> PolyplugLuaLogBridge -> LuaJIT scalar callback -> user Lua function
--
-- A minimal staged lua guest bundle logs one Warn record at polyplug_init
-- time; the test asserts the host's Lua callback observed the real level +
-- scope + message delivered across the C ABI.
--
-- (Runtime-ORIGINATED Warn diagnostics — function_count mismatch, version
-- mismatch, missing function_count entry — are all gated on
-- Compatibility::Relaxed, and `Runtime::load_bundle` hardcodes
-- `Compatibility::default()` (Strict) instead of the configured
-- RuntimeConfig.compatibility, so none of them are reachable through the FFI
-- load path today; the guest funnel record is the deterministic real
-- diagnostic.)
--
-- Skip-honestly policy (matches test_reload_runtime.lua): when POLYPLUG_LIB is
-- absent the test FAILS LOUDLY with instructions — a runtime test that
-- silently passes hides exactly the never-run breakage class it exists to
-- catch.
--
-- Run from repo root:
--   cargo build --release -p polyplug -p polyplug_lua
--   POLYPLUG_LIB=$PWD/target/release/libpolyplug.so \
--   POLYPLUG_LUA_LIB=$PWD/target/release/libpolyplug_lua.so \
--   luajit sdks/lua/host/tests/test_log_runtime.lua
--
-- Optional log-path micro-benchmark (delivery cost per log line):
--   POLYPLUG_BENCH_ITERS=1000000 ... luajit sdks/lua/host/tests/test_log_runtime.lua

-- ─── Path setup ──────────────────────────────────────────────────────────────
-- The working directory may be anywhere; resolve everything from this script's
-- own directory (sdks/lua/host/tests/).
local script_dir = debug.getinfo(1, "S").source:match("^@(.+/)[^/]+$") or "./"
package.path = script_dir .. "../?.lua;"
           .. script_dir .. "../../abi/?.lua;"
           .. script_dir .. "../../loaders/lua/?.lua;"
           .. package.path

-- ─── Skip-honestly: a runtime test must never silently pass ──────────────────
local polyplug_lib = os.getenv("POLYPLUG_LIB")
if not polyplug_lib or polyplug_lib == "" then
    io.stderr:write(
        "FATAL: POLYPLUG_LIB not set — this runtime test must not silently pass.\n"
        .. "Build the core and point the test at it:\n"
        .. "  cargo build --release -p polyplug -p polyplug_lua\n"
        .. "  export POLYPLUG_LIB=$PWD/target/release/libpolyplug.so\n"
        .. "  export POLYPLUG_LUA_LIB=$PWD/target/release/libpolyplug_lua.so\n")
    os.exit(1)
end

local ffi = require("ffi")
local abi = require("polyplug_abi")
local polyplug = require("polyplug.runtime")
local lua_loader = require("polyplug.loaders.lua")

-- ─── Fixture: minimal lua guest bundle that logs at init time ────────────────
-- The polyplug_lua loader injects `_polyplug_log(level, scope, message)` into
-- every plugin VM; it delivers straight into the runtime's logging funnel
-- (the same sink as every runtime diagnostic). The staged plugin emits one
-- Warn record during polyplug_init, then registers a no-op contract.
local bundle_name = "log_test_plugin"
-- manifest.validate enforces id == FNV1a-64(name).
local bundle_id_str = tostring(abi.bundle_id(bundle_name)):gsub("ULL$", "")

local tmp_dir = os.tmpname()
os.remove(tmp_dir)
os.execute("mkdir -p '" .. tmp_dir .. "'")

local manifest_out = assert(io.open(tmp_dir .. "/manifest.toml", "w"))
manifest_out:write(string.format([=[
id = %s
name = "%s"
version = "1.0"
runtime = "lua"
needs_reinit_on_dep_reload = false
provides = ["log.test@1"]
file = "bundle.lua"

[function_count]
"log.test@1" = 1
]=], bundle_id_str, bundle_name))
manifest_out:close()

local bundle_out = assert(io.open(tmp_dir .. "/bundle.lua", "w"))
bundle_out:write([=[
local function make_log_test(_host) return {} end
local function impl_noop(_instance, _args, _out) end
function polyplug_init(_host, _ctx)
    -- Warn (2): passes the default log_max_level so the host callback sees it.
    _polyplug_log(2, "guest.log_test", "lua guest warn via funnel")
    _G._polyplug_handlers = {
        ["log.test"] = {
            contract_version = 1,
            plugin_name      = "log-test",
            factory          = make_log_test,
            functions        = { [0] = impl_noop },
        },
    }
end
]=])
bundle_out:close()

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

-- ─── Custom logger receives a real funnel diagnostic ─────────────────────────
print("=== opts.log receives real runtime diagnostics via the loader trampoline ===")

polyplug.load_lib(polyplug_lib)

local records = {}
local rt = polyplug.Runtime.new({
    log = function(level, scope, message)
        records[#records + 1] = { level = level, scope = scope, message = message }
    end,
    -- Warn is also the default; passed explicitly so the test pins the knob.
    log_max_level = polyplug.LogLevel.Warn,
})

check(rt._log_bridge ~= nil, "runtime instance anchors the log bridge cdata")
check(rt._log_cb_cdata ~= nil, "runtime instance anchors the log callback cdata")

lua_loader.register(rt)
check(#records == 0, "no diagnostics before load_bundle (got " .. #records .. ")")

rt:load_bundle(tmp_dir)

check(#records >= 1, "at least one diagnostic delivered during load (got " .. #records .. ")")

local guest_warn = nil
for _, rec in ipairs(records) do
    if rec.scope == "guest.log_test" then
        guest_warn = rec
    end
end

check(guest_warn ~= nil, "the guest funnel record was delivered to the host callback")
check(guest_warn ~= nil and guest_warn.level == polyplug.LogLevel.Warn,
    "record carries LogLevel.Warn (got "
    .. tostring(guest_warn and guest_warn.level) .. ")")
check(guest_warn ~= nil and type(guest_warn.scope) == "string"
        and type(guest_warn.message) == "string",
    "scope and message arrive as plain Lua strings")
check(guest_warn ~= nil and guest_warn.message == "lua guest warn via funnel",
    "message survives the trampoline byte-for-byte (got \""
    .. tostring(guest_warn and guest_warn.message) .. "\")")

-- ─── Rule 12: a second runtime without a logger must not feed this one ───────
print("\n=== per-instance isolation: second runtime has its own (default) sink ===")

local count_before = #records
local rt2 = polyplug.Runtime.new()
lua_loader.register(rt2)
-- The same staged bundle: its Warn goes to rt2's default stderr sink, NEVER
-- to rt's Lua callback.
rt2:load_bundle(tmp_dir)
check(#records == count_before,
    "rt2 diagnostics did not leak into rt's logger (got " .. (#records - count_before)
    .. " extra records)")
rt2:destroy()

-- ─── Optional log-path micro-benchmark (POLYPLUG_BENCH_ITERS) ────────────────
-- Measures the DELIVERY cost of one log line through the full Lua-side path:
-- native trampoline call -> bridge read -> LuaJIT scalar callback ->
-- ffi.string copies -> user Lua function. This is the per-delivered-line
-- overhead a custom Lua logger adds; unlogged calls (level filtered by
-- log_max_level) never reach this path and stay zero-cost.
local bench_iters = os.getenv("POLYPLUG_BENCH_ITERS")
if bench_iters then
    local n = tonumber(bench_iters)
    if n and n > 0 then
        local lib = lua_loader.bridge_lib()
        local consumed = 0
        local cb = ffi.cast("PolyplugLuaLogCallbackFn", function(
            _user_data, level, scope_ptr, scope_len, msg_ptr, msg_len
        )
            local scope = ffi.string(scope_ptr, scope_len)
            local message = ffi.string(msg_ptr, msg_len)
            consumed = consumed + #scope + #message + tonumber(level)
        end)
        local bridge = ffi.new("PolyplugLuaLogBridge")
        bridge.callback = cb
        bridge.user_data = nil
        local bridge_ptr = ffi.cast("void*", bridge)
        local scope_str = "guest.log_test"
        local msg_str = "lua guest warn via funnel — representative log line"
        local scope_buf = ffi.new("uint8_t[?]", #scope_str, scope_str)
        local msg_buf = ffi.new("uint8_t[?]", #msg_str, msg_str)
        local scope_sv = ffi.new("StringView")
        scope_sv.ptr = scope_buf
        scope_sv.len = #scope_str
        local msg_sv = ffi.new("StringView")
        msg_sv.ptr = msg_buf
        msg_sv.len = #msg_str
        local warmup = math.min(n, 10000)
        for _ = 1, warmup do
            lib.polyplug_lua_log_trampoline(bridge_ptr, 2, scope_sv, msg_sv)
        end
        consumed = 0
        local t0 = os.clock()
        for _ = 1, n do
            lib.polyplug_lua_log_trampoline(bridge_ptr, 2, scope_sv, msg_sv)
        end
        local t1 = os.clock()
        local expected = n * (#scope_str + #msg_str + 2)
        if consumed == expected then
            print(string.format("LOGPATH_NS=%.2f LANG=lua", (t1 - t0) * 1e9 / n))
        else
            io.stderr:write(string.format(
                "LOGPATH bench: callback consumed %d, expected %d — no result printed\n",
                consumed, expected))
        end
        cb:free()
    end
end

rt:destroy()
os.execute("rm -rf '" .. tmp_dir .. "'")

-- ─── Summary ─────────────────────────────────────────────────────────────────
print("")
print(string.format("Results: %d passed, %d failed", tests_passed, tests_failed))
if tests_failed > 0 then
    os.exit(1)
end
