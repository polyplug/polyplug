-- host-libs/lua/polyplug.lua
-- LuaJIT FFI host library for polyplug.

local ffi = require('ffi')
local jit = require('jit')
local M = {}

local function get_platform_identifier()
    local os = jit.os
    local arch = jit.arch
    local os_map = {
        ["Linux"] = "linux",
        ["OSX"] = "darwin",
        ["Windows"] = "win32"
    }
    local arch_map = {
        ["x64"] = "x64",
        ["arm64"] = "arm64"
    }
    return (os_map[os] or "unknown") .. "-" .. (arch_map[arch] or "unknown")
end

local function get_script_dir()
    local source = debug.getinfo(1, "S").source
    if source:sub(1, 1) == "@" then
        return source:match("^@(.+)/") or "."
    end
    return "."
end

local function auto_load_native_lib()
    local platform = get_platform_identifier()
    local script_dir = get_script_dir()
    local native_dir = script_dir .. "/_native/" .. platform
    
    local jit_os = jit.os
    local lib_name
    if jit_os == "Windows" then
        lib_name = "polyplug.dll"
    else
        lib_name = "libpolyplug.so"
    end
    
    local lib_path = native_dir .. "/" .. lib_name
    
    local f = io.open(lib_path, "r")
    if f then
        f:close()
        return M.load_lib(lib_path)
    end
    
    local env_lib = os.getenv("POLYPLUG_LIB")
    if env_lib then
        return M.load_lib(env_lib)
    end
    
    return M.load_lib(lib_name)
end

-- Error code constants matching polyplug ABI
M.PolyplugError = {
    NOT_FOUND = 4,
    STALE_HANDLE = 5,
    FUNCTION_NOT_AVAIL = 6
}

local FNV_OFFSET = 0xcbf29ce484222325ULL
local FNV_PRIME = 0x00000100000001B3ULL

local function fnv1a_64(str)
    local h = FNV_OFFSET
    for i = 1, #str do
        local b = str:byte(i)
        h = ffi.bit.bxor(h, b)
        h = h * FNV_PRIME
    end
    return h
end

function M.contract_id(name, major_version)
    local s = name .. '@' .. tostring(major_version)
    return fnv1a_64(s)
end

function M.bundle_id(name)
    return fnv1a_64(name)
end

ffi.cdef([[
    typedef struct OpaqueRuntime OpaqueRuntime;

    OpaqueRuntime* polyplug_runtime_create(void);
    void polyplug_runtime_destroy(OpaqueRuntime* rt);
    uint32_t polyplug_runtime_load_bundle(OpaqueRuntime* rt, const uint8_t* path, size_t path_len);
    uint32_t polyplug_runtime_reload_bundle(OpaqueRuntime* rt, const uint8_t* path, size_t path_len);
    uint64_t polyplug_runtime_find_by_contract(const OpaqueRuntime* rt, uint64_t contract_id, uint32_t min_version);
    uint64_t polyplug_runtime_find_by_bundle(const OpaqueRuntime* rt, uint64_t bundle_id, uint64_t contract_id, uint32_t min_version);
    size_t polyplug_runtime_find_all_by_contract(const OpaqueRuntime* rt, uint64_t contract_id, uint32_t min_version, uint64_t* out, size_t out_cap);
    const void* polyplug_runtime_resolve_plugin(const OpaqueRuntime* rt, uint64_t packed_handle);
    uint32_t polyplug_runtime_error_message_len(void);
    void polyplug_runtime_last_error(uint8_t* buf, size_t buf_len);
    uint32_t polyplug_runtime_register_loader(OpaqueRuntime* rt, void* loader_ptr);
    void polyplug_host_free(void* ptr, size_t size, size_t align);

    typedef struct { uint8_t _reserved; } PolyplugNativeConfig;
    void* polyplug_native_loader_create(const PolyplugNativeConfig* cfg);

    typedef struct { const uint8_t* ptr; size_t len; } StringView;

    typedef struct {
        uint64_t contract_id;
        uint32_t contract_version;
        uint32_t function_count;
        const void* functions;
    } PluginVTable;

    typedef struct {
        uint32_t code;
        uint32_t _pad;
        const uint8_t* message_ptr;
        size_t message_len;
    } AbiError;
]])

local DispatchFnType = ffi.typeof("AbiError (*)(const void*, void*)")
local func_cache = {}

