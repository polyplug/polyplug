-- Generated Lua internal plugin integration test.
--
-- Run from the repository root:
--   cargo build -p polyplug -p polyplug_lua -p polyplugc
--   POLYPLUG_LIB=$PWD/target/debug/libpolyplug.so \
--   POLYPLUG_LUA_LIB=$PWD/target/debug/libpolyplug_lua.so \
--   POLYPLUGC_BIN=$PWD/target/debug/polyplugc \
--   luajit sdks/lua/host/tests/test_internal_plugin_runtime.lua

local script_dir = debug.getinfo(1, "S").source:match("^@(.+/)[^/]+$") or "./"
local polyplugc = os.getenv("POLYPLUGC_BIN")
if not polyplugc or polyplugc == "" then
    io.stderr:write("FATAL: POLYPLUGC_BIN must name a built polyplugc binary.\n")
    os.exit(1)
end
if not os.getenv("POLYPLUG_LIB") or not os.getenv("POLYPLUG_LUA_LIB") then
    io.stderr:write("FATAL: POLYPLUG_LIB and POLYPLUG_LUA_LIB must name built libraries.\n")
    os.exit(1)
end

local function quote(value)
    return "'" .. value:gsub("'", "'\\''") .. "'"
end

local function write_file(path, contents)
    local file = assert(io.open(path, "w"))
    file:write(contents)
    file:close()
end

local function generate_internal(bundle, out_dir)
    local command = quote(polyplugc) .. " generate --bundle " .. quote(bundle)
        .. " --internal --lang lua --out " .. quote(out_dir)
    if os.execute(command) ~= 0 then
        error("polyplugc failed while generating " .. bundle, 2)
    end
end

local generated_root = os.tmpname()
os.remove(generated_root)
os.execute("mkdir -p " .. quote(generated_root))

local first_api = generated_root .. "/first_api.toml"
write_file(first_api, [=[
[[plugin_contract]]
name = "profile.counter"
version = "1.0"

[[plugin_contract.functions]]
name = "increment"
return = "u32"

[[plugin_contract.functions]]
name = "value"
return = "u32"

[[plugin_contract.functions]]
name = "text"
return = "StringView"
]=])
local first_bundle = generated_root .. "/first_bundle.toml"
write_file(first_bundle, [=[
[bundle]
name = "lua_first_internal_plugin"
version = "1.0"
api = "first_api.toml"

[[plugin]]
name = "older_provider"
implements = ["profile.counter@1.0"]
]=])
generate_internal(first_bundle, generated_root)

local second_api = generated_root .. "/second_api.toml"
write_file(second_api, [=[
[[types]]
name = "SecondPayload"
fields = [{ name = "count", type = "u32" }]

[[plugin_contract]]
name = "profile.counter"
version = "1.0"

[[plugin_contract.functions]]
name = "increment"
return = "u32"

[[plugin_contract.functions]]
name = "value"
return = "u32"

[[plugin_contract.functions]]
name = "text"
return = "StringView"

[[plugin_contract]]
name = "profile.extra"
version = "1.0"

[[plugin_contract.functions]]
name = "metadata"
return = "SecondPayload"
]=])
local second_bundle = generated_root .. "/second_bundle.toml"
write_file(second_bundle, [=[
[bundle]
name = "lua_second_internal_plugin"
version = "1.0"
api = "second_api.toml"

[[plugin]]
name = "profile_provider"
implements = ["profile.counter@1.0"]

[[plugin]]
name = "profile_provider_second"
implements = ["profile.counter@1.0"]

[[plugin]]
name = "extra_provider"
implements = ["profile.extra@1.0"]
]=])
generate_internal(second_bundle, generated_root)

local directories = assert(io.popen(
    "find " .. quote(generated_root .. "/internal") .. " -mindepth 1 -maxdepth 1 -type d | sort"
))
local first_root = assert(directories:read("*l"), "first internal plugin namespace")
local second_root = assert(directories:read("*l"), "second internal plugin namespace")
directories:close()

package.path = script_dir .. "../?.lua;"
    .. script_dir .. "../../abi/?.lua;"
    .. script_dir .. "../../loaders/lua/?.lua;"
    .. package.path

local polyplug = require("polyplug")
local first_profile = dofile(first_root .. "/init.lua")
local second_profile = dofile(second_root .. "/init.lua")

local function check(ok, message)
    if not ok then
        error(message, 2)
    end
end

local function counter_factory(seed)
    return function(_host)
        local value = seed
        local serial = 0
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
            text = function(self)
                local _ = self
                serial = serial + 1
                return string.format("internal-return-%08d", serial)
            end,
        }
    end
end

local function extra_factory(_host)
    return {
        metadata = function(self)
            local _ = self
            return { count = 303 }
        end,
    }
end

local runtime = polyplug.Runtime.new()
local first_providers = first_profile.guest.providers({
    older_provider_profile_counter = counter_factory(0),
})
local first = first_profile.guest.register(runtime, first_providers)
local first_counter = first.older_provider_profile_counter
local retry_ok, retry_error = pcall(function()
    first_profile.guest.register(runtime, first_providers)
end)
check(not retry_ok and tostring(retry_error):find("consumed by a previous registration attempt", 1, true),
    "failed generated registration input must be consumed")

check(first_counter:increment() == 1, "first internal plugin must retain state")
local before_returns = collectgarbage("count")
for _ = 1, 20000 do
    first_counter:value()
end
collectgarbage("collect")
local after_returns = collectgarbage("count")
check(after_returns - before_returns < 512,
    "generated internal plugin returns must retain bounded per-instance roots")

local second_providers = second_profile.guest.providers({
    profile_provider_profile_counter = counter_factory(100),
    profile_provider_second_profile_counter = counter_factory(200),
    extra_provider_profile_extra = extra_factory,
})
local second = second_profile.guest.register(runtime, second_providers)
check(second.bundle_id ~= first.bundle_id,
    "root-relative generated bindings must keep internal plugin identities distinct")
check(second.profile_provider_profile_counter:value() == 100,
    "first same-contract provider must use its exact committed handle")
check(second.profile_provider_second_profile_counter:value() == 200,
    "second same-contract provider must use its exact committed handle")
check(second.extra_provider_profile_extra:metadata().count == 303,
    "distinct generated API types must dispatch through their own profile bindings")

check(first_counter:revalidate(),
    "exact-handle caller must revalidate after an unrelated registry revision")
check(first_counter:value() == 1,
    "revalidation after unrelated registration must preserve the exact caller state")

local foreign_counter = second_profile.host.ProfileCounterContract_create(runtime, runtime:host())
check(foreign_counter ~= nil and type(foreign_counter:value()) == "number",
    "ordinary lookup callers must dispatch through a provider owned by another Lua resident")
foreign_counter:destroy()

second.profile_provider_profile_counter:destroy()
second.profile_provider_second_profile_counter:destroy()
second.extra_provider_profile_extra:destroy()
runtime:unload_bundle(second.bundle_id)
check(first_counter:revalidate(),
    "exact-handle caller must remain revalidatable after unrelated bundle unload")
check(first_counter:value() == 1,
    "unrelated bundle unload must preserve the live exact caller instance")

first_counter:destroy()
check(not first_counter:is_valid(),
    "destroy after unrelated registry revisions must release the exact caller instance")
runtime:unload_bundle(first.bundle_id)
runtime:destroy()
os.execute("rm -rf " .. quote(generated_root))
print("PASS: Lua generated internal plugin exact handles, lifecycle, bounded returns, and multi-provider dispatch")
