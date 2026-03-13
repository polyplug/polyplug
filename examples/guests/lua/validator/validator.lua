-- examples/guests/lua/validator/validator.lua
--
-- Field Validator — Lua plugin implementing pipeline.validator@1
-- Demonstrates: Lua plugin with LuaJIT FFI + _G._polyplug_handlers pattern
--
-- Validation rules:
--   1. name must be non-empty
--   2. value must be non-empty
--   3. name and value must contain only printable ASCII (0x20..0x7E)
--   4. count must be > 0
local ffi = require("ffi")
local polyplug_guest = require("polyplug_guest")

-- Contract ID: fnv1a_64("pipeline.validator@1") = 0x027ABCEBF8020D90
local VALIDATOR_CONTRACT_ID_HI = 0x027ABCEB  -- upper 32 bits
local VALIDATOR_CONTRACT_ID_LO = 0xF8020D90  -- lower 32 bits

-- ABI type definitions (guarded to prevent "already defined" errors on second require())
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

    typedef struct {
        uint8_t  valid;    /* 1 = valid, 0 = invalid */
        uint8_t  _pad[7];
        struct { const uint8_t* ptr; size_t len; } reason;
    } ValidationResult;
]])

-- Returns true when all bytes in the LuaJIT string are printable ASCII (0x20..0x7E).
local function is_printable_ascii(s)
    for i = 1, #s do
        local b = s:byte(i)
        if b < 0x20 or b > 0x7E then
            return false
        end
    end
    return true
end

-- Implementation: validate(args: DataRecord*, out: ValidationResult*)
local function impl_validate(args_ptr, out_ptr)
    -- SAFETY: args_ptr and out_ptr are valid pointers per ABI contract.
    -- The host runtime allocates both buffers and guarantees alignment to 8 bytes.
    local args = ffi.cast("DataRecord*",      ffi.cast("uintptr_t", args_ptr))
    local out  = ffi.cast("ValidationResult*", ffi.cast("uintptr_t", out_ptr))

    -- Extract strings from StringView fields
    local name_str  = ffi.string(args.name.ptr,  args.name.len)
    local value_str = ffi.string(args.value.ptr, args.value.len)

    -- Rule 1: name must be non-empty
    if args.name.len == 0 then
        local reason_sv = polyplug_guest.string_view("name must not be empty")
        out.valid         = 0
        out.reason.ptr    = reason_sv.ptr
        out.reason.len    = reason_sv.len
        return 0  -- ABI_OK
    end

    -- Rule 2: value must be non-empty
    if args.value.len == 0 then
        local reason_sv = polyplug_guest.string_view("value must not be empty")
        out.valid         = 0
        out.reason.ptr    = reason_sv.ptr
        out.reason.len    = reason_sv.len
        return 0  -- ABI_OK
    end

    -- Rule 3a: name must contain only printable ASCII
    if not is_printable_ascii(name_str) then
        local reason_sv = polyplug_guest.string_view("name must contain only printable ASCII")
        out.valid         = 0
        out.reason.ptr    = reason_sv.ptr
        out.reason.len    = reason_sv.len
        return 0  -- ABI_OK
    end

    -- Rule 3b: value must contain only printable ASCII
    if not is_printable_ascii(value_str) then
        local reason_sv = polyplug_guest.string_view("value must contain only printable ASCII")
        out.valid         = 0
        out.reason.ptr    = reason_sv.ptr
        out.reason.len    = reason_sv.len
        return 0  -- ABI_OK
    end

    -- Rule 4: count must be > 0
    if args.count == 0 then
        local reason_sv = polyplug_guest.string_view("count must be greater than zero")
        out.valid         = 0
        out.reason.ptr    = reason_sv.ptr
        out.reason.len    = reason_sv.len
        return 0  -- ABI_OK
    end

    -- All rules passed — record is valid
    local reason_sv = polyplug_guest.string_view("ok")
    out.valid      = 1
    out.reason.ptr = reason_sv.ptr
    out.reason.len = reason_sv.len
    return 0  -- ABI_OK
end

-- polyplug_init is called by LuaLoader with the PluginRegistrar pointer as i64.
-- It does NOT call register_plugin directly — the LuaLoader (Rust) does that
-- after reading _G._polyplug_handlers and creating Rust-side trampolines.
function polyplug_init(_registrar_ptr_int, _ctx_ptr)
    -- LuaLoader reads _G._polyplug_handlers instead of calling register_plugin directly.
    -- This is the exact pattern from tests/fixtures/test_plugin.lua.
    _G._polyplug_handlers = {
        contract_name    = "pipeline.validator",
        contract_id_hex  = "027ABCEBF8020D90",  -- VALIDATOR_CONTRACT_ID without 0x prefix
        contract_version = 1,  -- major version = 1 (used by LuaLoader for contract_id computation)
        plugin_name      = "field-validator-lua",
        -- Functions in declaration order (function_id order):
        --   [0] = validate  (contract function)
        functions = {
            [0] = impl_validate,  -- function_id 0: validate (contract function)
        },
    }
end
