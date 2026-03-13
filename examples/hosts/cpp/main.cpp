#include <cstdint>
#include <cstring>
#include <iostream>
#include <limits>
#include <iomanip>
#include <stdexcept>
#include <string>
#include <string_view>
#include <unordered_map>
#include <vector>


#include "../../../host-libs/cpp/polyplug/abi.hpp"

struct DataRecord {
    StringView name;
    StringView value;
    uint32_t count;
    uint32_t _pad;
};

struct ValidationResult {
    uint8_t valid;
    uint8_t _pad[7];
    StringView reason;
};

static_assert(sizeof(DataRecord) == 40, "DataRecord ABI size mismatch");
static_assert(sizeof(ValidationResult) == 24, "ValidationResult ABI size mismatch");

static constexpr uint64_t DECODER_CONTRACT_ID = 0x133E62ABD6E7D5BEULL;
static constexpr uint64_t TRANSFORMER_CONTRACT_ID = 0x0E3044133E12EB05ULL;
static constexpr uint64_t ENCODER_CONTRACT_ID = 0x12AD37F43386F752ULL;
static constexpr uint64_t REPORTER_CONTRACT_ID = 0xD50E539CAE219A15ULL;
static constexpr uint64_t VALIDATOR_CONTRACT_ID = 0x027ABCEBF8020D90ULL;

struct PluginRef {
    OpaqueGuard* guard;
    const PluginVTable* vtable;
};

static StringView null_sv() {
    StringView sv;
    sv.ptr = nullptr;
    sv.len = 0U;
    return sv;
}

static uint64_t fnv1a_64(std::string_view s) {
    constexpr uint64_t FNV_OFFSET = 0xcbf29ce484222325ULL;
    constexpr uint64_t FNV_PRIME = 0x00000100000001B3ULL;
    uint64_t hash = FNV_OFFSET;
    for (unsigned char c : s) {
        hash ^= static_cast<uint64_t>(c);
        hash *= FNV_PRIME;
    }
    return hash;
}

static std::string read_last_error() {
    size_t len = polyplug_error_message_len();
    if (len == 0U) {
        return std::string();
    }
    std::vector<uint8_t> buf(len);
    size_t written = polyplug_last_error(buf.data(), buf.size());
    return std::string(reinterpret_cast<const char*>(buf.data()), written);
}

static void free_error_message(const AbiError& err) {
    if (err.code == ABI_OK) {
        return;
    }
    if (err.message.ptr == nullptr || err.message.len == 0U) {
        return;
    }
    polyplug_host_free(
        const_cast<uint8_t*>(err.message.ptr),
        err.message.len,
        1U
    );
}

static std::string abi_error_to_string(const AbiError& err) {
    if (err.message.ptr == nullptr || err.message.len == 0U) {
        return std::string("unknown error");
    }
    return std::string(
        reinterpret_cast<const char*>(err.message.ptr),
        err.message.len
    );
}

static void ensure_ok(const AbiError& err, const char* stage) {
    if (err.code == ABI_OK) {
        return;
    }
    std::string msg = abi_error_to_string(err);
    free_error_message(err);
    throw std::runtime_error(
        std::string(stage) + " failed: " + msg + " (code " + std::to_string(err.code) + ")"
    );
}

static void load_bundle(OpaqueRuntime* runtime, const std::string& path) {
    uint32_t result = polyplug_load_bundle(
        runtime,
        reinterpret_cast<const uint8_t*>(path.data()),
        path.size()
    );
    if (result == 0U) {
        return;
    }
    std::string msg = read_last_error();
    if (msg.empty()) {
        msg = "unknown error";
    }
    throw std::runtime_error("load_bundle failed for " + path + ": " + msg);
}

static PluginRef resolve_plugin(
    OpaqueRuntime* runtime,
    const std::string& bundle_name,
    uint64_t contract_id
) {
    uint64_t bundle_id = fnv1a_64(bundle_name);
    uint64_t packed = polyplug_rt_find_by_bundle(runtime, bundle_id, contract_id, 0U);
    if (packed == std::numeric_limits<uint64_t>::max()) {
        throw std::runtime_error("plugin not found for bundle: " + bundle_name);
    }
    OpaqueGuard* guard = polyplug_rt_resolve_plugin(runtime, packed);
    if (guard == nullptr) {
        std::string msg = read_last_error();
        if (msg.empty()) {
            msg = "resolve_plugin returned null";
        }
        throw std::runtime_error(msg);
    }
    const void* vt_ptr = polyplug_get_vtable(guard);
    if (vt_ptr == nullptr) {
        polyplug_guard_free(guard);
        throw std::runtime_error("null vtable for bundle: " + bundle_name);
    }
    PluginRef ref;
    ref.guard = guard;
    ref.vtable = static_cast<const PluginVTable*>(vt_ptr);
    return ref;
}

