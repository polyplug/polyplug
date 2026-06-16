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
    ReentrantCall = 9,
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
--
-- `host_ptr` is the HostApi pointer threaded explicitly into the guest (the
-- author factory receives it; the generated dispatch threads it) — no host
-- pointer is stored in this module (Rule 12).
function M.alloc_string(host_ptr, s)
    if host_ptr == nil or host_ptr == 0 then
        error("alloc_string: host pointer is nil")
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

-- Allocate a return-value string from THIS call's CallArena.
--
-- Use this for strings RETURNED from a contract function: the bytes are served
-- from the host's per-call arena and stay valid until the caller's next
-- arena-backed call, so the guest never frees them. For data that must outlive
-- the call, use `alloc_string` and free it explicitly.
--
-- `arena_alloc(size, arena) -> integer` is the loader-supplied allocator passed
-- as the FINAL positional argument of every dispatch call (NOT a module global);
-- `arena_ptr` is the `arena` integer the dispatch passed to the handler. Both are
-- threaded explicitly so concurrent and same-VM reentrant dispatch stay correct —
-- each call's arena travels with its own call frame (Rule 12).
function M.alloc_string_arena(arena_alloc, arena_ptr, s)
    local len = #s
    local view = ffi.new("StringView")
    if len == 0 then
        view.ptr = nil
        view.len = 0
        return view
    end
    local addr = arena_alloc(len, arena_ptr)
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
-- or the host's stderr default) by calling `HostApi.log` DIRECTLY through the
-- threaded host pointer — no loader-injected `_polyplug_log` bridge (Rule 12).
--
-- `host_ptr` is the HostApi pointer threaded into the guest (the author factory
-- receives it); a nil/0 host_ptr is a no-op, so plugins may call this
-- unconditionally (e.g. plain LuaJIT unit tests with no host). `level` is one of
-- M.LogLevel (the host clamps unknown values to Error), `scope` is a short stable
-- tag — convention "guest.<plugin-name>" — and `message` is delivered verbatim.
function M.log(host_ptr, level, scope, message)
    if host_ptr == nil or host_ptr == 0 then
        return
    end
    -- Self-passing convention: log(this, level, scope, message). The host reads
    -- both views only for the duration of this synchronous call.
    local host = ffi.cast("HostApi*", ffi.cast("uintptr_t", host_ptr))
    host.log(host, level, M.string_view(scope), M.string_view(message))
end

function M.ok()
    return ffi.new("AbiError", { code = 0 })
end

function M.err(code, message)
    return ffi.new("AbiError", { code = code, message = M.string_view(message) })
end

return M