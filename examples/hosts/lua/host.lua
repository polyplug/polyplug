local ffi = require("ffi")
local bit = require("bit")

local band = bit.band
local bxor = bit.bxor
local lshift = bit.lshift
local rshift = bit.rshift

local script_dir = debug.getinfo(1, "S").source:match("^@(.*/)")
    or debug.getinfo(1, "S").source:match("^@(.*[/\\])")
    or "./"

local REPO_ROOT = script_dir .. "../../.."

package.path = REPO_ROOT .. "/host-libs/lua/?.lua;" .. package.path

local polyplug = require("polyplug")

local function resolve_polyplug_so()
    local env_path = os.getenv("POLYPLUG_SO")
    if env_path and #env_path > 0 then
        return env_path
    end
    local full_path = REPO_ROOT .. "/examples/hosts/js/target/debug/libpolyplug_full.so"
    local f = io.open(full_path, "rb")
    if f then
        f:close()
        return full_path
    end
    return REPO_ROOT .. "/target/debug/libpolyplug.so"
end

polyplug.load_lib(resolve_polyplug_so())

ffi.cdef([[
    typedef struct {
        const uint8_t* ptr;
        size_t         len;
    } StringView;

    typedef struct {
        uint8_t* ptr;
        size_t   len;
        size_t   cap;
    } Buffer;

    typedef struct {
        StringView name;
        StringView value;
        uint32_t   count;
        uint32_t   _pad;
    } DataRecord;

    typedef struct {
        uint32_t   code;
        uint32_t   _pad;
        const uint8_t* msg_ptr;
        size_t         msg_len;
    } AbiError;

    typedef AbiError (*AbiFunc)(const void* args, void* out);

    typedef struct {
        uint64_t  contract_id;
        uint32_t  contract_version;
        uint32_t  function_count;
        AbiFunc*  functions;
    } PluginVTable;

    typedef struct {
        uint8_t    valid;
        uint8_t    _pad[7];
        StringView reason;
    } ValidationResult;
]])

local function u64(hi, lo)
    return ffi.cast("uint64_t", hi) * 0x100000000ULL + lo
end

local DECODER_CONTRACT_ID = u64(0x133E62AB, 0xD6E7D5BE)
local TRANSFORMER_CONTRACT_ID = u64(0x0E304413, 0x3E12EB05)
local ENCODER_CONTRACT_ID = u64(0x12AD37F4, 0x3386F752)
local REPORTER_CONTRACT_ID = u64(0xD50E539C, 0xAE219A15)
local VALIDATOR_CONTRACT_ID = u64(0x027ABCEB, 0xF8020D90)

local ABI_OK = 0

local function fnv1a_64(bytes)
    local FNV_OFFSET_HI = 0xCBF29CE4
    local FNV_OFFSET_LO = 0x84222325
    local FNV_PRIME_HI = 0x00000100
    local FNV_PRIME_LO = 0x000001B3

    local function mul_u32(a, b)
        local a0 = band(a, 0xFFFF)
        local a1 = rshift(a, 16)
        local b0 = band(b, 0xFFFF)
        local b1 = rshift(b, 16)

        local p0 = a0 * b0
        local p1 = a0 * b1
        local p2 = a1 * b0
        local p3 = a1 * b1

        local mid = p1 + p2
        local mid_low = band(mid, 0xFFFF)
        local mid_high = rshift(mid, 16)

        local sum = p0 + lshift(mid_low, 16)
        local low = band(sum, 0xFFFFFFFF)
        local carry = math.floor(sum / 4294967296)
        local high = band(p3 + mid_high + carry, 0xFFFFFFFF)
        return high, low
    end

    local function mul_fnv(hi, lo)
        local h0, l0 = mul_u32(lo, FNV_PRIME_LO)
        local _, l1 = mul_u32(hi, FNV_PRIME_LO)
        local _, l2 = mul_u32(lo, FNV_PRIME_HI)
        local high = band(h0 + l1 + l2, 0xFFFFFFFF)
        return high, l0
    end

    local hi = FNV_OFFSET_HI
    local lo = FNV_OFFSET_LO
    for i = 1, #bytes do
        local b = bytes:byte(i)
        lo = bxor(lo, b)
        hi, lo = mul_fnv(hi, lo)
    end
    return u64(hi, lo)
end

local function bundle_id(name)
    return fnv1a_64(name)
end

local function call_vtable_fn(vtable_ptr, fn_index, args_ptr, out_ptr)
    local vt = ffi.cast("const PluginVTable*", vtable_ptr)
    local fn_ptr = vt.functions[fn_index]
    return fn_ptr(args_ptr, out_ptr)
