local ffi = require('ffi')
local polyplug = require('polyplug_guest')
local abi = require('polyplug_abi')
local contracts = require('generated.guest.contracts')
local callers = require('generated.guest.host_contract_callers')

local function do_work(input)
    local s = polyplug.to_str(input)

    local logger = callers.HostLoggerCaller.from_host(abi.get_host_vtable(), 1)

    if logger and logger:is_valid() then
        logger:log('Processing input: ' .. s)
        logger:log('Step 1: Analyzing input')
        logger:log('Step 2: Transforming data')
        logger:log('Step 3: Generating output')
    end

    return polyplug.alloc_string('WORKED: ' .. string.upper(s))
end

contracts.set_worker_impl(do_work)

return contracts