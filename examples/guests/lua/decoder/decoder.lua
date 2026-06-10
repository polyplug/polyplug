local ffi = require('ffi')
local polyplug = require('polyplug_guest')
local polyplug_abi = require('polyplug_abi')
local contracts = require('generated.guest.contracts')

local function decode(input)
    local s = polyplug_abi.to_str(input):gsub(',', '|')
    return polyplug.alloc_string('DECODED:' .. s)
end

contracts.set_decoder_impl(decode)

return contracts