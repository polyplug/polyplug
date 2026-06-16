local ffi = require('ffi')
local polyplug = require('polyplug_guest')
local polyplug_abi = require('polyplug_abi')
local contracts = require('generated.guest.contracts')

-- Factory returning a per-instance validator. The validator is stateless, so each
-- instance is a fresh object whose method is the contract function.
local function new_validator(host)
    local self = {}

    function self:validate(input)
        local s = polyplug_abi.to_str(input)
        if s:sub(1, 8) == 'DECODED:' then s = s:sub(9) end
        local name, value, count = s:match('^([^|]*)|([^|]*)|([^|]*)$')
        if name and name ~= '' and value ~= '' and tonumber(count) then
            return 'VALID:' .. s
        end
        return 'INVALID:expected name|value|count'
    end

    return self
end

contracts.set_validator_factory(new_validator)

return contracts