static AbiError call_fn(
    const PluginVTable* vtable,
    uint32_t fn_id,
    const void* args,
    void* out
) {
    if (vtable == nullptr || vtable->functions == nullptr) {
        AbiError err;
        err.code = ABI_ERROR_GENERIC;
        err.message = null_sv();
        return err;
    }
    if (fn_id >= vtable->function_count) {
        AbiError err;
        err.code = ABI_FUNCTION_NOT_AVAIL;
        err.message = null_sv();
        return err;
    }
    using FnPtr = AbiError (*)(const void*, void*);
    void* const* fn_table = vtable->functions;
    auto fn_ptr = reinterpret_cast<FnPtr>(fn_table[fn_id]);
    return fn_ptr(args, out);
}

static std::string string_view_to_string(const StringView& sv) {
    if (sv.ptr == nullptr || sv.len == 0U) {
        return std::string();
    }
    return std::string(reinterpret_cast<const char*>(sv.ptr), sv.len);
}

static std::string trim_line_endings(const std::string& value) {
    if (value.empty()) {
        return value;
    }
    std::string trimmed = value;
    while (!trimmed.empty()) {
        char tail = trimmed.back();
        if (tail != '\n' && tail != '\r') {
            break;
        }
        trimmed.pop_back();
    }
    return trimmed;
}

static std::string bundle_display_name(const std::string& path) {
    size_t last = path.find_last_of('/');
    if (last == std::string::npos || last == 0U) {
        return path;
    }
    size_t prev = path.find_last_of('/', last - 1U);
    if (prev == std::string::npos) {
        return path.substr(0U, last) + "/" + path.substr(last + 1U);
    }
    std::string parent = path.substr(prev + 1U, last - prev - 1U);
    std::string leaf = path.substr(last + 1U);
    return parent + "/" + leaf;
}

static void run_pipeline(
    const std::string& label,
    const PluginRef& decoder,
    const PluginRef& transformer,
    const PluginRef& encoder,
    const PluginRef& reporter,
    const PluginRef& validator,
    const std::string& input_csv
) {
    std::cout << "--- " << label << " ---" << std::endl;

    Buffer input_buf;
    input_buf.ptr = const_cast<void*>(static_cast<const void*>(input_csv.data()));
    input_buf.len = input_csv.size();
    input_buf.cap = input_csv.size();

    DataRecord decoded;
    decoded.name = null_sv();
    decoded.value = null_sv();
    decoded.count = 0U;
    decoded._pad = 0U;

    AbiError decode_err = call_fn(
        decoder.vtable,
        0U,
        &input_buf,
        &decoded
    );
    ensure_ok(decode_err, "decode");

    DataRecord transformed;
    transformed.name = null_sv();
    transformed.value = null_sv();
    transformed.count = 0U;
    transformed._pad = 0U;

    AbiError transform_err = call_fn(
        transformer.vtable,
        0U,
        &decoded,
        &transformed
    );
    ensure_ok(transform_err, "transform");

    Buffer encoded;
    encoded.ptr = nullptr;
    encoded.len = 0U;
    encoded.cap = 0U;

    AbiError encode_err = call_fn(
        encoder.vtable,
        0U,
        &transformed,
        &encoded
    );
    ensure_ok(encode_err, "encode");

    std::string encoded_str;
    if (encoded.ptr != nullptr && encoded.len > 0U) {
        encoded_str.assign(
            static_cast<const char*>(encoded.ptr),
            encoded.len
        );
    }
    std::cout << "Run output: " << trim_line_endings(encoded_str) << std::endl;

    StringView report_sv = null_sv();
    AbiError report_err = call_fn(
        reporter.vtable,
        0U,
        &transformed,
        &report_sv
    );
    ensure_ok(report_err, "report");

    std::string report_str = string_view_to_string(report_sv);
    if (!report_str.empty()) {
        std::cout << "Run summary: " << report_str << std::endl;
    }

    ValidationResult validation;
    validation.valid = 0U;
    std::memset(validation._pad, 0, sizeof(validation._pad));
    validation.reason = null_sv();

    AbiError validate_err = call_fn(
        validator.vtable,
        0U,
        &transformed,
        &validation
    );
    ensure_ok(validate_err, "validate");

    std::string reason_str = string_view_to_string(validation.reason);
    std::cout << "Validation: " << (validation.valid ? "ok" : "invalid")
              << " (" << reason_str << ")" << std::endl;
}

