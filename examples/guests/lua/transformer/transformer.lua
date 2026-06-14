local ffi = require('ffi')
local polyplug = require('polyplug_guest')
local polyplug_abi = require('polyplug_abi')
local contracts = require('generated.guest.contracts')

-- Factory returning a per-instance transformer. The transformer is stateless, so
-- each instance is a fresh object whose method is the contract function.
local function new_transformer(host)
    local self = {}

    function self:transform(input)
        local s = polyplug_abi.to_str(input)
        if s:sub(1, 8) == 'DECODED:' then s = s:sub(9) end
        local name, value, count = s:match('^([^|]*)|([^|]*)|([^|]*)$')
        if name and count and tonumber(count) then
            return polyplug.alloc_string(string.format(
                'TRANSFORMED:%s|%s (transformed)|%d', name:upper(), value, tonumber(count) + 1))
        end
        return polyplug.alloc_string('INVALID:format')
    end

    return self
end

contracts.set_transformer_factory(new_transformer)

return contracts
