-- Generated Lua in-process adapter integration test.
--
-- Run from the repository root:
--   cargo build -p polyplug -p polyplug_lua -p polyplugc
--   POLYPLUG_LIB=$PWD/target/debug/libpolyplug.so \
--   POLYPLUG_LUA_LIB=$PWD/target/debug/libpolyplug_lua.so \
--   POLYPLUGC_BIN=$PWD/target/debug/polyplugc \
--   luajit sdks/lua/host/tests/test_in_process_runtime.lua

local script_dir = debug.getinfo(1, "S").source:match("^@(.+/)[^/]+$") or "./"
local polyplugc = os.getenv("POLYPLUGC_BIN")
if not polyplugc or polyplugc == "" then
    io.stderr:write("FATAL: POLYPLUGC_BIN must name a built polyplugc binary.\n")
    os.exit(1)
end
if not os.getenv("POLYPLUG_LIB") or not os.getenv("POLYPLUG_LUA_LIB") then
    io.stderr:write("FATAL: POLYPLUG_LIB and POLYPLUG_LUA_LIB must name the built bridge libraries.\n")
    os.exit(1)
end

local function quote(value)
    return "'" .. value:gsub("'", "'\\''") .. "'"
end

local generated_root = os.tmpname()
os.remove(generated_root)
local generated_main = generated_root .. "/main"
local generated_dependent = generated_root .. "/dependent"
local function generate(api, output)
    local command = quote(polyplugc) .. " generate --api " .. quote(api)
        .. " --lang lua --out " .. quote(output)
    if os.execute(command) ~= 0 then
        error("polyplugc failed while generating " .. api, 2)
    end
end

generate(script_dir .. "in_process_api.toml", generated_main)
generate(script_dir .. "in_process_dependent_api.toml", generated_dependent)

package.path = generated_main .. "/host/?.lua;"
    .. script_dir .. "../?.lua;"
    .. script_dir .. "../../abi/?.lua;"
    .. script_dir .. "../../loaders/lua/?.lua;"
    .. package.path

local polyplug = require("polyplug")
local lua_loader = require("polyplug.loaders.lua")
local callers = require("callers")
local dependent_callers = dofile(generated_dependent .. "/host/callers.lua")

local function check(ok, message)
    if not ok then
        error(message, 2)
    end
end

local function counter_factory(seed)
    return function(_host)
        local value = seed
        return {
            increment = function(self)
                local _ = self
                value = value + 1
                return value
            end,
            value = function(self)
                local _ = self
                return value
            end,
        }
    end
end

local function marker_factory(value)
    return function(_host)
        return {
            value = function(self)
                local _ = self
                return value
            end,
        }
    end
end

local function bundle(name, seed, dependencies)
    return callers.in_process_bundle({
        name = name,
        dependencies = dependencies,
        implementations = {
            ["state.counter"] = counter_factory(seed),
            ["state.marker"] = marker_factory(seed * 10),
            ["state.dependent"] = marker_factory(seed * 100),
        },
    }, lua_loader.bridge_lib())
end

local first = polyplug.Runtime.new()
local second = polyplug.Runtime.new()

local rejected = bundle("lua-in-process-rejected", 1)
rejected.registration.contracts = nil
check(not pcall(function() first:register_in_process_bundle(rejected) end),
    "invalid complete registration must reject atomically")
check(first:resolve_guest_contract(first:find_guest_contract(callers.STATE_COUNTER_CONTRACT_ID, 0)) == nil,
    "rejected multi-contract bundle must publish no contracts")

local first_bundle = bundle("lua-in-process-first", 0)
local first_id = first:register_in_process_bundle(first_bundle)
first_bundle = nil
collectgarbage("collect")
collectgarbage("collect")

local first_counter = callers.StateCounterContract_create(first, first:host())
local first_peer = callers.StateCounterContract_create(first, first:host())
check(first_counter:increment() == 1 and first_counter:increment() == 2,
    "factory state must be retained per caller instance")
check(first_peer:value() == 0, "separate caller instances must not share state")

local second_id = second:register_in_process_bundle(bundle("lua-in-process-second", 100))
local second_counter = callers.StateCounterContract_create(second, second:host())
check(second_counter:increment() == 101, "second Runtime must own an independent resident")
check(first_counter:value() == 2, "second Runtime must not alter first Runtime state")
first_counter:destroy()
first_peer:destroy()

local dependent_bundle = dependent_callers.in_process_bundle({
    name = "lua-in-process-dependent",
    dependencies = { callers.STATE_COUNTER_CONTRACT_ID },
    implementations = {
        ["state.dependent"] = marker_factory(500),
    },
}, lua_loader.bridge_lib())
local dependent_id = first:register_in_process_bundle(dependent_bundle)
check(not pcall(function() first:unload_bundle(first_id) end),
    "unload with a dependent bundle must fail")

collectgarbage("collect")
local retained_counter = callers.StateCounterContract_create(first, first:host())
check(retained_counter:value() == 0, "failed unload must retain a callable resident")
retained_counter:destroy()

first:unload_bundle(dependent_id)
first:unload_bundle(first_id)
local replacement_id = first:register_in_process_bundle(bundle("lua-in-process-first", 7))
local replacement_counter = callers.StateCounterContract_create(first, first:host())
check(replacement_counter:value() == 7, "successful unload must permit fresh re-registration")
replacement_counter:destroy()
first:unload_bundle(replacement_id)

second_counter:destroy()
second:unload_bundle(second_id)
first:destroy()
second:destroy()
os.execute("rm -rf " .. quote(generated_root))
print("PASS: Lua generated in-process atomicity, state, isolation, resident lifetime, unload, and re-registration")
