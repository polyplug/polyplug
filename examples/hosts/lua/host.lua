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
local js_deno_loader = require("js_deno")

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

local function fnv1a_64(str)
    local FNV_OFFSET = ffi.cast("uint64_t", 0xCBF29CE484222325ULL)
    local FNV_PRIME  = ffi.cast("uint64_t", 0x00000100000001B3ULL)
    local hash = FNV_OFFSET
    for i = 1, #str do
        hash = ffi.cast("uint64_t", bit.bxor(tonumber(hash % 256), str:byte(i)))
              + ffi.cast("uint64_t", hash - hash % 256)
        hash = hash * FNV_PRIME
    end
    return hash
end

local function u64(hi, lo)
    return ffi.cast("uint64_t", hi) * 0x100000000ULL + lo
end

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

local GUESTS = {
    { dir = "rust/decoder",           bundle_name = "rust_transformer",       contract_id = TRANSFORMER_CONTRACT_ID, fn_name = "transform" },
    { dir = "rust/reporter",          bundle_name = "rust_reporter",          contract_id = REPORTER_CONTRACT_ID,    fn_name = "report" },
    { dir = "cpp/transformer",        bundle_name = "cpp_transformer",        contract_id = TRANSFORMER_CONTRACT_ID, fn_name = "transform" },
    { dir = "cpp/reporter",           bundle_name = "cpp_reporter",           contract_id = REPORTER_CONTRACT_ID,    fn_name = "report" },
    { dir = "csharp/encoder",         bundle_name = "csharp_transformer",     contract_id = TRANSFORMER_CONTRACT_ID, fn_name = "transform" },
    { dir = "csharp/reporter",        bundle_name = "csharp_reporter",        contract_id = REPORTER_CONTRACT_ID,    fn_name = "report" },
    { dir = "python/decoder",         bundle_name = "python_transformer",     contract_id = TRANSFORMER_CONTRACT_ID, fn_name = "transform" },
    { dir = "python/reporter",        bundle_name = "python_reporter",        contract_id = REPORTER_CONTRACT_ID,    fn_name = "report" },
    { dir = "lua/transformer",        bundle_name = "lua_transformer",        contract_id = TRANSFORMER_CONTRACT_ID, fn_name = "transform" },
    { dir = "lua/reporter",           bundle_name = "lua_reporter",           contract_id = REPORTER_CONTRACT_ID,    fn_name = "report" },
    { dir = "js_quickjs/transformer", bundle_name = "js_quickjs_transformer", contract_id = TRANSFORMER_CONTRACT_ID, fn_name = "transform" },
    { dir = "js_quickjs/reporter",    bundle_name = "js_quickjs_reporter",    contract_id = REPORTER_CONTRACT_ID,    fn_name = "report" },
    { dir = "js_deno/transformer",    bundle_name = "js_deno_transformer",    contract_id = TRANSFORMER_CONTRACT_ID, fn_name = "transform" },
    { dir = "js_deno/reporter",       bundle_name = "js_deno_reporter",       contract_id = REPORTER_CONTRACT_ID,    fn_name = "report" },
}

local function string_view_to_str(sv)
    if sv.ptr == nil or sv.len == 0 then
        return ""
    end
    return ffi.string(sv.ptr, sv.len)
end

local function main()
    local rt = polyplug.Runtime.new()

    polyplug.register_native_loader(rt._ptr)
    polyplug.register_dotnet_loader(rt._ptr, { min_framework = "10.0" })
    polyplug.register_python_loader(rt._ptr, { min_version = "3.11" })
    polyplug.register_lua_loader(rt._ptr)
    polyplug.register_js_loader(rt._ptr)
    js_deno_loader.register(rt._ptr)

    for _, g in ipairs(GUESTS) do
        rt:load_bundle(REPO_ROOT .. "/examples/guests/" .. g.dir)
    end

    for _, g in ipairs(GUESTS) do
        local bid = bundle_id(g.bundle_name)
        local handle = rt:find_by_bundle(bid, g.contract_id, 0)
        if ffi.cast("uint64_t", handle) == polyplug.NULL_HANDLE then
            error("plugin not found: " .. g.bundle_name)
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
            error(string.format("call failed for %s: code %d", g.dir, abi_err.code))
        end

        local result = string_view_to_str(output_sv)
        local label = "[" .. g.dir .. "]"
        print(string.format("%-30s %s(\"hello\") = \"%s\"", label, g.fn_name, result))

        guard:free()
    end

    rt:free()
end

local ok, err = pcall(main)
if not ok then
    io.stderr:write("error: " .. tostring(err) .. "\n")
    os.exit(1)
end
