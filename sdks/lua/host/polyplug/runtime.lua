-- sdks/lua/host/polyplug/runtime.lua
-- Runtime class with HostApi-based API (18-04 refactor).
-- All ABI struct types are imported from the auto-generated abi.lua (per D-26).

local ffi = require('ffi')
local abi = require('polyplug_abi')
local reload_phase = require('polyplug.reload_phase')

-- ─── Host-specific FFI definitions ──────────────────────────────────────────────
-- Only FFI function declarations live here. All struct types (RuntimeConfig,
-- ReloadPhase, HostApi, etc.) come from the auto-generated abi.lua.
ffi.cdef([[
    // The only two FFI exports: create and destroy.
    // polyplug_runtime_create takes a `const RuntimeConfig*` (or NULL); the
    // on_reload callback pointer lives INSIDE RuntimeConfig (offset 8).
    const HostApi* polyplug_runtime_create(const void* config);
    bool polyplug_runtime_destroy(const HostApi* host);

    void polyplug_begin_internal_plugin(
        const HostApi* host, const uint8_t* manifest_bytes, size_t manifest_len,
        uint32_t language, uint64_t* out_bundle_id, AbiError* out_error);
    void polyplug_commit_internal_plugin_with_handles(
        const HostApi* host, uint64_t bundle_id, GuestContractHandle* out_handles,
        size_t handle_capacity, size_t* out_handle_count, AbiError* out_error);
    void polyplug_abort_internal_plugin(const HostApi* host, uint64_t bundle_id);
    bool polyplug_attach_internal_plugin_resident(
        const HostApi* host, uint64_t bundle_id, void* resident,
        uint64_t owner_thread_id, void (*release)(void*), AbiError* out_error);
    uint64_t polyplug_current_os_thread_id(void);

    // ─── Custom-logger bridge (polyplug_lua loader-cdylib trampoline) ───
    // LuaJIT FFI callbacks cannot receive structs by value, so a Lua host can
    // never install RuntimeConfig.log directly (its scope/message StringViews
    // are BY VALUE — deliberate, hot path). The polyplug_lua loader cdylib
    // exports polyplug_lua_log_trampoline with the exact RuntimeConfig.log
    // signature; it reads a PolyplugLuaLogBridge from log_user_data and
    // forwards the views as ptr+len scalars to the LuaJIT-creatable callback
    // below. Mirrors crates/polyplug_lua/src/ffi.rs::PolyplugLuaLogBridge.
    typedef void (*PolyplugLuaLogCallbackFn)(
        void*, uint32_t, const uint8_t*, size_t, const uint8_t*, size_t);
    typedef struct PolyplugLuaLogBridge {
        PolyplugLuaLogCallbackFn callback;
        void* user_data;
    } PolyplugLuaLogBridge;
    void polyplug_lua_log_trampoline(void* user_data, uint32_t level,
                                     StringView scope, StringView message);
]])

local M = {}
local pending_finalizer_runtimes = {}

-- GuestContractHandle is `#[repr(C)] { index: u32, generation: u32 }` (8 bytes); the null
-- handle is index == u32::MAX. Null checks test the `.index` field of the returned cdata.
M.NULL_HANDLE_INDEX = 0xFFFFFFFF
M.AbiErrorCode = abi.AbiErrorCode

-- Log severity levels for the opts.log callback of Runtime.new, read from the
-- ABI LogLevel enum cdef'd by abi.lua (single source — cannot drift). Lower
-- values are more severe; a record is delivered only when
-- `level <= log_max_level`.
M.LogLevel = {
    Error = tonumber(ffi.C.LogLevel_Error),
    Warn  = tonumber(ffi.C.LogLevel_Warn),
    Info  = tonumber(ffi.C.LogLevel_Info),
    Debug = tonumber(ffi.C.LogLevel_Debug),
    Trace = tonumber(ffi.C.LogLevel_Trace),
}

