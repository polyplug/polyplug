local ffi = require('ffi')
local polyplug = require('polyplug_guest')
local abi = require('polyplug_abi')
local contracts = require('generated.guest.contracts')

local function report(input)
    local s = polyplug.to_str(input)
    s = abi.strip_prefix(s, "TRANSFORMED:")
    local parts = abi.split(s, "|")
    if #parts >= 3 then
        return polyplug.alloc_string(string.format('Report: %s has value \'%s\' with count %s', parts[1], parts[2], parts[3]))
    end
    return polyplug.alloc_string('INVALID:format')
end

contracts.set_reporter_impl(report)

return contracts