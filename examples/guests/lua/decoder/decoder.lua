local ffi = require('ffi')
local polyplug = require('polyplug_guest')

local function decode(input)
    local s = polyplug.to_str(input):gsub(',', '|')
    return polyplug.alloc_string('DECODED:' .. s)
end

return {
    ['pipeline.Decoder'] = { decode = decode },
}
