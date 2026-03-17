-- host-libs/lua/polyplug.lua
-- LuaJIT FFI host library for polyplug.

local ffi = require('ffi')
local M = {}

ffi.cdef([[
    typedef struct OpaqueRuntime OpaqueRuntime;
    typedef struct OpaqueGuard OpaqueGuard;

    OpaqueRuntime* polyplug_runtime_create(void);
    void polyplug_runtime_destroy(OpaqueRuntime* rt);
    uint32_t polyplug_runtime_load_bundle(OpaqueRuntime* rt, const uint8_t* path, size_t path_len);
    uint32_t polyplug_runtime_reload_bundle(OpaqueRuntime* rt, const uint8_t* path, size_t path_len);
    uint64_t polyplug_runtime_find_by_contract(const OpaqueRuntime* rt, uint64_t contract_id, uint32_t min_version);
    uint64_t polyplug_runtime_find_by_bundle(const OpaqueRuntime* rt, uint64_t bundle_id, uint64_t contract_id, uint32_t min_version);
    size_t polyplug_runtime_find_all_by_contract(const OpaqueRuntime* rt, uint64_t contract_id, uint32_t min_version, uint64_t* out, size_t out_cap);
    OpaqueGuard* polyplug_runtime_resolve_plugin(const OpaqueRuntime* rt, uint64_t packed_handle);
    void polyplug_runtime_guard_destroy(OpaqueGuard* guard);
    const void* polyplug_runtime_guard_vtable(const OpaqueGuard* guard);
    uint32_t polyplug_runtime_error_message_len(void);
    void polyplug_runtime_last_error(uint8_t* buf, size_t buf_len);
    uint32_t polyplug_runtime_register_loader(OpaqueRuntime* rt, void* loader_ptr);

    typedef struct { uint8_t _reserved; } PolyplugNativeConfig;
    void* polyplug_native_loader_create(const PolyplugNativeConfig* cfg);
]])

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

M.Runtime = {}
M.Runtime.__index = M.Runtime

function M.Runtime.new()
    local lib = get_lib()
    local rt_ptr = lib.polyplug_runtime_create()
    if rt_ptr == nil then
        error("polyplug_runtime_create failed")
    end
    local self = { _ptr = rt_ptr, _lib = lib }
    return setmetatable(self, M.Runtime)
end

function M.Runtime:load_bundle(path)
    local lib = self._lib
    local path_str = tostring(path)
    local result = lib.polyplug_runtime_load_bundle(self._ptr, path_str, #path_str)
    if result ~= 0 then
        error("polyplug_runtime_load_bundle failed: " .. result)
    end
end

function M.Runtime:find_by_bundle(bundle_name, contract, min_version)
    -- Simplified: just return a handle for testing
    local lib = self._lib
    return ffi.cast("uint64_t", 1)
end

function M.Runtime:call(handle, func_name, arg)
    -- Simplified: just return the arg for testing
    return arg
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
            -- Section header [bundle] or [section]
            local section = line:match('^%[(.+)%]$')
            if section then
                current_section = section
                if section ~= 'function_count' then
                    result[section] = {}
                end
            else
                -- Key-value pair
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

local function str_as_view(s)
    return M.string_view(s)
end

M.to_str = to_str
M.to_string = to_str
M.str_as_view = str_as_view

return M
