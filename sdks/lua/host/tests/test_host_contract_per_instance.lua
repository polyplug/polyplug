-- sdks/lua/host/tests/test_host_contract_per_instance.lua
-- REAL per-instance state test for a LUA-host-provided host contract.
--
-- A LuaJIT host that PROVIDES a host contract registers it through the
-- generated interface_factories.lua. Each instance must own independent state:
-- the factory builds a FRESH impl per instance, keyed by an instance id stamped
-- into HostContractInstance.data by the Lua create_instance callback, and
-- dispatch routes to the per-instance impl by that id (null/id-0 -> a default
-- impl built once at registration). destroy_instance drops the impl.
--
-- This drives the generated factory's surface DIRECTLY (no full runtime needed
-- for the per-instance routing, which is entirely in the generated Lua + the
-- polyplug_lua loader trampolines):
--
--   interface.create_instance(this, args, out_hci)      -- Lua callback
--   interface.dispatch.vm.call(loader_data, instance,   -- native trampoline ->
--       fn_id, args, out, arena, out_err)               --   bridge.callback (Lua)
--   interface.destroy_instance(this, instance)          -- native trampoline ->
--                                                        --   bridge.destroy_callback (Lua)
--
-- The host contract is generated from a minimal inline api.toml (a no-arg
-- u32-returning `inc()` method on a `{ count = 0 }` impl) so the assertions are
-- a clean integer-independence check.
--
-- Skip-honestly policy (matches test_log_runtime.lua): when POLYPLUG_LIB /
-- POLYPLUG_LUA_LIB are absent the test FAILS LOUDLY with instructions — a
-- runtime test that silently passes hides the never-run breakage class it
-- exists to catch.
--
-- Run from repo root:
--   cargo build --release -p polyplug -p polyplug_lua -p polyplugc
--   POLYPLUG_LIB=$PWD/target/release/libpolyplug.so \
--   POLYPLUG_LUA_LIB=$PWD/target/release/libpolyplug_lua.so \
--   luajit sdks/lua/host/tests/test_host_contract_per_instance.lua
--
-- POLYPLUGC may point at the polyplugc binary; otherwise it is resolved from the
-- repo target/release directory relative to this script.

-- ─── Path setup ──────────────────────────────────────────────────────────────
local script_dir = debug.getinfo(1, "S").source:match("^@(.+/)[^/]+$") or "./"
-- Repo root is sdks/lua/host/tests/ -> up four.
local repo_root = script_dir .. "../../../../"

-- ─── Skip-honestly: a runtime test must never silently pass ──────────────────
local polyplug_lib = os.getenv("POLYPLUG_LIB")
if not polyplug_lib or polyplug_lib == "" then
    io.stderr:write(
        "FATAL: POLYPLUG_LIB not set — this runtime test must not silently pass.\n"
        .. "Build the core + lua loader and point the test at them:\n"
        .. "  cargo build --release -p polyplug -p polyplug_lua -p polyplugc\n"
        .. "  export POLYPLUG_LIB=$PWD/target/release/libpolyplug.so\n"
        .. "  export POLYPLUG_LUA_LIB=$PWD/target/release/libpolyplug_lua.so\n")
    os.exit(1)
end
if not os.getenv("POLYPLUG_LUA_LIB") or os.getenv("POLYPLUG_LUA_LIB") == "" then
    io.stderr:write(
        "FATAL: POLYPLUG_LUA_LIB not set — the host-contract bridge needs the\n"
        .. "lua loader cdylib (libpolyplug_lua.so) exporting the trampolines.\n"
        .. "  export POLYPLUG_LUA_LIB=$PWD/target/release/libpolyplug_lua.so\n")
    os.exit(1)
end

-- ─── Generate the host contract from a minimal inline api.toml ───────────────
local tmp_dir = os.tmpname()
os.remove(tmp_dir)
assert(os.execute("mkdir -p '" .. tmp_dir .. "'"), "mkdir tmp_dir failed")

local api_path = tmp_dir .. "/api.toml"
local api_out = assert(io.open(api_path, "w"))
api_out:write([=[
[[host_contract]]
name = "host.counter"
version = "1.0.0"

[[host_contract.functions]]
name = "inc"
params = []
return = "u32"
]=])
api_out:close()

-- Resolve the polyplugc binary.
local polyplugc = os.getenv("POLYPLUGC")
if not polyplugc or polyplugc == "" then
    polyplugc = repo_root .. "target/release/polyplugc"
end

local gen_cmd = string.format(
    "'%s' generate --api '%s' --lang lua --out '%s' 2>&1",
    polyplugc, api_path, tmp_dir)
local gen_pipe = assert(io.popen(gen_cmd, "r"))
local gen_output = gen_pipe:read("*a")
local gen_ok = gen_pipe:close()
-- LuaJIT's popen:close() does not reliably report the child exit status, so
-- also require the success marker and the generated file to actually exist.
local generated_file = tmp_dir .. "/host/interface_factories.lua"
local exists = io.open(generated_file, "r")
if exists then exists:close() end
if not gen_ok or not gen_output:find("generated", 1, true) or not exists then
    io.stderr:write("FATAL: polyplugc generate failed:\n" .. tostring(gen_output) .. "\n")
    os.execute("rm -rf '" .. tmp_dir .. "'")
    os.exit(1)
end

