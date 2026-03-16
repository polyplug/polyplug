-- Lua encoder plugin — implements pipeline.Encoder@1
-- Input:  "name|value|42"
-- Output: "ENCODED:name,value,42"

local contracts = require("generated.guest.contracts")

local function encode(data_sv)
    local data_str = data_sv.str
    local result = "ENCODED:" .. data_str:gsub("|", ",")
    return result
end

contracts.set_lua_encoder_impl(encode)