M.NULL_HANDLE = ffi.cast("uint64_t", "0xFFFFFFFFFFFFFFFF")

function M.load_lib(so_path)
    M._lib = ffi.load(so_path)
    return M._lib
end

local function get_lib()
    if not M._lib then
        error("polyplug: library not loaded. Call load_lib() first.")
    end
    return M._lib
end

function M.last_error(lib)
    lib = lib or get_lib()
    local len = lib.polyplug_runtime_error_message_len()
    if len == 0 then
        return ""
    end
    local buf = ffi.new("uint8_t[?]", len)
    lib.polyplug_runtime_last_error(buf, len)
    return ffi.string(buf, len)
end

M.Runtime = {}
M.Runtime.__index = M.Runtime

function M.Runtime.new()
    local lib = get_lib()
    local rt_ptr = lib.polyplug_runtime_create()
    if rt_ptr == nil then
        error("polyplug_runtime_create failed")
    end
    local self = { _ptr = rt_ptr, _lib = lib, _destroyed = false }
    local obj = setmetatable(self, M.Runtime)
    ffi.gc(rt_ptr, function(ptr)
        if not self._destroyed and ptr ~= nil then
            lib.polyplug_runtime_destroy(ptr)
        end
    end)
    return obj
end

function M.Runtime:load_bundle(path)
    local lib = self._lib
    local path_str = tostring(path)
    local result = lib.polyplug_runtime_load_bundle(self._ptr, path_str, #path_str)
    if result ~= 0 then
        error("polyplug_runtime_load_bundle failed: " .. result)
    end
end

function M.Runtime:reload_bundle(path)
    local lib = self._lib
    local path_str = tostring(path)
    local result = lib.polyplug_runtime_reload_bundle(self._ptr, path_str, #path_str)
    if result ~= 0 then
        error("polyplug_runtime_reload_bundle failed: " .. result)
    end
end

function M.Runtime:find_by_bundle(bundle_id, contract_id, min_version)
    local lib = self._lib
    return lib.polyplug_runtime_find_by_bundle(self._ptr, bundle_id, contract_id, min_version)
end

function M.Runtime:find_by_contract(contract_id, min_version)
    local lib = self._lib
    return lib.polyplug_runtime_find_by_contract(self._ptr, contract_id, min_version)
end

function M.Runtime:find_all_by_contract(contract_id, min_version, cap)
    cap = cap or 64
    local lib = self._lib
    local out = ffi.new("uint64_t[?]", cap)
    local count = lib.polyplug_runtime_find_all_by_contract(self._ptr, contract_id, min_version, out, cap)
    local result = {}
    for i = 0, math.min(count, cap) - 1 do
        table.insert(result, out[i])
    end
    return result
end

function M.Runtime:resolve_plugin(packed_handle)
    if packed_handle == M.NULL_HANDLE then
        return nil, "null handle"
    end
    -- Guard stores runtime and handle for hot-reload safety
    -- Each call re-resolves vtable to detect stale handles
    return M.Guard.new(self, packed_handle)
end

function M.Runtime:destroy()
    if self._ptr ~= nil and not self._destroyed then
        self._lib.polyplug_runtime_destroy(self._ptr)
        self._destroyed = true
        self._ptr = nil
    end
end

-- Guard stores runtime + handle for hot-reload safety
-- Re-resolves vtable on each call to detect stale handles after hot-reload
M.Guard = {}
M.Guard.__index = M.Guard

function M.Guard.new(runtime, packed_handle)
    if runtime == nil then
        error("polyplug: runtime is nil")
    end
    if packed_handle == nil then
        error("polyplug: packed_handle is nil")
    end
    local self = {
        _runtime = runtime,
        _handle = packed_handle,
    }
    return setmetatable(self, M.Guard)
end

function M.Guard:handle()
    return self._handle
end

-- Internal: resolve vtable for this call (hot-reload safe)
function M.Guard:_resolve_vtable()
    local rt = self._runtime
    if rt._destroyed then
        return nil, "runtime destroyed"
    end
    local lib = rt._lib
    local vtable_ptr = lib.polyplug_runtime_resolve_plugin(rt._ptr, self._handle)
    if vtable_ptr == nil then
        return nil, M.last_error(lib)
    end
    return vtable_ptr, nil
end

-- Call a plugin function by index (hot-reload safe)
-- Re-resolves vtable on each call to detect stale handles
function M.Guard:call(func_idx, input)
    local vtable_ptr, err = self:_resolve_vtable()
    if vtable_ptr == nil then
        error("polyplug: failed to resolve vtable: " .. (err or "unknown"))
    end
    
    local lib = self._runtime._lib
    local vtable = ffi.cast("const PluginVTable*", vtable_ptr)
    
    if func_idx >= vtable.function_count then
        error("function index " .. func_idx .. " out of bounds")
    end
    
    local funcs = ffi.cast("const void* const*", vtable.functions)
    local func_ptr = funcs[func_idx]
    local func = func_cache[func_ptr]
    if func == nil then
        func = ffi.cast(DispatchFnType, func_ptr)
        func_cache[func_ptr] = func
    end
    
    local input_data = ffi.new("uint8_t[?]", #input)
    ffi.copy(input_data, input, #input)
    local input_sv = ffi.new("StringView", { ptr = input_data, len = #input })
    
    local output_sv = ffi.new("StringView", { ptr = nil, len = 0 })
    
    local result = func(ffi.cast("const void*", input_sv), ffi.cast("void*", output_sv))
    
    if result.code == 0 and output_sv.ptr ~= nil and output_sv.len > 0 then
        local output_str = ffi.string(output_sv.ptr, output_sv.len)
        lib.polyplug_host_free(ffi.cast("void*", output_sv.ptr), output_sv.len, 1)
        return output_str
    else
        error("plugin returned error code=" .. result.code)
    end
end

-- Loader registration
local _loader_libs = {}

local function get_loader_lib(name)
    local lib = _loader_libs[name]
    if not lib then
        lib = ffi.load(name)
        _loader_libs[name] = lib
    end
    return lib
end

function M.register_native_loader(rt_ptr)
    local lib = get_loader_lib("polyplug_native")
    local cfg = ffi.new("PolyplugNativeConfig", { _reserved = 0 })
    local loader = lib.polyplug_native_loader_create(cfg)
    if loader == nil then
        error("polyplug: native loader create failed")
    end
    local polyplug_lib = get_lib()
    local err = polyplug_lib.polyplug_runtime_register_loader(ffi.cast("OpaqueRuntime*", rt_ptr), loader)
    if err ~= 0 then
        error("polyplug: native loader register failed: " .. err)
    end
end

-- TOML parser
local function parse_toml(content)
    local result = {}
    local current_section = nil
    
    for line in content:gmatch('[^\n]+') do
        line = line:gsub('^%s+', ''):gsub('%s+$', '')
        if #line > 0 and not line:match('^#') then
            local section = line:match('^%[(.+)%]$')
            if section then
                current_section = section
                if section ~= 'function_count' then
                    result[section] = {}
                end
            else
                local key, value = line:match('^([%w_]+)%s*=%s*(.+)$')
                if key and value then
                    value = value:gsub('^"', ''):gsub('"$', '')
                    if value:match('^%[') then
                        local arr = {}
                        for item in value:gmatch('"([^"]+)"') do
                            table.insert(arr, item)
                        end
                        value = arr
                    elseif value:match('^%d+$') then
                        value = tonumber(value)
                    end
                    
                    if current_section and current_section ~= 'function_count' then
                        result[current_section][key] = value
                    elseif not current_section or current_section == 'function_count' then
                        result[key] = value
                    end
                end
            end
        end
    end
    
    return result
end

-- Scanner
function M.scan_dir(dir_path)
    local bundles = {}
    local dir = io.popen('find "' .. dir_path .. '" -maxdepth 1 -type d -mindepth 1 2>/dev/null')
    for subdir in dir:lines() do
        local manifest_path = subdir .. '/manifest.toml'
        local file = io.open(manifest_path, 'r')
        if file then
            local content = file:read('*all')
            file:close()
            local manifest = parse_toml(content)
            table.insert(bundles, { path = subdir, manifest = manifest })
        end
    end
    dir:close()
    return bundles
end

-- String helpers
local function to_str(sv)
    if not sv.ptr or sv.len == 0 then
        return ""
    end
    return ffi.string(sv.ptr, sv.len)
end

M.to_str = to_str
M.to_string = to_str

M.get_platform_identifier = get_platform_identifier
M.get_script_dir = get_script_dir

auto_load_native_lib()

return M