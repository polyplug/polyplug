-- examples/guests/lua/decoder/decoder.lua
--
-- Decoder — Lua plugin implementing pipeline.Decoder@1
-- Contract: decode(input: string) -> string
-- Input:  "name,value,42"
-- Output: "DECODED:name|value|42"
local ffi = require("ffi")
local polyplug_guest = require("polyplug_guest")

-- Contract ID: fnv1a_64("pipeline.Decoder@1") = 0x12F3C106B0C3DC1E
local DECODER_CONTRACT_ID_HI = 0x12F3C106  -- upper 32 bits
local DECODER_CONTRACT_ID_LO = 0xB0C3DC1E  -- lower 32 bits

-- Implementation: decode(args: StringView*, out: StringView*)
local function impl_decode(args_ptr, out_ptr)
    -- SAFETY: args_ptr and out_ptr are valid StringView pointers per ABI contract.
    -- The host runtime allocates both buffers and guarantees alignment to 8 bytes.
    local args = ffi.cast("StringView*", ffi.cast("uintptr_t", args_ptr))
    local out  = ffi.cast("StringView*", ffi.cast("uintptr_t", out_ptr))

    local input_str = ffi.string(args.ptr, args.len)

    -- Parse "name,value,42" -> replace commas with pipes, prefix with DECODED:
    local decoded_str = "DECODED:" .. input_str:gsub(",", "|")

    local sv = polyplug_guest.string_view(decoded_str)
    out.ptr = sv.ptr
    out.len = sv.len

    return 0  -- ABI_OK
end

function polyplug_init(_registrar_ptr_int, _ctx_ptr)
    _G._polyplug_handlers = {
        contract_name    = "pipeline.Decoder",
        contract_id_hex  = "12F3C106B0C3DC1E",
        contract_version = 1,
        plugin_name      = "decoder-lua",
        functions = {
            [0] = impl_decode,
        },
    }
end
