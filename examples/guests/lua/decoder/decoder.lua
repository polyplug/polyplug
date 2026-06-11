local ffi = require('ffi')
local polyplug = require('polyplug_guest')
local polyplug_abi = require('polyplug_abi')
local contracts = require('generated.guest.contracts')

-- One-time Info log into the host's logging funnel (RuntimeConfig.log) on the
-- first decode. Guarded so hot dispatch paths pay only a local boolean check;
-- hosts without a custom logger drop Info silently (default sink is
-- Error/Warn to stderr), so cross-host stdout parity is unaffected.
local logged_online = false

local function decode(input)
    if not logged_online then
        logged_online = true
        polyplug.log(polyplug.LogLevel.Info, 'guest.lua_decoder', 'decoder online')
    end
    local s = polyplug_abi.to_str(input):gsub(',', '|')
    return polyplug.alloc_string('DECODED:' .. s)
end

contracts.set_decoder_impl(decode)

return contracts