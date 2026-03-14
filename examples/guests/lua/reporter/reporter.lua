-- examples/guests/lua/reporter/reporter.lua
--
-- Reporter — Lua plugin implementing data.Reporter@1
-- Contract: report(value: string) -> string
-- Returns: "lua:report({value})"
local ffi = require("ffi")
local polyplug_guest = require("polyplug_guest")

-- Contract ID: fnv1a_64("data.Reporter@1") = 0x81D41D43E511D297
local REPORTER_CONTRACT_ID_HI = 0x81D41D43  -- upper 32 bits
local REPORTER_CONTRACT_ID_LO = 0xE511D297  -- lower 32 bits

-- Implementation: report(args: StringView*, out: StringView*)
local function impl_report(args_ptr, out_ptr)
    -- SAFETY: args_ptr and out_ptr are valid StringView pointers per ABI contract.
    -- The host runtime allocates both buffers and guarantees alignment to 8 bytes.
    local args = ffi.cast("StringView*", ffi.cast("uintptr_t", args_ptr))
    local out  = ffi.cast("StringView*", ffi.cast("uintptr_t", out_ptr))

    local value_str = ffi.string(args.ptr, args.len)
    local result_str = "lua:report(" .. value_str .. ")"

    local sv = polyplug_guest.string_view(result_str)
    out.ptr = sv.ptr
    out.len = sv.len

    return 0  -- ABI_OK
end

function polyplug_init(_registrar_ptr_int, _ctx_ptr)
    _G._polyplug_handlers = {
        contract_name    = "data.Reporter",
        contract_id_hex  = "81D41D43E511D297",
        contract_version = 1,
        plugin_name      = "reporter-lua",
        functions = {
            [0] = impl_report,
        },
    }
end
