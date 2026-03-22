local ffi = require('ffi')
local polyplug = require('polyplug_guest')
local contracts = require('generated.guest.contracts')

local function validate(input)
    local s = polyplug.to_str(input)
    if s:sub(1, 8) == 'DECODED:' then s = s:sub(9) end
    local parts = {}
    for part in s:gmatch('[^|]+') do table.insert(parts, part) end
    if #parts == 3 and parts[1] ~= '' and parts[2] ~= '' and tonumber(parts[3]) then
        return polyplug.alloc_string('VALID:' .. s)
    end
    return polyplug.alloc_string('INVALID:expected name|value|count')
end

contracts.set_validator_impl(validate)

return contracts