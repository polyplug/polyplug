-- sdks/lua/host/polyplug/reload_phase.lua
-- ReloadPhase type for hot-reload notifications.

local M = {}

--- Phase type constants (match FFI callback phase_type parameter)
M.TYPE_PREPARING = 0  -- Before vtable swap, host should cleanup instances
M.TYPE_RELOADED = 1   -- After vtable swap, instances can be re-resolved
M.TYPE_FAILED = 2     -- Reload aborted after max retries

--- Create a new ReloadPhase instance.
--- @param phase_type number Phase type (TYPE_PREPARING, TYPE_RELOADED, or TYPE_FAILED)
--- @param bundle_id number FNV-1a 64-bit hash of the bundle name
--- @param bundle_name string Human-readable bundle name
--- @param retry_count number Current retry attempt (0-indexed, only for Preparing)
--- @param reason string Error reason (only for Failed phase)
--- @return table ReloadPhase instance
function M.new(phase_type, bundle_id, bundle_name, retry_count, reason)
    return {
        type = phase_type,
        bundle_id = bundle_id,
        bundle_name = bundle_name or "",
        retry_count = retry_count or 0,
        reason = reason or "",
    }
end

--- Check if this phase is Preparing.
--- @param phase table ReloadPhase instance
--- @return boolean
function M.is_preparing(phase)
    return phase.type == M.TYPE_PREPARING
end

--- Check if this phase is Reloaded.
--- @param phase table ReloadPhase instance
--- @return boolean
function M.is_reloaded(phase)
    return phase.type == M.TYPE_RELOADED
end

--- Check if this phase is Failed.
--- @param phase table ReloadPhase instance
--- @return boolean
function M.is_failed(phase)
    return phase.type == M.TYPE_FAILED
end

--- Get string representation of phase type.
--- @param phase_type number Phase type constant
--- @return string Human-readable phase name
function M.phase_type_name(phase_type)
    local names = {
        [M.TYPE_PREPARING] = "Preparing",
        [M.TYPE_RELOADED] = "Reloaded",
        [M.TYPE_FAILED] = "Failed",
    }
    return names[phase_type] or ("Unknown(" .. tostring(phase_type) .. ")")
end

return M