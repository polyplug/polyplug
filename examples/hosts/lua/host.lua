local ffi = require("ffi")

local script_dir = debug.getinfo(1, "S").source:match("^@(.*/)")
    or debug.getinfo(1, "S").source:match("^@(.*[/\\])")
    or "./"

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

ffi.cdef([[
    typedef struct {
        const uint8_t* ptr;
        size_t         len;
    } ExStringView;

    typedef struct {
        uint32_t   code;
        uint32_t   _pad;
        const uint8_t* msg_ptr;
        size_t         msg_len;
    } ExAbiError;

    typedef ExAbiError (*ExAbiFunc)(const void* args, void* out);

    typedef struct {
        uint64_t   contract_id;
        uint32_t   contract_version;
        uint32_t   function_count;
        ExAbiFunc* functions;
    } ExPluginVTable;
]])

local TRANSFORMER_CONTRACT_ID = ffi.cast("uint64_t", 0x3D53C682F3F5A9EFULL)
local REPORTER_CONTRACT_ID    = ffi.cast("uint64_t", 0x81D41D43E511D297ULL)

local ABI_OK = 0

local BUNDLE_IDS = {}
local function bundle_id(name)
    if not BUNDLE_IDS[name] then
        local FNV_OFFSET = 0xCBF29CE484222325ULL
        local FNV_PRIME  = 0x00000100000001B3ULL
        local hash = ffi.cast("uint64_t", FNV_OFFSET)
        for i = 1, #name do
            local b = ffi.cast("uint64_t", name:byte(i))
            hash = bit.bxor(hash, b)
            hash = hash * FNV_PRIME
        end
        BUNDLE_IDS[name] = hash
    end
    return BUNDLE_IDS[name]
end

local function resolve_plugin_path()
    local env_path = os.getenv("POLYPLUG_PLUGIN_PATH")
    if env_path and #env_path > 0 then
        return env_path
    end
    return REPO_ROOT .. "/examples/plugins"
end

local function scan_plugin_dir(dir)
    local bundles = {}
    local p = io.popen('ls -1 "' .. dir .. '" 2>/dev/null')
    if not p then return bundles end

    for entry in p:lines() do
        local bundle_dir = dir .. "/" .. entry
        local manifest_path = bundle_dir .. "/manifest.toml"
        local f = io.open(manifest_path, "r")
        if f then
            local content = f:read("*all")
            f:close()

            local bname = content:match('bundle_name%s*=%s*"([^"]+)"')
            local provides = {}
            local provides_str = content:match('provides%s*=%s*%[([^%]]+)%]')
            if provides_str then
                for contract in provides_str:gmatch('"([^"]+)"') do
                    provides[#provides + 1] = contract
                end
            end

            if bname then
                bundles[#bundles + 1] = {
                    path = bundle_dir,
                    bundle_name = bname,
                    provides = provides,
                }
            end
        end
    end
    p:close()

    table.sort(bundles, function(a, b) return a.bundle_name < b.bundle_name end)
    return bundles
end

local function string_view_to_str(sv)
    if sv.ptr == nil or sv.len == 0 then
        return ""
    end
    return ffi.string(sv.ptr, sv.len)
end

local function main()
    local plugin_dir = resolve_plugin_path()
    io.stderr:write("plugin directory: " .. plugin_dir .. "\n")

    local rt = polyplug.Runtime.new()

    polyplug.register_native_loader(rt._ptr)
    polyplug.register_dotnet_loader(rt._ptr, { min_framework = "10.0" })
    polyplug.register_python_loader(rt._ptr, { min_version = "3.11" })
    polyplug.register_lua_loader(rt._ptr)
    polyplug.register_js_loader(rt._ptr)

    local bundles = scan_plugin_dir(plugin_dir)
    if #bundles == 0 then
        error("no plugins found in " .. plugin_dir .. ". Run examples/build_all.sh first.")
    end

    io.stderr:write("discovered " .. #bundles .. " bundles\n")

    for _, b in ipairs(bundles) do
        rt:load_bundle(b.path)
        io.stderr:write("  loaded: " .. b.bundle_name .. "\n")
    end

    for _, b in ipairs(bundles) do
        local contract_id = nil
        local fn_name = nil

        for _, contract in ipairs(b.provides) do
            if contract == "data.Transformer" then
                contract_id = TRANSFORMER_CONTRACT_ID
                fn_name = "transform"
                break
            elseif contract == "data.Reporter" then
                contract_id = REPORTER_CONTRACT_ID
                fn_name = "report"
                break
            end
        end

        if not contract_id then goto continue end

        local bid = bundle_id(b.bundle_name)
        local handle = rt:find_by_bundle(bid, contract_id, 0)
        if ffi.cast("uint64_t", handle) == polyplug.NULL_HANDLE then
            error("plugin not found: " .. b.bundle_name)
        end

        local guard, err = rt:resolve_plugin(handle)
        if not guard then
            error("resolve failed: " .. (err or "unknown"))
        end

        local vtable_ptr = guard:vtable()
        local vt = ffi.cast("const ExPluginVTable*", vtable_ptr)

        local input = "hello"
        local input_sv = ffi.new("ExStringView")
        input_sv.ptr = ffi.cast("const uint8_t*", input)
        input_sv.len = #input

        local output_sv = ffi.new("ExStringView")
        output_sv.ptr = nil
        output_sv.len = 0

        local abi_err = vt.functions[0](input_sv, output_sv)
        if abi_err.code ~= ABI_OK then
            guard:free()
            error(string.format("call failed for %s: code %d", b.bundle_name, abi_err.code))
        end

        local result = string_view_to_str(output_sv)
        local label = "[" .. b.bundle_name .. "]"
        print(string.format("%-30s %s(\"hello\") = \"%s\"", label, fn_name, result))

        guard:free()

        ::continue::
    end

    rt:free()
end

local ok, err = pcall(main)
if not ok then
    io.stderr:write("error: " .. tostring(err) .. "\n")
    os.exit(1)
end