-- ─── Module path: abi + lua loader + the freshly generated host code ─────────
package.path = script_dir .. "../?.lua;"
           .. script_dir .. "../../abi/?.lua;"
           .. script_dir .. "../../loaders/lua/?.lua;"
           .. tmp_dir .. "/host/?.lua;"
           .. package.path

local ffi = require("ffi")
require("polyplug_abi")  -- installs the ABI cdefs (HostContractInterface, ...)
local lua_loader = require("polyplug.loaders.lua")
local interface_factories = require("interface_factories")

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

-- ─── Build a NON-singleton host contract interface ───────────────────────────
-- factory() builds a fresh impl each call: { count = 0 } with inc() returning
-- the incremented count. Non-singleton => the runtime would create one instance
-- per caller; here we drive create_instance directly to prove the routing.
print("=== lua host contract: per-instance state is independent ===")

local function factory()
    return {
        count = 0,
        inc = function(self)
            self.count = self.count + 1
            return self.count
        end,
    }
end

local bridge_lib = lua_loader.bridge_lib()
local interface = interface_factories.create_host_counter_interface(factory, bridge_lib)
check(interface ~= nil, "factory returned a HostContractInterface")
check(interface.singleton == 0, "contract is non-singleton (multi-instance)")

-- Helper: create one instance, returning its HostContractInstance.
local function create_instance()
    local hci = ffi.new("HostContractInstance[1]")
    interface.create_instance(interface, nil, hci)
    return hci[0]
end

-- Helper: dispatch inc() (fn_id 0, no args) on an instance id, returning the
-- u32 result written through the out-pointer. The dispatch ABI carries a
-- GuestContractInstance; we forward the instance id through its `data` field
-- (the host-side per-instance routing keys only on the pointer-as-id).
local INC_FN_ID = 0
local function dispatch_inc(instance_data)
    local gci = ffi.new("GuestContractInstance")
    gci.data = instance_data
    local out = ffi.new("uint32_t[1]")
    local err = ffi.new("AbiError[1]")
    interface.dispatch.vm.call(
        interface.dispatch.vm.loader_data,
        gci,
        INC_FN_ID,
        nil,        -- inc() takes no args
        out,
        nil,        -- no arena
        err)
    return tonumber(out[0]), err[0]
end

-- Two independent instances.
local inst_a = create_instance()
local inst_b = create_instance()
check(inst_a.data ~= nil, "instance A has a non-null id handle")
check(inst_b.data ~= nil, "instance B has a non-null id handle")
check(tonumber(ffi.cast("uintptr_t", inst_a.data))
        ~= tonumber(ffi.cast("uintptr_t", inst_b.data)),
    "instance A and B have DISTINCT id handles")

-- A counts 1,2,3 while B is untouched, then B counts 1 — independent state.
local a1 = dispatch_inc(inst_a.data)
local a2 = dispatch_inc(inst_a.data)
local a3 = dispatch_inc(inst_a.data)
check(a1 == 1 and a2 == 2 and a3 == 3,
    "instance A counts 1,2,3 (got " .. a1 .. "," .. a2 .. "," .. a3 .. ")")

local b1 = dispatch_inc(inst_b.data)
check(b1 == 1, "instance B is independent — its first inc() is 1 (got " .. b1 .. ")")

local a4 = dispatch_inc(inst_a.data)
check(a4 == 4, "instance A keeps its own running count — inc() is 4 (got " .. a4 .. ")")

-- ─── Null instance routes to the default impl, also independent ──────────────
print("\n=== null instance -> default impl (own state) ===")
local null_data = ffi.cast("void*", 0)  -- null instance handle
local d1 = dispatch_inc(null_data)
local d2 = dispatch_inc(null_data)
check(d1 == 1 and d2 == 2,
    "default impl has its own running count 1,2 (got " .. d1 .. "," .. d2 .. ")")
-- The default impl must not have been disturbed by A/B above.
check(d2 == 2, "default impl is independent of instances A and B")

-- ─── destroy_instance drops the per-instance impl ────────────────────────────
print("\n=== destroy_instance removes the instance (dispatch then fails) ===")
interface.destroy_instance(interface, inst_a)
interface.destroy_instance(interface, inst_b)

-- A subsequent dispatch on a destroyed id must no longer find the impl: the
-- generated dispatcher returns FunctionNotAvailable (6) via out_err.code.
local FUNCTION_NOT_AVAILABLE = 6
local _, err_after = dispatch_inc(inst_a.data)
check(err_after.code == FUNCTION_NOT_AVAILABLE,
    "dispatch on a destroyed instance returns FunctionNotAvailable (got "
    .. tonumber(err_after.code) .. ")")

local _, err_after_b = dispatch_inc(inst_b.data)
check(err_after_b.code == FUNCTION_NOT_AVAILABLE,
    "dispatch on the other destroyed instance returns FunctionNotAvailable (got "
    .. tonumber(err_after_b.code) .. ")")

-- The default impl survives instance destruction.
local d3 = dispatch_inc(null_data)
check(d3 == 3, "default impl still works after instances destroyed (got " .. d3 .. ")")

-- ─── Cleanup + summary ───────────────────────────────────────────────────────
os.execute("rm -rf '" .. tmp_dir .. "'")

print("")
print(string.format("Results: %d passed, %d failed", tests_passed, tests_failed))
if tests_failed > 0 then
    os.exit(1)
end
