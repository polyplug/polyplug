local ffi = require('ffi')
local polyplug = require('polyplug_guest')
local contracts = require('generated.guest.contracts')

local function report(input)
    local s = polyplug.to_str(input)
    if s:sub(1, 12) == 'TRANSFORMED:' then s = s:sub(13) end
    local name, value, count = s:match('^([^|]*)|([^|]*)|([^|]*)$')
    if name then
        return polyplug.alloc_string(string.format(
            'Report: %s has value \'%s\' with count %s', name, value, count))
    end
    return polyplug.alloc_string('INVALID:format')
end

contracts.set_reporter_impl(report)

return contracts