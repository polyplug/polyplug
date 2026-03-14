#include <cstdint>
#include <cstring>
#include <iostream>
#include <iomanip>
#include <limits>
#include <stdexcept>
#include <string>
#include <string_view>
#include <vector>

#include "../../../host-libs/cpp/polyplug/runtime.hpp"
#include "../../../host-libs/cpp/polyplug/loaders.hpp"

struct GuestSpec {
    const char* dir;
    const char* bundle_name;
    uint64_t    contract_id;
    const char* fn_name;
};

static constexpr uint64_t TRANSFORMER_CONTRACT_ID = 0x3D53C682F3F5A9EFULL;
static constexpr uint64_t REPORTER_CONTRACT_ID    = 0x81D41D43E511D297ULL;

static constexpr GuestSpec GUESTS[] = {
    { "rust/decoder",           "rust_transformer",       TRANSFORMER_CONTRACT_ID, "transform" },
    { "rust/reporter",          "rust_reporter",          REPORTER_CONTRACT_ID,    "report" },
    { "cpp/transformer",        "cpp_transformer",        TRANSFORMER_CONTRACT_ID, "transform" },
    { "cpp/reporter",           "cpp_reporter",           REPORTER_CONTRACT_ID,    "report" },
    { "csharp/encoder",         "csharp_transformer",     TRANSFORMER_CONTRACT_ID, "transform" },
    { "csharp/reporter",        "csharp_reporter",        REPORTER_CONTRACT_ID,    "report" },
    { "python/decoder",         "python_transformer",     TRANSFORMER_CONTRACT_ID, "transform" },
    { "python/reporter",        "python_reporter",        REPORTER_CONTRACT_ID,    "report" },
    { "lua/transformer",        "lua_transformer",        TRANSFORMER_CONTRACT_ID, "transform" },
    { "lua/reporter",           "lua_reporter",           REPORTER_CONTRACT_ID,    "report" },
    { "js_quickjs/transformer", "js_quickjs_transformer", TRANSFORMER_CONTRACT_ID, "transform" },
    { "js_quickjs/reporter",    "js_quickjs_reporter",    REPORTER_CONTRACT_ID,    "report" },
    { "js_deno/transformer",    "js_deno_transformer",    TRANSFORMER_CONTRACT_ID, "transform" },
    { "js_deno/reporter",       "js_deno_reporter",       REPORTER_CONTRACT_ID,    "report" },
};

static uint64_t fnv1a_64(std::string_view s) {
    constexpr uint64_t FNV_OFFSET = 0xCBF29CE484222325ULL;
    constexpr uint64_t FNV_PRIME  = 0x00000100000001B3ULL;
    uint64_t hash = FNV_OFFSET;
    for (unsigned char c : s) {
        hash ^= static_cast<uint64_t>(c);
        hash *= FNV_PRIME;
    }
    return hash;
}

static std::string read_last_error() {
    size_t len = polyplug_error_message_len();
    if (len == 0U) return std::string();
    std::vector<uint8_t> buf(len);
    size_t written = polyplug_last_error(buf.data(), buf.size());
    return std::string(reinterpret_cast<const char*>(buf.data()), written);
}

static void load_bundle(OpaqueRuntime* runtime, const std::string& path) {
    uint32_t result = polyplug_load_bundle(
        runtime,
        reinterpret_cast<const uint8_t*>(path.data()),
        path.size()
    );
    if (result != 0U) {
        std::string msg = read_last_error();
        if (msg.empty()) msg = "unknown error";
        throw std::runtime_error("load_bundle failed for " + path + ": " + msg);
    }
}

static std::string string_view_to_string(const StringView& sv) {
    if (sv.ptr == nullptr || sv.len == 0U) return std::string();
    return std::string(reinterpret_cast<const char*>(sv.ptr), sv.len);
}

int main() {
    auto rt = polyplug::Runtime::builder().build();

    polyplug::loaders::register_native(rt);
    polyplug::loaders::register_dotnet(rt);
    polyplug::loaders::register_python(rt);
    polyplug::loaders::register_lua(rt);
    polyplug::loaders::register_js(rt);
    polyplug::loaders::register_js_deno(rt);

    try {
        for (const GuestSpec& g : GUESTS) {
            std::string path = std::string("examples/guests/") + g.dir;
            load_bundle(rt.handle(), path);
        }

        for (const GuestSpec& g : GUESTS) {
            uint64_t bid = fnv1a_64(g.bundle_name);
            uint64_t packed = polyplug_rt_find_by_bundle(rt.handle(), bid, g.contract_id, 0U);
            if (packed == std::numeric_limits<uint64_t>::max()) {
                throw std::runtime_error(std::string("plugin not found: ") + g.bundle_name);
            }

            OpaqueGuard* guard = polyplug_rt_resolve_plugin(rt.handle(), packed);
            if (guard == nullptr) {
                throw std::runtime_error(std::string("resolve failed: ") + g.bundle_name);
            }

            const PluginVTable* vtable = static_cast<const PluginVTable*>(polyplug_get_vtable(guard));
            if (vtable == nullptr || vtable->functions == nullptr || vtable->function_count == 0U) {
                polyplug_guard_free(guard);
                throw std::runtime_error(std::string("null vtable: ") + g.bundle_name);
            }

            using FnPtr = AbiError (*)(const void*, void*);
            auto fn_ptr = reinterpret_cast<FnPtr>(vtable->functions[0]);

            const char* input = "hello";
            StringView input_sv;
            input_sv.ptr = reinterpret_cast<const uint8_t*>(input);
            input_sv.len = 5U;

            StringView output_sv;
            output_sv.ptr = nullptr;
            output_sv.len = 0U;

            AbiError err = fn_ptr(&input_sv, &output_sv);
            if (err.code != ABI_OK) {
                polyplug_guard_free(guard);
                throw std::runtime_error(std::string("call failed for ") + g.dir);
            }

            std::string result = string_view_to_string(output_sv);

            std::string label = std::string("[") + g.dir + "]";
            std::cout << std::left << std::setw(30) << label
                      << g.fn_name << "(\"hello\") = \"" << result << "\""
                      << std::endl;

            polyplug_guard_free(guard);
        }

        return 0;
    } catch (const std::exception& ex) {
        std::cerr << "error: " << ex.what() << std::endl;
        return 1;
    }
}