int main() {
    std::cout << "=== polyplug C# host example ===" << std::endl;

    OpaqueRuntime* runtime = polyplug_runtime_new();
    if (runtime == nullptr) {
        std::string msg = read_last_error();
        if (msg.empty()) {
            msg = "polyplug_runtime_new failed";
        }
        std::cerr << msg << std::endl;
        return 1;
    }

    try {
        std::vector<std::string> bundles = {
            "examples/guests/rust/decoder",
            "examples/guests/rust/encoder",
            "examples/guests/cpp/transformer",
            "examples/guests/cpp/validator",
            "examples/guests/csharp/encoder",
            "examples/guests/csharp/reporter",
            "examples/guests/python/decoder",
            "examples/guests/python/reporter",
            "examples/guests/lua/transformer",
            "examples/guests/lua/validator",
            "examples/guests/js/validator",
            "examples/guests/js/reporter",
        };

        std::cout << "Loading 12 guest plugins..." << std::endl;
        std::size_t index = 0U;
        for (const std::string& path : bundles) {
            index += 1U;
            load_bundle(runtime, path);
            std::cout << "  [OK]  " << std::setw(2) << index << "/12 "
                      << bundle_display_name(path) << std::endl;
        }

        std::unordered_map<std::string, PluginRef> plugins;
        plugins.emplace("decoder_rust", resolve_plugin(runtime, "csv_decoder", DECODER_CONTRACT_ID));
        plugins.emplace("encoder_rust", resolve_plugin(runtime, "csv_encoder_rust", ENCODER_CONTRACT_ID));
        plugins.emplace("transformer_cpp", resolve_plugin(runtime, "uppercase_transformer", TRANSFORMER_CONTRACT_ID));
        plugins.emplace("validator_cpp", resolve_plugin(runtime, "cpp_validator", VALIDATOR_CONTRACT_ID));
        plugins.emplace("encoder_csharp", resolve_plugin(runtime, "csv_encoder_csharp", ENCODER_CONTRACT_ID));
        plugins.emplace("reporter_csharp", resolve_plugin(runtime, "csharp_reporter", REPORTER_CONTRACT_ID));
        plugins.emplace("decoder_python", resolve_plugin(runtime, "python_decoder", DECODER_CONTRACT_ID));
        plugins.emplace("reporter_python", resolve_plugin(runtime, "summary_reporter", REPORTER_CONTRACT_ID));
        plugins.emplace("transformer_lua", resolve_plugin(runtime, "reverse_transformer", TRANSFORMER_CONTRACT_ID));
        plugins.emplace("validator_lua", resolve_plugin(runtime, "lua_validator", VALIDATOR_CONTRACT_ID));
        plugins.emplace("validator_js", resolve_plugin(runtime, "field_validator", VALIDATOR_CONTRACT_ID));
        plugins.emplace("reporter_js", resolve_plugin(runtime, "js_reporter", REPORTER_CONTRACT_ID));

        run_pipeline(
            "Run 1: Rust decoder, C++ transformer, Rust encoder, C# reporter, C++ validator",
            plugins.at("decoder_rust"),
            plugins.at("transformer_cpp"),
            plugins.at("encoder_rust"),
            plugins.at("reporter_csharp"),
            plugins.at("validator_cpp"),
            "Alice,hello,3\n"
        );

        run_pipeline(
            "Run 2: Python decoder, Lua transformer, C# encoder, Python reporter, Lua validator",
            plugins.at("decoder_python"),
            plugins.at("transformer_lua"),
            plugins.at("encoder_csharp"),
            plugins.at("reporter_python"),
            plugins.at("validator_lua"),
            "Bob,world,4\n"
        );

        run_pipeline(
            "Run 3: Rust decoder, C++ transformer, C# encoder, JS reporter, JS validator",
            plugins.at("decoder_rust"),
            plugins.at("transformer_cpp"),
            plugins.at("encoder_csharp"),
            plugins.at("reporter_js"),
            plugins.at("validator_js"),
            "Cara,polyplug,5\n"
        );

        for (auto& entry : plugins) {
            if (entry.second.guard != nullptr) {
                polyplug_guard_free(entry.second.guard);
                entry.second.guard = nullptr;
                entry.second.vtable = nullptr;
            }
        }

        polyplug_runtime_free(runtime);
        std::cout << "pipeline complete" << std::endl;
        return 0;
    } catch (const std::exception& ex) {
        std::cerr << "error: " << ex.what() << std::endl;
        polyplug_runtime_free(runtime);
        return 1;
    }
}
