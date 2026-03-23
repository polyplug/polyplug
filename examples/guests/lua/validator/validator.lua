local ffi = require('ffi')
local polyplug = require('polyplug_guest')
local abi = require('polyplug_abi')
local contracts = require('generated.guest.contracts')

local function validate(input)
    local s = polyplug.to_str(input)
    s = abi.strip_prefix(s, "DECODED:")
    local parts = abi.split(s, "|")
    if #parts == 3 and parts[1] ~= '' and parts[2] ~= '' and tonumber(parts[3]) then
        return polyplug.alloc_string('VALID:' .. s)
    end
    return polyplug.alloc_string('INVALID:expected name|value|count')
end

contracts.set_validator_impl(validate)

return contracts