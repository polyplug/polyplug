-- Lua host example using polyplugc-generated bindings.
--
-- This host demonstrates the real-world polyplug pattern:
--   1. Generate host bindings: polyplugc --api api.toml --lang lua --out generated/
--   2. Import generated callers: local callers = require("generated.host.callers")
--   3. Use type-safe contract wrappers instead of manual vtable dispatch
--
-- Zero hand-written contract IDs, zero manual unsafe dispatch.

local ffi = require("ffi")

local script_dir = debug.getinfo(1, "S").source:match("^@(.*/)") or "./"
local REPO_ROOT = script_dir .. "../../.."

package.path = REPO_ROOT .. "/host-libs/lua/?.lua;"
    .. REPO_ROOT .. "/host-libs/lua/loaders/?.lua;"
    .. package.path

local polyplug = require("polyplug")
local native_loader = require("native")
local dotnet_loader = require("dotnet")
local python_loader = require("python")
local lua_loader = require("lua")
local js_loader = require("js")

local function resolve_polyplug_so()
    local env_path = os.getenv("POLYPLUG_SO")
    if env_path and #env_path > 0 then
        return env_path
    end
    return REPO_ROOT .. "/target/debug/libpolyplug.so"
end

polyplug.load_lib(resolve_polyplug_so())

-- Import generated callers and contract IDs
local callers = require("generated.host.callers")

local function string_view_to_str(sv)
    if sv.ptr == nil or sv.len == 0 then
        return ""
    end
    return ffi.string(sv.ptr, sv.len)
end

local function resolve_plugin_path()
    local env_path = os.getenv("POLYPLUG_PLUGIN_PATH")
    if env_path and #env_path > 0 then
        return env_path
    end
    return REPO_ROOT .. "/examples/plugins"
end

local function main()
    local plugin_path = resolve_plugin_path()
    print("plugin directory: " .. plugin_path)

    local rt = polyplug.Runtime.new()
    
    -- Register all loaders
    polyplug.register_native_loader(rt._ptr)
    polyplug.register_dotnet_loader(rt._ptr, { min_framework = "10.0" })
    polyplug.register_python_loader(rt._ptr, { min_version = "3.11" })
    polyplug.register_lua_loader(rt._ptr)
    polyplug.register_js_loader(rt._ptr)

    -- Load bundles from plugin directory
    local bundles = rt:scan_plugin_dir(plugin_path)
    print("Loaded " .. #bundles .. " bundles")

    print("\n=== polyplug lua host example ===")

    -- Find and call decoder plugin using generated caller
    local decoder_handle = rt:find_by_contract(callers.PIPELINE_DECODER_CONTRACT_ID, 0)
    if decoder_handle ~= nil then
        print("[lua_decoder] found decoder plugin")
        
        -- Use generated caller for type-safe invocation
        local vtable = rt:resolve_plugin(decoder_handle)
        if vtable ~= nil then
            local input_sv = ffi.new("StringView", "name,value,42")
            local result = callers.pipeline_Decoder_decode(vtable, input_sv)
            print("  decode result: " .. string_view_to_str(result))
        end
    end

    -- Find and call transformer plugin
    local transformer_handle = rt:find_by_contract(callers.DATA_TRANSFORMER_CONTRACT_ID, 0)
    if transformer_handle ~= nil then
        print("[lua_transformer] found transformer plugin")
        
        local vtable = rt:resolve_plugin(transformer_handle)
        if vtable ~= nil then
            local data_sv = ffi.new("StringView", "test,data,123")
            local result = callers.data_Transformer_transform(vtable, data_sv)
            print("  transform result: " .. string_view_to_str(result))
        end
    end

    -- Find and call encoder plugin
    local encoder_handle = rt:find_by_contract(callers.PIPELINE_ENCODER_CONTRACT_ID, 0)
    if encoder_handle ~= nil then
        print("[lua_encoder] found encoder plugin")
        
        local vtable = rt:resolve_plugin(encoder_handle)
        if vtable ~= nil then
            local data_sv = ffi.new("StringView", "name|value|42")
            local result = callers.pipeline_Encoder_encode(vtable, data_sv)
            print("  encode result: " .. string_view_to_str(result))
        end
    end

    print("\n=== done ===")
end

main()
