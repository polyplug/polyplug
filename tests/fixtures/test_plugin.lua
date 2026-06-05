-- tests/fixtures/test_plugin.lua
-- Lua test plugin implementing the test.add@1 contract.
-- This is loaded by integration_lua tests via LuaLoader.
--
-- DESIGN: This plugin does NOT create LuaJIT FFI callbacks directly,
-- because LuaJIT FFI callbacks cannot return structs by value (e.g. AbiError).
-- Instead, polyplug_init populates _G._polyplug_handlers with pure Lua
-- function implementations. The LuaLoader (Rust side) wraps these in
-- extern "C" trampolines and builds the GuestContractInterface itself.

local ffi = require("ffi")
local polyplug_guest = require("polyplug_guest")

local VERSION_STR = "1.0.0-lua"

-- Implementation: add(a: u32, b: u32) -> u32
-- args_ptr: i64 integer pointing to a {a:u32, b:u32} C struct
-- out_ptr:  i64 integer pointing to a u32 output slot
local function impl_add(args_ptr, out_ptr)
    local args = ffi.cast("uint32_t*", ffi.cast("uintptr_t", args_ptr))
    local out  = ffi.cast("uint32_t*", ffi.cast("uintptr_t", out_ptr))
    out[0] = args[0] + args[1]
end

local function impl_add_primitive(args_ptr, out_ptr)
    impl_add(args_ptr, out_ptr)
end

local function impl_version(_args_ptr, out_ptr)
    local out = ffi.cast("StringView*", ffi.cast("uintptr_t", out_ptr))
    out[0] = polyplug_guest.string_view(VERSION_STR)
end

local function impl_reset(_args_ptr, _out_ptr)
    -- no-op
end

-- polyplug_init is called by LuaLoader with the HostInterface pointer as i64.
-- It does NOT call register_plugin directly — the LuaLoader (Rust) does that
-- after reading _G._polyplug_handlers and creating Rust-side trampolines.
function polyplug_init(registrar_ptr, ctx_ptr)
    _G._polyplug_handlers = {
        ["test.add"] = {
            contract_version = 1,
            plugin_name = "test-plugin-lua",
            -- Functions in declaration order (must match contract function_id order):
            functions = {
                [0] = impl_add,           -- function_id 0: add
                [1] = impl_add_primitive, -- function_id 1: add_primitive
                [2] = impl_version,       -- function_id 2: version
                [3] = impl_reset,         -- function_id 3: reset
            },
        },
    }
end
