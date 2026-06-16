local ffi = require('ffi')
local polyplug = require('polyplug_guest')
local polyplug_abi = require('polyplug_abi')
local contracts = require('generated.guest.contracts')

-- Factory returning a per-instance encoder. The encoder is stateless, so each
-- instance is a fresh object whose method is the contract function.
local function new_encoder(host)
    local self = {}

    function self:encode(input)
        local s = polyplug_abi.to_str(input)
        if s:sub(1, 12) == 'TRANSFORMED:' then s = s:sub(13) end
        return (s:gsub('|', ','))
    end

    return self
end

contracts.set_encoder_factory(new_encoder)

return contracts