-- Bundle signature enforcement policy for the opts.signature_policy option of
-- Runtime.new, read from the ABI SignaturePolicy enum cdef'd by abi.lua (single
-- source — cannot drift). Defaults to SignaturePolicy.Off (unsigned bundles
-- load normally) when the option is not set.
M.SignaturePolicy = {
    Off      = tonumber(ffi.C.SignaturePolicy_Off),
    WarnOnly = tonumber(ffi.C.SignaturePolicy_WarnOnly),
    Required = tonumber(ffi.C.SignaturePolicy_Required),
}

-- Exact FFI signature used for HostApi.find_all_guest_contracts. The return
-- type is the ABI `Array` struct BY VALUE (24 bytes: items ptr + len size_t +
-- align size_t) — asserted against the generated ABI layout in
-- tests/test_reload_notification.lua. Never narrow this to a smaller struct:
-- the callee writes the full 24-byte sret.
M.FIND_ALL_FN_SIGNATURE = "Array(*)(const HostApi*, uint64_t, uint32_t)"

-- ─── Cached HostApi function-pointer ctypes ─────────────────────────────────────
-- LuaJIT does NOT intern anonymous function-pointer types: every
-- `ffi.cast("T(*)(...)", p)` parsed from a STRING registers a brand-new ctype
-- in an internal table hard-capped at 65,536 entries that is never garbage
-- collected, so any Runtime method called in a tight loop eventually aborts
-- the whole VM with "table overflow". Parse each HostApi function-pointer
-- type exactly ONCE here and cast through the cached ctype object at the call
-- sites (casting with a ctype object creates no new ctype).
local GET_ERROR_LEN_FN_T = ffi.typeof("size_t(*)(const HostApi*)")
local GET_LAST_ERROR_FN_T = ffi.typeof("size_t(*)(const HostApi*, uint8_t*, size_t)")
-- Out-param ABI: load_bundle and reload_bundle share one shape — they return
-- void and write their AbiError through a trailing AbiError* out-param.
local BUNDLE_PATH_FN_T = ffi.typeof("void(*)(const HostApi*, const uint8_t*, size_t, AbiError*)")
local UNLOAD_BUNDLE_FN_T = ffi.typeof("void(*)(const HostApi*, uint64_t, AbiError*)")
local FIND_GUEST_CONTRACT_FN_T = ffi.typeof("GuestContractHandle(*)(const HostApi*, uint64_t, uint32_t)")
local FIND_ALL_FN_T = ffi.typeof(M.FIND_ALL_FN_SIGNATURE)
local HOST_FREE_FN_T = ffi.typeof("void(*)(const HostApi*, void*, size_t, size_t)")
local RESOLVE_GUEST_CONTRACT_FN_T = ffi.typeof("const GuestContractInterface*(*)(const HostApi*, GuestContractHandle)")
local REGISTER_HOST_CONTRACT_FN_T = ffi.typeof(
    "void(*)(const HostApi*, const HostContractInterface*, AbiError*)"
)
local REGISTER_LOADER_FN_T = ffi.typeof("void(*)(const HostApi*, void*, AbiError*)")
local UINTPTR_T = ffi.typeof("uintptr_t")
local MAX_EXACT_LUA_INTEGER = 9007199254740992

local function pointer_token(value)
    local token = tonumber(ffi.cast(UINTPTR_T, value))
    if token == nil or token < 0 or token >= MAX_EXACT_LUA_INTEGER then
        error("polyplug: native pointer cannot be represented exactly by Lua", 3)
    end
    return token
end


local function bundle_id_from_parts(low, high)
    return ffi.new("uint64_t", low) + ffi.new("uint64_t", high) * ffi.new("uint64_t", 4294967296)
end

M.bundle_id = abi.bundle_id
M.host_contract_id = abi.host_contract_id

