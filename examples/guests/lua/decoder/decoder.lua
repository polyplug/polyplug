-- Lua decoder plugin — implements pipeline.Decoder@1
-- Input:  "name,value,42"
-- Output: "DECODED:name|value|42"

local contracts = require("generated.guest.contracts")

local function decode(input_sv)
    local input_str = input_sv.str
    local parts = {}
    for part in input_str:gmatch("[^,]+") do
        table.insert(parts, part)
    end
    local joined = table.concat(parts, "|")
    local result = "DECODED:" .. joined
    return result
end

-- Register implementation
contracts.set_lua_decoder_impl(decode)
