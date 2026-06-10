-- sdks/lua/guest/polyplug_guest.lua
-- LuaJIT FFI guest library for polyplug plugins.
-- This module is loaded via require("polyplug_guest").
-- All ffi.cdef calls are at module load time and guarded with pcall
-- to prevent "already defined" errors when a second plugin calls require().

local ffi = require("ffi")
-- Load-bearing: registers the ABI ffi.cdef definitions (StringView, HostApi,
-- AbiError, ...) that every ffi.new/ffi.cast below depends on. StringView
-- helpers (to_str, starts_with, ...) also live there — see sdk_validator.yaml.
require("polyplug_abi")

local M = {}

-- Lua-accessible constant tables mirroring the ABI enums. The generated
-- abi.lua only declares these as C enums (ffi.C.AbiErrorCode_Ok,
-- ffi.C.DispatchType_Native, ...), so the guest SDK surfaces them as plain
-- Lua tables for generated guest code and plugin authors.
M.AbiErrorCode = {
    Ok = 0,
    Generic = 1,
    BufferTooSmall = 2,
    Panic = 3,
    NotFound = 4,
    StaleHandle = 5,
    FunctionNotAvailable = 6,
    DuplicateProvider = 7,
    InvalidPointer = 8,
    HostContractNotFound = 100,
    HostContractVersionMismatch = 101,
    HostContractCallFailed = 102,
}

M.DispatchType = {
    Native = 0,
    VirtualMachine = 1,
}

-- Log severity levels for M.log, mirroring the ABI LogLevel enum
-- (LogLevel_Error .. LogLevel_Trace in abi.lua). Lower values are more severe.
M.LogLevel = {
    Error = 1,
    Warn  = 2,
    Info  = 3,
    Debug = 4,
    Trace = 5,
}

local _host_interface_ptr = nil

function M.store_host_interface(ptr)
    _host_interface_ptr = ptr
end

function M.get_host_interface()
    return _host_interface_ptr
end

function M.cast_host_interface(ptr_int)
    return ffi.cast("HostApi*", ffi.cast("uintptr_t", ptr_int))
end

function M.cast_context(ptr)
    return ffi.cast("BundleInitContext*", ptr)
end

function M.string_view(s)
    return ffi.new("StringView", { ptr = ffi.cast("const uint8_t*", s), len = #s })
end

-- Allocate a string in HOST memory and return a StringView pointing at it.
-- Cross-boundary data MUST use the host allocator (CLAUDE.md rule 8),
-- so the returned bytes outlive this call and may be handed back to the host.
function M.alloc_string(s)
    local host_ptr = _host_interface_ptr
    if host_ptr == nil then
        error("alloc_string: host interface not stored (call store_host_interface first)")
    end
    local host = ffi.cast("HostApi*", ffi.cast("uintptr_t", host_ptr))
    local len = #s
    local view = ffi.new("StringView")
    if len == 0 then
        view.ptr = nil
        view.len = 0
        return view
    end
    -- align 1 is valid for raw byte buffers.
    local buf = host.alloc(host, len, 1)
    if buf == nil then
        error("alloc_string: host allocation failed")
    end
    ffi.copy(buf, s, len)
    view.ptr = buf
    view.len = len
    return view
end

-- Allocate a return-value string from the current per-call CallArena.
--
-- Use this for strings RETURNED from a contract function: the bytes are served
-- from the host's per-call arena (published by the loader via the
-- `_polyplug_arena_alloc` bridge) and stay valid until the next call on the same
-- caller, so the guest never frees them. When no arena is active the bridge
-- falls back to `host->alloc`, so this behaves like `alloc_string`. For data
-- that must outlive the call, use `alloc_string` and free it explicitly.
function M.alloc_string_arena(s)
    local arena_alloc = _G._polyplug_arena_alloc
    if arena_alloc == nil then
        return M.alloc_string(s)
    end
    local len = #s
    local view = ffi.new("StringView")
    if len == 0 then
        view.ptr = nil
        view.len = 0
        return view
    end
    local addr = arena_alloc(len)
    if addr == 0 then
        error("alloc_string_arena: arena allocation failed")
    end
    local buf = ffi.cast("uint8_t*", ffi.cast("uintptr_t", addr))
    ffi.copy(buf, s, len)
    view.ptr = buf
    view.len = len
    return view
end

-- Send a log record to the host's logging funnel (RuntimeConfig log callback,
-- or the host's stderr default).
--
-- `level` is one of M.LogLevel (unknown values are clamped to Error by the
-- loader), `scope` is a short stable tag chosen by the guest — the suggested
-- convention is "guest.<plugin-name>" — and `message` is delivered verbatim.
--
-- The `_polyplug_log` bridge is injected into the VM by the polyplug Lua
-- loader; outside a polyplug VM (e.g. plain LuaJIT unit tests of plugin code)
-- it is absent and this helper degrades to a no-op.
function M.log(level, scope, message)
    local log_fn = _G._polyplug_log
    if log_fn == nil then
        return
    end
    log_fn(level, scope, message)
end

function M.ok()
    return ffi.new("AbiError", { code = 0 })
end

function M.err(code, message)
    return ffi.new("AbiError", { code = code, message = M.string_view(message) })
end

return M