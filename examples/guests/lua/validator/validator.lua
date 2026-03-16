-- examples/guests/lua/validator/validator.lua
--
-- Validator — Lua plugin implementing pipeline.Validator@1
-- Contract: validate(data: StringView) -> StringView
-- Input:  "DECODED:name|value|42"
-- Output: "VALID:name|value|42" or "INVALID:reason"
local ffi = require("ffi")
local polyplug_guest = require("polyplug_guest")

-- Contract ID: fnv1a_64("pipeline.Validator@1") = 0xA553FAB5D11C7AF0
local VALIDATOR_CONTRACT_ID_HI = 0xA553FAB5  -- upper 32 bits
local VALIDATOR_CONTRACT_ID_LO = 0xD11C7AF0  -- lower 32 bits

-- Implementation: validate(args: StringView*, out: StringView*)
local function impl_validate(args_ptr, out_ptr)
    -- SAFETY: args_ptr and out_ptr are valid StringView pointers per ABI contract.
    -- The host runtime allocates both buffers and guarantees alignment to 8 bytes.
    local args = ffi.cast("StringView*", ffi.cast("uintptr_t", args_ptr))
    local out  = ffi.cast("StringView*", ffi.cast("uintptr_t", out_ptr))

    local input_str = ffi.string(args.ptr, args.len)

    local result_str
    local prefix = "DECODED:"
    if input_str:sub(1, #prefix) == prefix then
        local payload = input_str:sub(#prefix + 1)
        local parts = {}
        for part in payload:gmatch("[^|]+") do
            parts[#parts + 1] = part
        end
        if #parts == 3 and tonumber(parts[3]) ~= nil then
            result_str = "VALID:" .. payload
        else
            result_str = "INVALID:expected 3 pipe-separated fields with numeric third field"
        end
    else
        result_str = "INVALID:missing DECODED: prefix"
    end

    local sv = polyplug_guest.string_view(result_str)
    out.ptr = sv.ptr
    out.len = sv.len

    return 0  -- ABI_OK
end

function polyplug_init(_registrar_ptr_int, _ctx_ptr)
    _G._polyplug_handlers = {
        contract_name    = "pipeline.Validator",
        contract_id_hex  = "A553FAB5D11C7AF0",
        contract_version = 1,
        plugin_name      = "validator-lua",
        functions = {
            [0] = impl_validate,
        },
    }
end
