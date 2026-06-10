local ffi = require('ffi')
local polyplug = require('polyplug_guest')
local polyplug_abi = require('polyplug_abi')
local contracts = require('generated.guest.contracts')

local function encode(input)
    local s = polyplug_abi.to_str(input)
    if s:sub(1, 12) == 'TRANSFORMED:' then s = s:sub(13) end
    return polyplug.alloc_string(s:gsub('|', ','))
end

contracts.set_encoder_impl(encode)

return contracts