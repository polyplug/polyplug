local ffi = require('ffi')
local polyplug = require('polyplug_guest')
local polyplug_abi = require('polyplug_abi')
local contracts = require('generated.guest.contracts')

local function validate(input)
    local s = polyplug_abi.to_str(input)
    if s:sub(1, 8) == 'DECODED:' then s = s:sub(9) end
    local name, value, count = s:match('^([^|]*)|([^|]*)|([^|]*)$')
    if name and name ~= '' and value ~= '' and tonumber(count) then
        return polyplug.alloc_string('VALID:' .. s)
    end
    return polyplug.alloc_string('INVALID:expected name|value|count')
end

contracts.set_validator_impl(validate)

return contracts