end

local function get_vtable(rt, handle)
    local guard, err = rt:resolve_plugin(handle)
    if not guard then
        error("resolve_plugin failed: " .. (err or "unknown"))
    end
    return guard:vtable(), guard
end

local function string_view_to_str(sv)
    if sv.ptr == nil or sv.len == 0 then
        return ""
    end
    return ffi.string(sv.ptr, sv.len)
end

local function abi_error_message(err, fallback)
    if err.msg_ptr == nil or err.msg_len == 0 then
        return fallback
    end
    return ffi.string(err.msg_ptr, err.msg_len)
end

local function load_bundles(rt, bundles)
    print("Loading 12 guest plugins...")
    for i, path in ipairs(bundles) do
        rt:load_bundle(path)
        local lang = path:match("examples/guests/([^/]+)/") or "?"
        local name = path:match("/([^/]+)$") or path
        print(string.format("  [OK]  %2d/12 %s/%s", i, lang, name))
    end
end

local function resolve_by_bundle(rt, bundle_name, contract_id)
    local handle = rt:find_by_bundle(bundle_id(bundle_name), contract_id, 0)
    if ffi.cast("uint64_t", handle) == polyplug.NULL_HANDLE then
        error("plugin not found for bundle: " .. bundle_name)
    end
    return get_vtable(rt, handle)
end

local function run_pipeline(label, decoder_vt, encoder_vt, transformer_vt, reporter_vt, validator_vt, input_csv)
    print("--- " .. label .. " ---")

    local input_buf = ffi.new("Buffer")
    local input_bytes = ffi.cast("uint8_t*", input_csv)
    input_buf.ptr = input_bytes
    input_buf.len = #input_csv
    input_buf.cap = #input_csv

    local record = ffi.new("DataRecord")
    record.name.ptr = nil
    record.name.len = 0
    record.value.ptr = nil
    record.value.len = 0
    record.count = 0
    record._pad = 0

    local decode_err = call_vtable_fn(decoder_vt, 0, input_buf, record)
    if decode_err.code ~= ABI_OK then
        local msg = abi_error_message(decode_err, "decode failed")
        error(string.format("decode failed: %s (code %d)", msg, decode_err.code))
    end

    local transformed = ffi.new("DataRecord")
    transformed.name.ptr = nil
    transformed.name.len = 0
    transformed.value.ptr = nil
    transformed.value.len = 0
    transformed.count = 0
    transformed._pad = 0

    local transform_err = call_vtable_fn(transformer_vt, 0, record, transformed)
    if transform_err.code ~= ABI_OK then
        local msg = abi_error_message(transform_err, "transform failed")
        error(string.format("transform failed: %s (code %d)", msg, transform_err.code))
    end

    local encoded = ffi.new("Buffer")
    encoded.ptr = nil
    encoded.len = 0
    encoded.cap = 0

    local encode_err = call_vtable_fn(encoder_vt, 0, transformed, encoded)
    if encode_err.code ~= ABI_OK then
        local msg = abi_error_message(encode_err, "encode failed")
        error(string.format("encode failed: %s (code %d)", msg, encode_err.code))
    end
    local output = ""
    if encoded.ptr ~= nil and encoded.len > 0 then
        output = ffi.string(encoded.ptr, encoded.len)
    end
    print("Run output: " .. output:gsub("\n", ""))

    local report_sv = ffi.new("StringView")
    report_sv.ptr = nil
    report_sv.len = 0
    local report_err = call_vtable_fn(reporter_vt, 0, transformed, report_sv)
    if report_err.code ~= ABI_OK then
        local msg = abi_error_message(report_err, "report failed")
        error(string.format("report failed: %s (code %d)", msg, report_err.code))
    end
    local report_str = string_view_to_str(report_sv)
    if #report_str > 0 then
        print("Run summary: " .. report_str)
    end

    local validation = ffi.new("ValidationResult")
    local validate_err = call_vtable_fn(validator_vt, 0, transformed, validation)
    if validate_err.code ~= ABI_OK then
        local msg = abi_error_message(validate_err, "validate failed")
        error(string.format("validate failed: %s (code %d)", msg, validate_err.code))
    end
    local reason = string_view_to_str(validation.reason)
    local status = validation.valid ~= 0 and "ok" or "invalid"
    print(string.format("Validation: %s (%s)", status, reason))
end

