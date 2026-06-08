local ffi = require('ffi')
local polyplug = require('polyplug_guest')
local contracts = require('generated.guest.contracts')

local function transform(input)
    local s = polyplug.to_str(input)
    if s:sub(1, 8) == 'DECODED:' then s = s:sub(9) end
    local name, value, count = s:match('^([^|]*)|([^|]*)|([^|]*)$')
    if name and count and tonumber(count) then
        return polyplug.alloc_string(string.format(
            'TRANSFORMED:%s|%s (transformed)|%d', name:upper(), value, tonumber(count) + 1))
    end
    return polyplug.alloc_string('INVALID:format')
end

contracts.set_transformer_impl(transform)

return contracts