-- sdks/lua/host/polyplug/runtime_config.lua
-- Runtime configuration options for hot-reload behavior and other settings.

local M = {}

--- Default configuration values
local DEFAULT_MAX_RETRIES = 3
local DEFAULT_RETRY_INTERVAL_MS = 1000
local DEFAULT_ABORT_ON_MAX_RETRIES = true

--- Create a new RuntimeConfig instance.
--- @param opts table|nil Optional configuration overrides
--- @return table RuntimeConfig instance
function M.new(opts)
    opts = opts or {}
    return {
        --- Maximum number of retry attempts for hot-reload operations.
        --- Default: 3. Set to 0 for infinite retries (when abort_on_max_retries is false).
        hot_reload_max_retries = opts.hot_reload_max_retries or DEFAULT_MAX_RETRIES,

        --- Interval between retry attempts for hot-reload operations (milliseconds).
        --- Default: 1000 (1 second).
        hot_reload_retry_interval_ms = opts.hot_reload_retry_interval_ms or DEFAULT_RETRY_INTERVAL_MS,

        --- Whether to abort hot-reload after exhausting max_retries.
        --- If true (default): abort and fire Failed notification.
        --- If false: keep retrying forever.
        hot_reload_abort_on_max_retries = opts.hot_reload_abort_on_max_retries ~= nil and opts.hot_reload_abort_on_max_retries or DEFAULT_ABORT_ON_MAX_RETRIES,
    }
end

return M