local function main()
    print("=== polyplug C# host example ===")

    local rt = polyplug.Runtime.new()
    polyplug.register_native_loader(rt._ptr)
    polyplug.register_dotnet_loader(rt._ptr, { min_framework = "10.0" })
    polyplug.register_python_loader(rt._ptr, { min_version = "3.11" })
    polyplug.register_lua_loader(rt._ptr)
    polyplug.register_js_loader(rt._ptr)

    local bundles = {
        REPO_ROOT .. "/examples/guests/rust/decoder",
        REPO_ROOT .. "/examples/guests/rust/encoder",
        REPO_ROOT .. "/examples/guests/cpp/transformer",
        REPO_ROOT .. "/examples/guests/cpp/validator",
        REPO_ROOT .. "/examples/guests/csharp/encoder",
        REPO_ROOT .. "/examples/guests/csharp/reporter",
        REPO_ROOT .. "/examples/guests/python/decoder",
        REPO_ROOT .. "/examples/guests/python/reporter",
        REPO_ROOT .. "/examples/guests/lua/transformer",
        REPO_ROOT .. "/examples/guests/lua/validator",
        REPO_ROOT .. "/examples/guests/js/validator",
        REPO_ROOT .. "/examples/guests/js/reporter",
    }

    load_bundles(rt, bundles)

    local decoder_rust_vt, decoder_rust_guard = resolve_by_bundle(rt, "csv_decoder", DECODER_CONTRACT_ID)
    local encoder_rust_vt, encoder_rust_guard = resolve_by_bundle(rt, "csv_encoder_rust", ENCODER_CONTRACT_ID)
    local transformer_cpp_vt, transformer_cpp_guard = resolve_by_bundle(rt, "uppercase_transformer", TRANSFORMER_CONTRACT_ID)
    local validator_cpp_vt, validator_cpp_guard = resolve_by_bundle(rt, "cpp_validator", VALIDATOR_CONTRACT_ID)
    local encoder_csharp_vt, encoder_csharp_guard = resolve_by_bundle(rt, "csv_encoder_csharp", ENCODER_CONTRACT_ID)
    local reporter_csharp_vt, reporter_csharp_guard = resolve_by_bundle(rt, "csharp_reporter", REPORTER_CONTRACT_ID)
    local decoder_python_vt, decoder_python_guard = resolve_by_bundle(rt, "python_decoder", DECODER_CONTRACT_ID)
    local reporter_python_vt, reporter_python_guard = resolve_by_bundle(rt, "summary_reporter", REPORTER_CONTRACT_ID)
    local transformer_lua_vt, transformer_lua_guard = resolve_by_bundle(rt, "reverse_transformer", TRANSFORMER_CONTRACT_ID)
    local validator_lua_vt, validator_lua_guard = resolve_by_bundle(rt, "lua_validator", VALIDATOR_CONTRACT_ID)
    local reporter_js_vt, reporter_js_guard = resolve_by_bundle(rt, "js_reporter", REPORTER_CONTRACT_ID)
    local validator_js_vt, validator_js_guard = resolve_by_bundle(rt, "field_validator", VALIDATOR_CONTRACT_ID)

    run_pipeline(
        "Run 1: Rust decoder, C++ transformer, Rust encoder, C# reporter, C++ validator",
        decoder_rust_vt,
        encoder_rust_vt,
        transformer_cpp_vt,
        reporter_csharp_vt,
        validator_cpp_vt,
        "Alice,hello,3\n"
    )

    run_pipeline(
        "Run 2: Python decoder, Lua transformer, C# encoder, Python reporter, Lua validator",
        decoder_python_vt,
        encoder_csharp_vt,
        transformer_lua_vt,
        reporter_python_vt,
        validator_lua_vt,
        "Bob,world,4\n"
    )

    run_pipeline(
        "Run 3: Rust decoder, C++ transformer, C# encoder, JS reporter, JS validator",
        decoder_rust_vt,
        encoder_csharp_vt,
        transformer_cpp_vt,
        reporter_js_vt,
        validator_js_vt,
        "Cara,polyplug,5\n"
    )

    decoder_rust_guard:free()
    encoder_rust_guard:free()
    transformer_cpp_guard:free()
    validator_cpp_guard:free()
    encoder_csharp_guard:free()
    reporter_csharp_guard:free()
    decoder_python_guard:free()
    reporter_python_guard:free()
    transformer_lua_guard:free()
    validator_lua_guard:free()
    reporter_js_guard:free()
    validator_js_guard:free()

    rt:free()
    print("pipeline complete")
end

local ok, err = pcall(main)
if not ok then
    io.stderr:write("error: " .. tostring(err) .. "\n")
    os.exit(1)
end
