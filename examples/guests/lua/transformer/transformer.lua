local ffi = require('ffi')
local polyplug = require('polyplug_guest')
local abi = require('polyplug_abi')
local contracts = require('generated.guest.contracts')

local function transform(input)
    local s = polyplug.to_str(input)
    s = abi.strip_prefix(s, "DECODED:")
    local parts = abi.split(s, "|")
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