-- Compatibility modes matching polyplug_abi::Compatibility #[repr(u32)]
M.COMPATIBILITY_STRICT = 0
M.COMPATIBILITY_RELAXED = 1
M.COMPATIBILITY_YOLO = 2

local function get_lib()
    if not M._lib then
        error("polyplug: library not loaded. Call load_lib() first.")
    end
    return M._lib
end

--- Build the trusted-key allowlist cdata array and point a RuntimeConfig at it.
--
-- Accepts a sequence of 32-byte Ed25519 verifying keys, each either a Lua
-- string of length 32 or a sequence table of 32 byte values (0-255). The keys
-- are copied into a freshly allocated `Ed25519PublicKey[n]` array; the config's
-- `trusted_keys` / `trusted_keys_len` / `trusted_keys__align` fields are set to
-- address it.
--
-- OWNERSHIP: the runtime COPIES `config.trusted_keys` during
-- polyplug_runtime_create. The returned `buf` cdata only needs to stay
-- referenced across that one call (the caller keeps it in a local), after which
-- it may be reclaimed. An empty or nil list leaves the fields zero
-- (Trust-On-First-Use) and returns nil.
-- @param config cdata  A `RuntimeConfig` cdata to populate.
-- @param keys table|nil  Sequence of 32-byte keys (strings or byte tables).
-- @return cdata|nil      The `Ed25519PublicKey[n]` buffer (alive across create), or nil.
function M.build_trusted_keys(config, keys)
    if keys == nil or #keys == 0 then
        return nil
    end
    local count = #keys
    local buf = ffi.new("Ed25519PublicKey[?]", count)
    for i = 1, count do
        local key = keys[i]
        if type(key) == "string" then
            if #key ~= 32 then
                error("polyplug: trusted key #" .. i .. " must be 32 bytes, got " .. #key)
            end
            ffi.copy(buf[i - 1].bytes, key, 32)
        elseif type(key) == "table" then
            if #key ~= 32 then
                error("polyplug: trusted key #" .. i .. " must be 32 bytes, got " .. #key)
            end
            for b = 1, 32 do
                buf[i - 1].bytes[b - 1] = key[b]
            end
        else
            error("polyplug: trusted key #" .. i .. " must be a 32-byte string or table")
        end
    end
    config.trusted_keys = ffi.cast("void*", buf)
    config.trusted_keys_len = count
    config.trusted_keys__align = ffi.alignof("Ed25519PublicKey")
    return buf
end

function M.load_lib(so_path)
    M._lib = ffi.load(so_path)
    return M._lib
end

--- Get last error message from HostApi.
-- @param host HostApi*  The host interface pointer.
-- @param lib                  The library handle.
-- @return string              The error message, or empty string.
function M.last_error(host, lib)
    lib = lib or get_lib()
    -- Cast and call through HostApi.get_error_len field
    local get_error_len_fn = ffi.cast(GET_ERROR_LEN_FN_T, host.get_error_len)
    local len = get_error_len_fn(host)
    if len == 0 then
        return ""
    end
    local buf = ffi.new("uint8_t[?]", len)
    -- Cast and call through HostApi.get_last_error field
    local get_last_error_fn = ffi.cast(GET_LAST_ERROR_FN_T, host.get_last_error)
    get_last_error_fn(host, buf, len)
    return ffi.string(buf, len)
end

M.Runtime = {}
M.Runtime.__index = M.Runtime

--- Create a new Runtime instance.
-- Uses HostApi-based API: polyplug_runtime_create returns HostApi*.
--
-- All configuration is per-instance (Rule 12: no module statics shared
-- across runtimes). The reload-callback and log-callback FFI cdata are owned
-- by the returned Runtime and released on destroy().
-- @param opts table|nil  Optional options table:
--   opts.config        table     { compatibility = number, hot_reload_enabled = boolean }
--   opts.on_reload     function  Callback receiving a ReloadPhase table.
--   opts.log           function  Custom logger `function(level, scope, message)`
--                                receiving every runtime diagnostic; level is a
--                                number (see M.LogLevel), scope/message are Lua
--                                strings. Requires the polyplug_lua loader
--                                cdylib (the SDK routes through its exported
--                                log trampoline — LuaJIT callbacks cannot
--                                receive the ABI's by-value StringViews).
--   opts.log_max_level number    Maximum delivered level (see M.LogLevel);
--                                defaults to M.LogLevel.Warn. Only meaningful
--                                with opts.log.
--   opts.signature_policy number Bundle signature enforcement policy (see
--                                M.SignaturePolicy); defaults to
--                                M.SignaturePolicy.Off.
--   opts.trusted_keys  table     Sequence of 32-byte Ed25519 verifying keys
--                                (each a Lua string of length 32 or a table of
--                                32 byte values) forming the key-pinning
--                                allowlist. Empty/unset = Trust-On-First-Use.
--                                Only effective alongside a non-Off
--                                signature_policy. The runtime copies the keys
--                                during create, so the SDK only holds the cdata
--                                buffer across that one call.
-- @return Runtime             The runtime instance.
function M.Runtime.new(opts)
    local lib = get_lib()
    opts = opts or {}
    local host_ptr
    local reload_cb_cdata = nil
    local log_cb_cdata = nil
    local log_bridge = nil
    local trusted_keys_buf = nil

    if opts.config or opts.on_reload or opts.log or opts.signature_policy
        or opts.trusted_keys then
        -- Build a single RuntimeConfig; the on_reload callback pointer lives
        -- inside it — there is no separate options wrapper.
        local config_c = ffi.new("RuntimeConfig", {
            compatibility = (opts.config and opts.config.compatibility)
                or M.COMPATIBILITY_STRICT,
            hot_reload_enabled = (opts.config and opts.config.hot_reload_enabled)
                and 1 or 0,
            on_reload = nil,
            signature_policy = opts.signature_policy or M.SignaturePolicy.Off,
        })

        if opts.on_reload then
            local callback = opts.on_reload
            -- The ABI signature is void(*)(void* user_data, const ReloadPhase*):
            -- an opaque user-data pointer followed by a const pointer to the
            -- 48-byte ReloadPhase. The runtime always passes a non-null pointer;
            -- the pointee (and the StringViews inside it) is valid only for the
            -- duration of the call — all fields are copied into a Lua table
            -- before the user callback returns. user_data is unused here — the
            -- Lua closure already captures `callback`. The callback body is
            -- wrapped in pcall: a Lua error must never unwind across the C ABI
            -- mid-reload.
            reload_cb_cdata = ffi.cast("RuntimeConfig_on_reload_fn", function(
                _user_data,
                phase_ptr
            )
                local ok, err = pcall(function()
                    if phase_ptr == nil then
                        -- Contract: never happens. Defence-in-depth only.
                        return
                    end
                    -- phase_type is an enum cdata: normalise to a plain Lua
                    -- number so the table compares cleanly against the
                    -- reload_phase.TYPE_* constants. bundle_id stays a uint64
                    -- cdata (a Lua number would lose precision past 2^53);
                    -- field reads box a copy, so retaining it is safe.
                    local phase = reload_phase.new(
                        tonumber(phase_ptr.phase_type),
                        phase_ptr.bundle_id,
                        abi.to_str(phase_ptr.bundle_name),
                        abi.to_str(phase_ptr.reason)
                    )
                    callback(phase)
                end)
                if not ok then
                    io.stderr:write("polyplug: reload callback error: " .. tostring(err) .. "\n")
                end
            end)
            config_c.on_reload = reload_cb_cdata
        end

        if opts.log then
            -- Custom logger. The ABI signature for RuntimeConfig.log passes
            -- the scope/message StringViews BY VALUE (deliberate — hot path),
            -- which LuaJIT FFI callbacks cannot receive. The SDK therefore
            -- installs the native polyplug_lua_log_trampoline exported by the
            -- polyplug_lua loader cdylib as RuntimeConfig.log; it reads the
            -- PolyplugLuaLogBridge below from log_user_data and forwards the
            -- views as ptr+len scalars to this LuaJIT callback.
            --
            -- THREAD AFFINITY: the user function runs on whatever thread the
            -- runtime logs from (the RuntimeConfig.log contract is
            -- any-thread). Do not touch thread-affine state inside it, and
            -- never re-enter the runtime from the callback.
            --
            -- The callback body is wrapped in pcall: a logger must never
            -- crash the runtime, and a Lua error must never unwind across
            -- the C ABI.
            local log_fn = opts.log
            log_cb_cdata = ffi.cast("PolyplugLuaLogCallbackFn", function(
                _user_data, level, scope_ptr, scope_len, msg_ptr, msg_len
            )
                local ok, cb_err = pcall(function()
                    -- Copy the views into interned Lua strings BEFORE the user
                    -- fn runs: the pointers are only valid for this call.
                    local scope = scope_len > 0 and ffi.string(scope_ptr, scope_len) or ""
                    local message = msg_len > 0 and ffi.string(msg_ptr, msg_len) or ""
                    log_fn(tonumber(level), scope, message)
                end)
                if not ok then
                    io.stderr:write("polyplug: log callback error: " .. tostring(cb_err) .. "\n")
                end
            end)

            -- Resolve the trampoline from the polyplug_lua loader cdylib via
            -- the loaders module — the SAME clib handle the vm-dispatch host
            -- interface factories use (the module caches its ffi.load handle
            -- for the process lifetime, so the symbol stays valid). The
            -- require is lazy on purpose: hosts that never pass opts.log do
            -- not need the loaders package on package.path.
            local ok_tramp, trampoline = pcall(function()
                return require('polyplug.loaders.lua').bridge_lib().polyplug_lua_log_trampoline
            end)
            if not ok_tramp then
                log_cb_cdata:free()
                error("polyplug: opts.log requires the polyplug_lua loader cdylib "
                    .. "(set POLYPLUG_LUA_LIB and put the lua loaders package on "
                    .. "package.path): " .. tostring(trampoline))
            end

            log_bridge = ffi.new("PolyplugLuaLogBridge")
            log_bridge.callback = log_cb_cdata
            log_bridge.user_data = nil -- the Lua closure already captures log_fn

            config_c.log = trampoline
            config_c.log_user_data = log_bridge
            config_c.log_max_level = opts.log_max_level or M.LogLevel.Warn
        end

        -- Trusted-key allowlist (key pinning). The runtime copies
        -- config.trusted_keys during create, so trusted_keys_buf only needs to
        -- stay referenced (local) across the create call below.
        trusted_keys_buf = M.build_trusted_keys(config_c, opts.trusted_keys)

        -- config_c stays anchored (local) for the duration of the create call;
        -- the runtime copies what it needs before returning.
        host_ptr = lib.polyplug_runtime_create(config_c)
    else
        host_ptr = lib.polyplug_runtime_create(nil)
    end

    if host_ptr == nil then
        if reload_cb_cdata ~= nil then
            reload_cb_cdata:free()
        end
        if log_cb_cdata ~= nil then
            log_cb_cdata:free()
        end
        error("polyplug_runtime_create failed: returned null HostApi")
    end

    -- Dereference HostApi struct from pointer
    local host = host_ptr[0]
    local self = {
        _host = host_ptr,      -- HostApi* pointer (for FFI calls)
        _host_struct = host,   -- Dereferenced struct (for field access)
        _lib = lib,
        -- Owned reload-callback cdata: must outlive the native runtime (the
        -- runtime may invoke it until destroy), freed in destroy()/GC.
        _reload_cb_cdata = reload_cb_cdata,
        -- Owned log-callback cdata + bridge cdata (Rule 12: per-instance, not
        -- module state). RuntimeConfig.log_user_data points at _log_bridge and
        -- the runtime may log from any thread until destroy, so BOTH must stay
        -- anchored here for the runtime's lifetime; the callback slot is freed
        -- in destroy()/GC (the bridge cdata is reclaimed by the Lua GC once
        -- unreferenced).
        _log_cb_cdata = log_cb_cdata,
        _log_bridge = log_bridge,
        _destroyed = false
    }
    local obj = setmetatable(self, M.Runtime)

    -- Finalizer: release Lua-owned roots only after native ownership was consumed.
    ffi.gc(host_ptr, function(ptr)
        if not self._destroyed and ptr ~= nil then
            if lib.polyplug_runtime_destroy(ptr) then
                self._destroyed = true
                self._host = nil
                pending_finalizer_runtimes[tostring(ptr)] = nil
                if self._reload_cb_cdata ~= nil then
                    self._reload_cb_cdata:free()
                    self._reload_cb_cdata = nil
                end
                if self._log_cb_cdata ~= nil then
                    self._log_cb_cdata:free()
                    self._log_cb_cdata = nil
                end
                self._log_bridge = nil
            else
                pending_finalizer_runtimes[tostring(ptr)] = self
            end
        end
    end)
    return obj
end

--- Load a plugin bundle from path.
-- Calls through HostApi.load_bundle field.
-- @param path string  Path to bundle directory.
function M.Runtime:load_bundle(path)
    local path_str = tostring(path)
    local path_bytes = ffi.new("uint8_t[?]", #path_str, path_str)
    -- Out-param ABI: load_bundle returns void and writes its AbiError through
    -- the trailing out-param.
    local fn = ffi.cast(BUNDLE_PATH_FN_T, self._host_struct.load_bundle)
    local err = ffi.new("AbiError[1]")
    fn(self._host, path_bytes, #path_str, err)
    if err[0].code ~= ffi.C.AbiErrorCode_Ok then
        error("load_bundle failed: " .. M.last_error(self._host, self._lib))
    end
end

--- Reload a plugin bundle (hot-reload).
-- Calls through HostApi.reload_bundle field.
-- @param path string  Path to bundle directory.
function M.Runtime:reload_bundle(path)
    local path_str = tostring(path)
    local path_bytes = ffi.new("uint8_t[?]", #path_str, path_str)
    -- Out-param ABI: reload_bundle returns void and writes its AbiError through
    -- the trailing out-param.
    local fn = ffi.cast(BUNDLE_PATH_FN_T, self._host_struct.reload_bundle)
    local err = ffi.new("AbiError[1]")
    fn(self._host, path_bytes, #path_str, err)
    if err[0].code ~= ffi.C.AbiErrorCode_Ok then
        error("reload_bundle failed: " .. M.last_error(self._host, self._lib))
    end
end

--- Register a generated internal plugin through the native transaction gateway.
---
--- The resident has copied its manifest and owns all provider metadata before this
--- method begins. The gateway consumes it on every outcome and returns exact committed
--- handles in generated-provider order.
--- @return table `{ bundle_id = uint64, handles = GuestContractHandle[] }`.
function M.Runtime:register_internal_plugin(resident)
    if self._destroyed or self._host == nil then
        error("register_internal_plugin: runtime is destroyed", 2)
    end
    if type(resident) ~= "number" then
        error("register_internal_plugin: expected a generated native resident", 2)
    end

    local native_bridge = require("polyplug.loaders.lua").internal_plugin_bridge()
    local result = {
        native_bridge.register_transaction(
            resident,
            pointer_token(self._host),
            pointer_token(self._lib.polyplug_begin_internal_plugin),
            pointer_token(self._lib.polyplug_attach_internal_plugin_resident),
            pointer_token(self._lib.polyplug_current_os_thread_id),
            pointer_token(self._lib.polyplug_commit_internal_plugin_with_handles),
            pointer_token(self._lib.polyplug_abort_internal_plugin)
        )
    }
    if result[1] ~= 1 then
        error("register_internal_plugin failed: " .. M.last_error(self._host, self._lib), 2)
    end
    local handle_count = result[4]
    if type(handle_count) ~= "number" or handle_count < 0
        or #result ~= 4 + handle_count * 2 then
        error("register_internal_plugin returned an invalid handle list", 2)
    end
    local handles = {}
    for index = 1, handle_count do
        local offset = 5 + (index - 1) * 2
        handles[index] = {
            index = result[offset],
            generation = result[offset + 1],
        }
    end
    return {
        bundle_id = bundle_id_from_parts(result[2], result[3]),
        handles = handles,
    }
end


--- Unload a plugin bundle by bundle ID.
-- Calls through HostApi.unload_bundle field.
-- @param bundle_id number  Bundle identifier (uint64).
function M.Runtime:unload_bundle(bundle_id)
    -- Out-param ABI: unload_bundle returns void and writes its AbiError through
    -- the trailing out-param.
    local fn = ffi.cast(UNLOAD_BUNDLE_FN_T, self._host_struct.unload_bundle)
    local err = ffi.new("AbiError[1]")
    fn(self._host, bundle_id, err)
    if err[0].code ~= ffi.C.AbiErrorCode_Ok then
        error("unload_bundle failed: " .. M.last_error(self._host, self._lib))
    end
end

--- Find guest contract by contract_id and minimum version.
-- Calls through HostApi.find_guest_contract field.
-- @param contract_id number  Contract identifier hash.
-- @param min_version number  Minimum version required.
-- @return cdata              GuestContractHandle cdata (index + generation); a null
--                           handle has `.index == M.NULL_HANDLE_INDEX`.
function M.Runtime:find_guest_contract(contract_id, min_version)
    -- Cast function pointer and call with self-passing pattern.
    -- GuestContractHandle is `#[repr(C)] { index: u32, generation: u32 }` (8 bytes) and
    -- crosses the C ABI as the struct by value, returned by value here.
    local fn = ffi.cast(FIND_GUEST_CONTRACT_FN_T, self._host_struct.find_guest_contract)
    return fn(self._host, contract_id, min_version)
end

--- Find all guest contracts matching contract_id and minimum version.
-- Calls through HostApi.find_all_guest_contracts field.
-- @param contract_id number  Contract identifier hash.
-- @param min_version number  Minimum version required.
-- @param cap number          Maximum results to return (default 64).
-- @return table              Array of GuestContractHandle cdata (index + generation).
function M.Runtime:find_all_guest_contracts(contract_id, min_version, cap)
    cap = cap or 64
    -- Cast function pointer and call with self-passing pattern.
    -- Returns the ABI `Array` struct BY VALUE: #[repr(C)] { items: *mut T,
    -- len: usize, align: usize } = 24 bytes. Declaring anything smaller makes
    -- the SysV sret write past the buffer LuaJIT allocates for the return
    -- value (memory corruption). The element type is GuestContractHandle
    -- (#[repr(C)] { index: u32, generation: u32 } = 8 bytes / stride 8).
    local fn = ffi.cast(FIND_ALL_FN_T, self._host_struct.find_all_guest_contracts)
    local arr = fn(self._host, contract_id, min_version)
    local result = {}
    -- arr.len is a size_t cdata (uint64_t); math.min on cdata errors, so
    -- convert to a Lua number first.
    local len = tonumber(arr.len)
    local items = ffi.cast("GuestContractHandle*", arr.items)
    if items ~= nil then
        for i = 0, math.min(len, cap) - 1 do
            table.insert(result, items[i])
        end
    end
    -- Free the array via HostApi.free using the runtime's allocation size and
    -- alignment: size = len * sizeof(GuestContractHandle), align = arr.align.
    if arr.items ~= nil and len > 0 then
        local elem_size = ffi.sizeof("GuestContractHandle")
        local free_fn = ffi.cast(HOST_FREE_FN_T, self._host_struct.free)
        free_fn(self._host, arr.items, len * elem_size, arr.align)
    end
    return result
end

--- Resolve a guest contract handle to a GuestContractInterface pointer.
-- Calls through HostApi.resolve_guest_contract field. The returned cdata
-- exposes the full struct (dispatch_type, create_instance, destroy_instance,
-- dispatch) so callers can create instances and dispatch functions.
-- @param handle cdata  GuestContractHandle cdata (index + generation) from find_guest_contract.
-- @return cdata                const GuestContractInterface* pointer, or nil if invalid.
function M.Runtime:resolve_guest_contract(handle)
    -- Null handle sentinel is index == u32::MAX (0xFFFFFFFF).
    if handle == nil or handle.index == M.NULL_HANDLE_INDEX then
        return nil, "null handle"
    end
    -- Cast function pointer and call with self-passing pattern.
    -- GuestContractHandle is `#[repr(C)] { index: u32, generation: u32 }` (8 bytes) and
    -- crosses the C ABI as the struct passed by value.
    local fn = ffi.cast(RESOLVE_GUEST_CONTRACT_FN_T, self._host_struct.resolve_guest_contract)
    local interface = fn(self._host, handle)
    if interface == nil then
        return nil, M.last_error(self._host, self._lib)
    end
    return interface
end

--- Register a host contract interface with the runtime.
-- Calls through HostApi.register_host_contract field.
-- @param interface HostContractInterface*  Pointer to host contract interface.
function M.Runtime:register_host_contract(interface)
    if interface == nil then
        error("register_host_contract: null interface pointer")
    end
    -- Out-param ABI: register_host_contract returns void and writes its
    -- AbiError through the trailing out-param.
    local fn = ffi.cast(REGISTER_HOST_CONTRACT_FN_T, self._host_struct.register_host_contract)
    local err = ffi.new("AbiError[1]")
    fn(self._host, interface, err)
    if err[0].code ~= ffi.C.AbiErrorCode_Ok then
        error("register_host_contract failed: " .. M.last_error(self._host, self._lib))
    end
end

--- Register a language loader with the runtime.
-- Calls through HostApi.register_loader field. The loader pointer is the
-- opaque handle returned by a loader cdylib's `polyplug_<lang>_loader_create`.
-- @param loader_ptr cdata     Opaque loader pointer from the loader create function.
function M.Runtime:register_loader(loader_ptr)
    local fn = ffi.cast(REGISTER_LOADER_FN_T, self._host_struct.register_loader)
    local err = ffi.new("AbiError[1]")
    fn(self._host, loader_ptr, err)
    if err[0].code ~= ffi.C.AbiErrorCode_Ok then
        error("register_loader failed: " .. M.last_error(self._host, self._lib))
    end
end

--- Get the HostApi pointer.
-- @return HostApi*  The host interface pointer.
function M.Runtime:host()
    return self._host
end

--- Destroy the runtime explicitly.
--- Returns false without releasing callback or resident ownership when destruction
--- is rejected for this thread; invoke `destroy()` again on the owner thread.
--- @return boolean
function M.Runtime:destroy()
    if self._destroyed then
        return true
    end
    local host = self._host
    if host == nil or not self._lib.polyplug_runtime_destroy(host) then
        return false
    end
    self._destroyed = true
    self._host = nil
    pending_finalizer_runtimes[tostring(host)] = nil
    if self._reload_cb_cdata ~= nil then
        self._reload_cb_cdata:free()
        self._reload_cb_cdata = nil
    end
    if self._log_cb_cdata ~= nil then
        self._log_cb_cdata:free()
        self._log_cb_cdata = nil
    end
    self._log_bridge = nil
    return true
end

return M
