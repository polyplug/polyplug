-- examples/hosts/lua/host.lua
-- Lua host example for polyplug.
--
-- Loads all 12 guest plugins (2 per language: Rust, C++, C#, Python, Lua, JS)
-- and runs the full pipeline: decode → validate → transform → encode → report.
--
-- Requires LuaJIT (for the ffi module). Run with:
--   luajit examples/hosts/lua/host.lua

local ffi    = require("ffi")
local script_dir = debug.getinfo(1, "S").source:match("^@(.*/)")
    or debug.getinfo(1, "S").source:match("^@(.*[/\\])")
    or "./"

-- ─── ABI type declarations ────────────────────────────────────────────────────
-- Mirrors the frozen ABI from crates/polyplug/src/abi.rs
ffi.cdef([[
    // StringView: non-owning UTF-8 string slice.
    typedef struct {
        const uint8_t* ptr;
        size_t         len;
    } StringView;

    // Buffer: byte buffer (may be host-allocated).
    typedef struct {
        uint8_t* ptr;
        size_t   len;
        size_t   cap;
    } Buffer;

    // DataRecord: decoded CSV row passed between pipeline stages.
    // Layout: name(16) + value(16) + count(4) + _pad(4) = 40 bytes.
    typedef struct {
        StringView name;
        StringView value;
        uint32_t   count;
        uint32_t   _pad;
    } DataRecord;

    // AbiError: returned by every ABI function.
    // Layout: code(4) + _pad(4) + message.ptr(8) + message.len(8) = 24 bytes.
    typedef struct {
        uint32_t   code;
        uint32_t   _pad;
        const uint8_t* msg_ptr;
        size_t         msg_len;
    } AbiError;

    // Generic ABI function pointer: fn(args: *const (), out: *mut ()) -> AbiError.
    typedef AbiError (*AbiFunc)(const void* args, void* out);

    // PluginVTable: layout mirrors polyplug::abi::PluginVTable.
    // Layout: contract_id(8) + contract_version(4) + function_count(4) + functions(8) = 24 bytes.
    typedef struct {
        uint64_t  contract_id;
        uint32_t  contract_version;
        uint32_t  function_count;
        AbiFunc*  functions;
    } PluginVTable;

    // ValidationResult: output type of pipeline.validator@1 plugins.
    // Layout: valid(1) + _pad(7) + reason.ptr(8) + reason.len(8) = 24 bytes.
    typedef struct {
        uint8_t    valid;
        uint8_t    _pad[7];
        StringView reason;
    } ValidationResult;
]])

local function u64(hi, lo) return ffi.cast("uint64_t", hi) * 0x100000000ULL + lo end
-- ─── Contract IDs (FNV-1a 64-bit of "pipeline.<name>@1") ─────────────────────
local DECODER_CONTRACT_ID     = u64(0x133E62AB, 0xD6E7D5BE)
local TRANSFORMER_CONTRACT_ID = u64(0x0E304413, 0x3E12EB05)
local ENCODER_CONTRACT_ID     = u64(0x12AD37F4, 0x3386F752)
local REPORTER_CONTRACT_ID    = u64(0xD50E539C, 0xAE219A15)
local VALIDATOR_CONTRACT_ID   = u64(0x027ABCEB, 0xF8020D90)

local ABI_OK = 0

-- ─── Helper: construct a null StringView ─────────────────────────────────────
local function null_sv()
    local sv = ffi.new("StringView")
    sv.ptr = nil
    sv.len = 0
    return sv
end

-- ─── Helper: construct a null Buffer ─────────────────────────────────────────
local function null_buf()
    local buf = ffi.new("Buffer")
    buf.ptr = nil
    buf.len = 0
    buf.cap = 0
    return buf
end

-- ─── Helper: call a vtable function ──────────────────────────────────────────
-- vtable_ptr: void* pointing to a PluginVTable
-- fn_index:   0-based index into vtable.functions
-- args_ptr:   const void* — pointer to the input argument struct
-- out_ptr:    void* — pointer to the output struct
-- Returns: AbiError
local function call_vtable_fn(vtable_ptr, fn_index, args_ptr, out_ptr)
    local vt = ffi.cast("const PluginVTable*", vtable_ptr)
    local fn_ptr = vt.functions[fn_index]
    return fn_ptr(args_ptr, out_ptr)
end

-- ─── Helper: get vtable from a guard ────────────────────────────────────────
local function get_vtable(rt, handle)
    local guard, err = rt:resolve_plugin(handle)
    if not guard then
        error("resolve_plugin failed: " .. (err or "unknown"))
    end
    return guard:vtable(), guard
end

-- ─── Load the companion shared library ───────────────────────────────────────
-- The companion cdylib (built from examples/hosts/lua/src/lib.rs) provides
-- polyplug_runtime_new_full() — a runtime with all language loaders registered.
local so_path = script_dir .. "target/debug/libpolyplug_lua_host.so"

-- Add path to polyplug_full module search path
package.path = script_dir .. "?.lua;" .. package.path

local polyplug = require("polyplug_full")
polyplug.load_lib(so_path)

-- ─── Build runtime ────────────────────────────────────────────────────────────
print("=== polyplug lua host ===")
local rt = polyplug.Runtime.new()

-- ─── Resolve guest plugin paths ───────────────────────────────────────────────
-- All paths are relative to the repo root (run from repo root).
local REPO_ROOT = script_dir .. "../../.."
local function guest_path(lang, name)
    return REPO_ROOT .. "/examples/guests/" .. lang .. "/" .. name
end

-- 12 guest plugin directories:
local guest_dirs = {
    -- C# guests loaded first: CLR must initialize before native guests are dlopen'd,
    -- otherwise the C++ .so symbols interfere with CLR startup (segfault).
    guest_path("csharp",  "encoder"),       -- 1: C# csv_encoder
    guest_path("csharp",  "reporter"),      -- 2: C# reporter
    guest_path("rust",    "decoder"),       -- 3: Rust csv_decoder
    guest_path("rust",    "encoder"),       -- 4: Rust csv_encoder
    guest_path("cpp",     "transformer"),   -- 5: C++ uppercase_transformer
    guest_path("cpp",     "validator"),     -- 6: C++ validator
    guest_path("python",  "decoder"),       -- 7: Python decoder
    guest_path("python",  "reporter"),      -- 8: Python reporter
    guest_path("lua",     "transformer"),   -- 9: Lua reverse_transformer
    guest_path("lua",     "validator"),     -- 10: Lua validator
    guest_path("js",      "validator"),     -- 11: JS field_validator
    guest_path("js",      "reporter"),      -- 12: JS reporter
}

-- ─── Load all 12 guests ───────────────────────────────────────────────────────
local loaded_count = 0
local load_errors  = {}

for i, dir in ipairs(guest_dirs) do
    local ok, err = pcall(function()
        rt:load_bundle(dir)
    end)
    if ok then
        loaded_count = loaded_count + 1
        print(string.format("[load] guest %2d OK: %s", i, dir:match("[^/]+/[^/]+$") or dir))
    else
        table.insert(load_errors, { index = i, dir = dir, err = err })
        print(string.format("[load] guest %2d WARN: %s — %s", i, dir:match("[^/]+/[^/]+$") or dir, err))
    end
end

print(string.format("[load] %d/12 guests loaded", loaded_count))

-- ─── Run pipeline ─────────────────────────────────────────────────────────────
-- The pipeline requires: decoder, validator, transformer, encoder, reporter.
-- We pick the first available provider of each contract.

local function find_first(contract_id, label)
    local handle = rt:find_by_contract(contract_id, 0)
    if ffi.cast("uint64_t", handle) == polyplug.NULL_HANDLE then
        error("no plugin found for contract: " .. label)
    end
    return handle
end

local decoder_h     = find_first(DECODER_CONTRACT_ID,     "pipeline.decoder")
local validator_h   = find_first(VALIDATOR_CONTRACT_ID,   "pipeline.validator")
local transformer_h = find_first(TRANSFORMER_CONTRACT_ID, "pipeline.transformer")
local encoder_h     = find_first(ENCODER_CONTRACT_ID,     "pipeline.encoder")
local reporter_h    = find_first(REPORTER_CONTRACT_ID,    "pipeline.reporter")

-- Resolve guards and vtables (keep guards alive for the duration of the pipeline).
local decoder_vt,     decoder_guard     = get_vtable(rt, decoder_h)
local validator_vt,   validator_guard   = get_vtable(rt, validator_h)
local transformer_vt, transformer_guard = get_vtable(rt, transformer_h)
local encoder_vt,     encoder_guard     = get_vtable(rt, encoder_h)
local reporter_vt,    reporter_guard    = get_vtable(rt, reporter_h)

-- ─── Stage 1: decode ──────────────────────────────────────────────────────────
local csv_input   = "Alice,hello,3\n"
local input_bytes = ffi.cast("uint8_t*", csv_input)
local input_buf   = ffi.new("Buffer")
input_buf.ptr     = input_bytes
input_buf.len     = #csv_input
input_buf.cap     = #csv_input

local record = ffi.new("DataRecord")
record.name.ptr  = nil ; record.name.len  = 0
record.value.ptr = nil ; record.value.len = 0
record.count     = 0
record._pad      = 0

local decode_err = call_vtable_fn(decoder_vt, 0, input_buf, record)
if decode_err.code ~= ABI_OK then
    error(string.format("decode failed: code=%d", decode_err.code))
end

local name_str  = ffi.string(record.name.ptr,  record.name.len)
local value_str = ffi.string(record.value.ptr, record.value.len)
print(string.format("[decode]    name=%s  value=%s  count=%d", name_str, value_str, record.count))

-- ─── Stage 2: validate ────────────────────────────────────────────────────────
-- ValidationResult is 24 bytes (valid:1 + pad:7 + reason:StringView:16).
-- Using uint64_t[1] (8 bytes) would corrupt the heap — must use correct type.
local validate_out = ffi.new("ValidationResult")
local _validate_err = call_vtable_fn(validator_vt, 0, record, validate_out)
-- Validation errors are non-fatal in this example.
if _validate_err.code ~= ABI_OK then
    print(string.format("[validate]  WARN code=%d (continuing)", _validate_err.code))
elseif validate_out.valid == 0 then
    local reason = validate_out.reason.ptr ~= nil
        and ffi.string(validate_out.reason.ptr, validate_out.reason.len) or "?"
    print(string.format("[validate]  INVALID: %s (continuing)", reason))
else
    print("[validate]  OK")
end

-- ─── Stage 3: transform ───────────────────────────────────────────────────────
local transformed = ffi.new("DataRecord")
transformed.name.ptr  = nil ; transformed.name.len  = 0
transformed.value.ptr = nil ; transformed.value.len = 0
transformed.count     = 0
transformed._pad      = 0

local _transform_err = call_vtable_fn(transformer_vt, 0, record, transformed)
if _transform_err.code ~= ABI_OK then
    print(string.format("[transform] WARN code=%d (using original record)", _transform_err.code))
    -- Fall back to original record if transform fails
    transformed = record
else
    local t_name  = transformed.name.ptr  ~= nil and ffi.string(transformed.name.ptr,  transformed.name.len)  or "(nil)"
    local t_value = transformed.value.ptr ~= nil and ffi.string(transformed.value.ptr, transformed.value.len) or "(nil)"
    print(string.format("[transform] name=%s  value=%s  count=%d", t_name, t_value, transformed.count))
end

-- ─── Stage 4: encode ──────────────────────────────────────────────────────────
local encoded_buf = ffi.new("Buffer")
encoded_buf.ptr   = nil
encoded_buf.len   = 0
encoded_buf.cap   = 0

local encode_err = call_vtable_fn(encoder_vt, 0, transformed, encoded_buf)
if encode_err.code ~= ABI_OK then
    error(string.format("encode failed: code=%d", encode_err.code))
end
local encoded_str = ffi.string(encoded_buf.ptr, encoded_buf.len)
print("[encode]    " .. encoded_str:gsub("\n", ""))

-- ─── Stage 5: report ─────────────────────────────────────────────────────────
local report_sv = ffi.new("StringView")
report_sv.ptr   = nil
report_sv.len   = 0

local report_err = call_vtable_fn(reporter_vt, 0, transformed, report_sv)
if report_err.code ~= ABI_OK then
    print(string.format("[report]    WARN code=%d", report_err.code))
elseif report_sv.ptr ~= nil and report_sv.len > 0 then
    local report_str = ffi.string(report_sv.ptr, report_sv.len)
    print("[report]    " .. report_str:gsub("\n", ""))
else
    print("[report]    (empty)")
end

-- ─── Free guards ─────────────────────────────────────────────────────────────
decoder_guard:free()
validator_guard:free()
transformer_guard:free()
encoder_guard:free()
reporter_guard:free()

-- ─── Error scenario: malformed input ─────────────────────────────────────────
print("--- error scenario: malformed input ---")
local bad_input   = "INVALID\n"
local bad_bytes   = ffi.cast("uint8_t*", bad_input)
local bad_buf     = ffi.new("Buffer")
bad_buf.ptr       = bad_bytes
bad_buf.len       = #bad_input
bad_buf.cap       = #bad_input

local bad_record  = ffi.new("DataRecord")
bad_record.name.ptr  = nil ; bad_record.name.len  = 0
bad_record.value.ptr = nil ; bad_record.value.len = 0
bad_record.count     = 0
bad_record._pad      = 0

local err_h2 = find_first(DECODER_CONTRACT_ID, "pipeline.decoder")
local err_vt, err_guard = get_vtable(rt, err_h2)
local bad_err = call_vtable_fn(err_vt, 0, bad_buf, bad_record)
if bad_err.code ~= ABI_OK then
    local msg = ""
    if bad_err.msg_ptr ~= nil and bad_err.msg_len > 0 then
        msg = ffi.string(bad_err.msg_ptr, bad_err.msg_len)
    else
        msg = "unknown error"
    end
    print(string.format("[error]     decode failed: %s (code %d)", msg, bad_err.code))
end
err_guard:free()

-- ─── Done ─────────────────────────────────────────────────────────────────────
rt:free()
print("pipeline complete")
