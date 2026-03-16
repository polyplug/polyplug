-- examples/guests/lua/encoder/encoder.lua
--
-- Encoder — Lua plugin implementing pipeline.Encoder@1
-- Contract: encode(data: StringView) -> StringView
-- Input:  "TRANSFORMED:NAME|value (transformed)|43"
-- Output: "NAME,value (transformed),43"
local ffi = require("ffi")
local polyplug_guest = require("polyplug_guest")

-- Contract ID: fnv1a_64("pipeline.Encoder@1") = 0x127D1703C6EFB432
local ENCODER_CONTRACT_ID_HI = 0x127D1703  -- upper 32 bits
local ENCODER_CONTRACT_ID_LO = 0xC6EFB432  -- lower 32 bits

-- Implementation: encode(args: StringView*, out: StringView*)
local function impl_encode(args_ptr, out_ptr)
    -- SAFETY: args_ptr and out_ptr are valid StringView pointers per ABI contract.
    -- The host runtime allocates both buffers and guarantees alignment to 8 bytes.
    local args = ffi.cast("StringView*", ffi.cast("uintptr_t", args_ptr))
    local out  = ffi.cast("StringView*", ffi.cast("uintptr_t", out_ptr))

    local data_str = ffi.string(args.ptr, args.len)

    local prefix = "TRANSFORMED:"
    local payload = data_str
    if data_str:sub(1, #prefix) == prefix then
        payload = data_str:sub(#prefix + 1)
    end

    local result_str = payload:gsub("|", ",")

    local sv = polyplug_guest.string_view(result_str)
    out.ptr = sv.ptr
    out.len = sv.len

    return 0  -- ABI_OK
end

function polyplug_init(_registrar_ptr_int, _ctx_ptr)
    _G._polyplug_handlers = {
        contract_name    = "pipeline.Encoder",
        contract_id_hex  = "127D1703C6EFB432",
        contract_version = 1,
        plugin_name      = "encoder-lua",
        functions = {
            [0] = impl_encode,
        },
    }
end
