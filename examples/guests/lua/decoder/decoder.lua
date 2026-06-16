local ffi = require('ffi')
local polyplug = require('polyplug_guest')
local polyplug_abi = require('polyplug_abi')
local contracts = require('generated.guest.contracts')

-- Factory returning a per-instance decoder. The loader calls this once per
-- instance (and once at load for the stateless default impl), so per-instance
-- state lives on `self` and is never shared across instances.
local function new_decoder(host)
    local self = {}

    -- One-time Info log into the host's logging funnel (RuntimeConfig.log) on the
    -- first decode of THIS instance. Guarded so hot dispatch paths pay only a
    -- local boolean check; hosts without a custom logger drop Info silently
    -- (default sink is Error/Warn to stderr), so cross-host stdout parity is
    -- unaffected.
    self.logged_online = false

    function self:decode(input)
        if not self.logged_online then
            self.logged_online = true
            polyplug.log(host, polyplug.LogLevel.Info, 'guest.lua_decoder', 'decoder online')
        end
        local s = polyplug_abi.to_str(input):gsub(',', '|')
        return 'DECODED:' .. s
    end

    return self
end

contracts.set_decoder_factory(new_decoder)

return contracts
