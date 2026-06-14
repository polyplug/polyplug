local ffi = require('ffi')
local polyplug = require('polyplug_guest')
local polyplug_abi = require('polyplug_abi')
local contracts = require('generated.guest.contracts')

-- Factory returning a per-instance reporter. The reporter is stateless, so each
-- instance is a fresh object whose method is the contract function.
local function new_reporter(host)
    local self = {}

    function self:report(input)
        local s = polyplug_abi.to_str(input)
        if s:sub(1, 12) == 'TRANSFORMED:' then s = s:sub(13) end
        local name, value, count = s:match('^([^|]*)|([^|]*)|([^|]*)$')
        if name then
            return polyplug.alloc_string(string.format(
                'Report: %s has value \'%s\' with count %s', name, value, count))
        end
        return polyplug.alloc_string('INVALID:format')
    end

    return self
end

contracts.set_reporter_factory(new_reporter)

return contracts
