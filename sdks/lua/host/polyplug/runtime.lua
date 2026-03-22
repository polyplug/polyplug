-- sdks/lua/host/polyplug/runtime.lua
-- Runtime class with hot-reload notification support.

local ffi = require('ffi')
local abi = require('polyplug_abi')
local reload_phase = require('polyplug.reload_phase')
local runtime_config = require('polyplug.runtime_config')

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
    size_t polyplug_runtime_error_message_len(void);
    void polyplug_runtime_last_error(uint8_t* buf, size_t buf_len);
    void polyplug_host_free(void* ptr, size_t size, size_t align);

    typedef void (*ReloadPhaseCallback)(
        uint32_t phase_type,
        uint64_t bundle_id,
        const uint8_t* bundle_name,
        size_t bundle_name_len,
        uint32_t retry_count,
        const uint8_t* reason,
        size_t reason_len
    );

    typedef struct {
        uint32_t hot_reload_max_retries;
        uint64_t hot_reload_retry_interval_ms;
        uint8_t hot_reload_abort_on_max_retries;
    } RuntimeConfigC;

    typedef struct {
        const RuntimeConfigC* config;
        ReloadPhaseCallback on_reload;
    } RuntimeCreateOptions;

    OpaqueRuntime* polyplug_runtime_create_with_options(const RuntimeCreateOptions* options);
]])

local M = {}

M.NULL_HANDLE = ffi.cast("uint64_t", "0xFFFFFFFFFFFFFFFF")
M.ABI_OK = abi.ABI_OK
M.ABI_ERROR_GENERIC = abi.ABI_ERROR_GENERIC
M.ABI_ERROR_NOT_FOUND = abi.ABI_ERROR_NOT_FOUND
M.ABI_ERROR_STALE_HANDLE = abi.ABI_ERROR_STALE_HANDLE
M.ABI_FUNCTION_NOT_AVAIL = abi.ABI_FUNCTION_NOT_AVAIL

M.contract_id = abi.contract_id
M.bundle_id = abi.bundle_id
M.extension_id = abi.extension_id

local function get_lib()
    if not M._lib then
        error("polyplug: library not loaded. Call load_lib() first.")
    end
    return M._lib
end

function M.load_lib(so_path)
    M._lib = ffi.load(so_path)
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

M._pending_reload_callback = nil
M._pending_config = nil

function M.on_reload(callback)
    M._pending_reload_callback = callback
end

function M.set_config(config)
    M._pending_config = config
end

function M.Runtime.new()
    local lib = get_lib()
    local rt_ptr

    if M._pending_config or M._pending_reload_callback then
        local options = ffi.new("RuntimeCreateOptions")
        local config_c

        if M._pending_config then
            config_c = ffi.new("RuntimeConfigC", {
                hot_reload_max_retries = M._pending_config.hot_reload_max_retries,
                hot_reload_retry_interval_ms = M._pending_config.hot_reload_retry_interval_ms,
                hot_reload_abort_on_max_retries = M._pending_config.hot_reload_abort_on_max_retries and 1 or 0,
            })
            options.config = config_c
        end

        if M._pending_reload_callback then
            local callback = M._pending_reload_callback
            M._ffi_reload_callback = ffi.cast("ReloadPhaseCallback", function(
                phase_type, bundle_id, bundle_name_ptr, bundle_name_len,
                retry_count, reason_ptr, reason_len
            )
                local bundle_name = ""
                if bundle_name_ptr ~= nil and bundle_name_len > 0 then
                    bundle_name = ffi.string(bundle_name_ptr, bundle_name_len)
                end
                local reason = ""
                if reason_ptr ~= nil and reason_len > 0 then
                    reason = ffi.string(reason_ptr, reason_len)
                end
                local phase = reload_phase.new(
                    phase_type, bundle_id, bundle_name, retry_count, reason
                )
                callback(phase)
            end)
            options.on_reload = M._ffi_reload_callback
        end

        rt_ptr = lib.polyplug_runtime_create_with_options(options)
    else
        rt_ptr = lib.polyplug_runtime_create()
    end

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
    return M.Guard.new(self, packed_handle)
end

function M.Runtime:destroy()
    if self._ptr ~= nil and not self._destroyed then
        self._lib.polyplug_runtime_destroy(self._ptr)
        self._destroyed = true
        self._ptr = nil
    end
end

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

function M.Guard:_resolve_interface()
    local rt = self._runtime
    if rt._destroyed then
        return nil, "runtime destroyed"
    end
    local lib = rt._lib
    local interface_ptr = lib.polyplug_runtime_resolve_plugin(rt._ptr, self._handle)
    if interface_ptr == nil then
        return nil, M.last_error(lib)
    end
    return interface_ptr, nil
end

local DispatchFnType = ffi.typeof("AbiError (*)(const void*, void*)")
local func_cache = {}

function M.Guard:call(func_idx, input)
    local interface_ptr, err = self:_resolve_interface()
    if interface_ptr == nil then
        error("polyplug: failed to resolve interface: " .. (err or "unknown"))
    end

    local lib = self._runtime._lib
    local interface = ffi.cast("const PluginInterface*", interface_ptr)

    if func_idx >= interface.function_count then
        error("function index " .. func_idx .. " out of bounds")
    end

    local dispatch_type = interface.dispatch_type
    local func_ptr

    if dispatch_type == 0 then
        local funcs = ffi.cast("const void* const*", interface.dispatch.native.functions)
        func_ptr = funcs[func_idx]
    else
        error("VM dispatch not yet supported in Lua host")
    end

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
        lib.polyplug_host_free(output_sv.ptr, output_sv.len, 1)
        return output_str
    else
        error("plugin returned error code=" .. result.code)
    end
end

return M