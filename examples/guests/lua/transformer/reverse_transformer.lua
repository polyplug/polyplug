-- examples/guests/lua/transformer/reverse_transformer.lua
--
-- Transformer — Lua plugin implementing data.Transformer@1
-- Contract: transform(input: string) -> string
-- Returns: "lua:transform({input})"
local ffi = require("ffi")
local polyplug_guest = require("polyplug_guest")

-- Contract ID: fnv1a_64("data.Transformer@1") = 0x3D53C682F3F5A9EF
local TRANSFORMER_CONTRACT_ID_HI = 0x3D53C682  -- upper 32 bits
local TRANSFORMER_CONTRACT_ID_LO = 0xF3F5A9EF  -- lower 32 bits

-- Implementation: transform(args: StringView*, out: StringView*)
local function impl_transform(args_ptr, out_ptr)
    -- SAFETY: args_ptr and out_ptr are valid StringView pointers per ABI contract.
    -- The host runtime allocates both buffers and guarantees alignment to 8 bytes.
    local args = ffi.cast("StringView*", ffi.cast("uintptr_t", args_ptr))
    local out  = ffi.cast("StringView*", ffi.cast("uintptr_t", out_ptr))

    local input_str = ffi.string(args.ptr, args.len)
    local result_str = "lua:transform(" .. input_str .. ")"

    local sv = polyplug_guest.string_view(result_str)
    out.ptr = sv.ptr
    out.len = sv.len

    return 0  -- ABI_OK
end

function polyplug_init(_registrar_ptr_int, _ctx_ptr)
    _G._polyplug_handlers = {
        contract_name    = "data.Transformer",
        contract_id_hex  = "3D53C682F3F5A9EF",
        contract_version = 1,
        plugin_name      = "transformer-lua",
        functions = {
            [0] = impl_transform,
        },
    }
end
