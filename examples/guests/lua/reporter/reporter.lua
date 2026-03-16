-- Lua reporter plugin — implements data.Reporter@1
-- Input:  "name,value,42"
-- Output: "REPORTED:name|value|42"

local contracts = require("generated.guest.contracts")

local function report(data_sv)
    local data_str = data_sv.str
    local result = "REPORTED:" .. data_str:gsub(",", "|")
    return result
end

contracts.set_lua_reporter_impl(report)
