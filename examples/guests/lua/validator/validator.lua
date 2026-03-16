-- Lua validator plugin — implements pipeline.Validator@1
-- Input:  "name,value,42"
-- Output: "VALID:name,value,42" or error

local contracts = require("generated.guest.contracts")

local function validate(data_sv)
    local data_str = data_sv.str
    local count = 0
    for _ in data_str:gmatch("[^,]+") do
        count = count + 1
    end
    if count ~= 3 then
        error("invalid format: expected 3 fields")
    end
    return "VALID:" .. data_str
end

contracts.set_lua_validator_impl(validate)
