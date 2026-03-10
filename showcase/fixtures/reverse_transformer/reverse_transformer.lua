-- showcase/plugins/reverse_transformer/reverse_transformer.lua
--
-- Reverse Transformer — Lua plugin implementing pipeline.transformer@1
-- Demonstrates:
--   1. Lua plugin with LuaJIT FFI + _G._polyplug_handlers pattern
--   2. Relaxed compatibility: 2 functions defined, contract expects 1 → warning emitted
local ffi = require("ffi")
local polyplug_guest = require("polyplug_guest")

-- Contract ID: fnv1a_64("pipeline.transformer@1") = 0x0E3044133E12EB05
local TRANSFORMER_CONTRACT_ID_HI = 0x0E304413  -- upper 32 bits
local TRANSFORMER_CONTRACT_ID_LO = 0x3E12EB05  -- lower 32 bits

-- ABI type definitions (guarded via polyplug_guest cdef_guarded; DataRecord is plugin-specific)
local function cdef_guarded(decl)
    local ok, err = pcall(ffi.cdef, decl)
    if not ok and not string.find(err, "already defined", 1, true) then
        error(err, 2)
    end
end

cdef_guarded([[
    typedef struct {
        struct { const uint8_t* ptr; size_t len; } name;
        struct { const uint8_t* ptr; size_t len; } value;
        uint32_t count;
        uint32_t _pad;
    } DataRecord;
]])

-- Implementation: transform(args: DataRecord, out: DataRecord)
-- Reverses each string field (name and value) using Lua string.reverse
local function impl_transform(args_ptr, out_ptr)
    -- SAFETY: args_ptr and out_ptr are valid DataRecord pointers per ABI contract.
    -- The host runtime allocates both buffers and guarantees alignment to 8 bytes.
    local args = ffi.cast("DataRecord*", ffi.cast("uintptr_t", args_ptr))
    local out  = ffi.cast("DataRecord*", ffi.cast("uintptr_t", out_ptr))

    -- Extract strings from StringView fields
    local name_str  = ffi.string(args.name.ptr,  args.name.len)
    local value_str = ffi.string(args.value.ptr, args.value.len)

    -- Reverse the strings (Lua string.reverse for ASCII)
    local rev_name  = name_str:reverse()
    local rev_value = value_str:reverse()

    -- Write to output using polyplug_guest.string_view() to create new StringView
    local sv_name  = polyplug_guest.string_view(rev_name)
    local sv_value = polyplug_guest.string_view(rev_value)
    out.name.ptr  = sv_name.ptr
    out.name.len  = sv_name.len
    out.value.ptr = sv_value.ptr
    out.value.len = sv_value.len
    out.count     = args.count  -- count unchanged

    return 0  -- ABI_OK
end

-- Extra function (for Relaxed compat scenario):
-- Reverses each space-separated word in name and value.
-- This is the EXTRA function: contract expects 1, we provide 2 → Relaxed warning.
local function impl_reverse_words(args_ptr, out_ptr)
    -- SAFETY: args_ptr and out_ptr are valid DataRecord pointers per ABI contract.
    -- The host runtime allocates both buffers and guarantees alignment to 8 bytes.
    local args = ffi.cast("DataRecord*", ffi.cast("uintptr_t", args_ptr))
    local out  = ffi.cast("DataRecord*", ffi.cast("uintptr_t", out_ptr))

    local name_str  = ffi.string(args.name.ptr,  args.name.len)
    local value_str = ffi.string(args.value.ptr, args.value.len)

    -- Reverse word order (split on spaces, reverse, rejoin)
    local function reverse_words(s)
        local words = {}
        for w in s:gmatch("%S+") do
            table.insert(words, 1, w)  -- insert at front = reverse order
        end
        return table.concat(words, " ")
    end

    local sv_name  = polyplug_guest.string_view(reverse_words(name_str))
    local sv_value = polyplug_guest.string_view(reverse_words(value_str))
    out.name.ptr  = sv_name.ptr
    out.name.len  = sv_name.len
    out.value.ptr = sv_value.ptr
    out.value.len = sv_value.len
    out.count     = args.count

    return 0
end

-- polyplug_init is called by LuaLoader with the PluginRegistrar pointer as i64.
-- It does NOT call register_plugin directly — the LuaLoader (Rust) does that
-- after reading _G._polyplug_handlers and creating Rust-side trampolines.
function polyplug_init(_registrar_ptr_int)
    -- LuaLoader reads _G._polyplug_handlers instead of calling register_plugin directly.
    -- This is the exact pattern from tests/fixtures/test_plugin.lua.
    _G._polyplug_handlers = {
        contract_name    = "pipeline.transformer",
        contract_id_hex  = "0E3044133E12EB05",  -- TRANSFORMER_CONTRACT_ID without 0x prefix
        contract_version = 1,  -- major version = 1 (used by LuaLoader for contract_id computation)
        plugin_name      = "reverse-transformer-lua",
        -- Functions in declaration order (function_id order):
        --   [0] = transform        (contract function)
        --   [1] = reverse_words    (extra function — Relaxed compat scenario)
        functions = {
            [0] = impl_transform,       -- function_id 0: transform (contract function)
            [1] = impl_reverse_words,   -- function_id 1: extra function (Relaxed compat scenario)
        },
    }
end
