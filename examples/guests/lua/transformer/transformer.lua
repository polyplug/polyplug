local ffi = require('ffi')
local polyplug = require('polyplug_guest')
local contracts = require('generated.guest.contracts')

local function transform(input)
    local s = polyplug.to_str(input)
    if s:sub(1, 8) == 'DECODED:' then s = s:sub(9) end
    local parts = {}
    for part in s:gmatch('[^|]+') do table.insert(parts, part) end
    if #parts >= 3 then
        local name = parts[1]:upper()
        local value = parts[2] .. ' (transformed)'
        local count = tonumber(parts[3]) + 1
        return polyplug.alloc_string(string.format('TRANSFORMED:%s|%s|%d', name, value, count))
    end
    return polyplug.alloc_string('INVALID:format')
end

contracts.set_transformer_impl(transform)

return contracts