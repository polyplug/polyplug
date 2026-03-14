#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <iostream>
#include <iomanip>
#include <limits>
#include <stdexcept>
#include <string>
#include <string_view>
#include <vector>
#include <filesystem>

#include "../../../host-libs/cpp/polyplug/runtime.hpp"
#include "../../../host-libs/cpp/polyplug/loaders.hpp"

static constexpr uint64_t TRANSFORMER_CONTRACT_ID = 0x3D53C682F3F5A9EFULL;
static constexpr uint64_t REPORTER_CONTRACT_ID    = 0x81D41D43E511D297ULL;

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

static std::string resolve_plugin_path() {
    const char* env = std::getenv("POLYPLUG_PLUGIN_PATH");
    if (env != nullptr && std::strlen(env) > 0U) {
        return std::string(env);
    }
    return "examples/plugins";
}

struct DiscoveredBundle {
    std::string path;
    std::string bundle_name;
    std::string runtime;
    std::vector<std::string> provides;
};

static std::vector<DiscoveredBundle> scan_plugin_dir(const std::string& dir) {
    std::vector<DiscoveredBundle> bundles;
    namespace fs = std::filesystem;

    if (!fs::is_directory(dir)) return bundles;

    for (const auto& entry : fs::directory_iterator(dir)) {
        if (!entry.is_directory()) continue;

        auto manifest_path = entry.path() / "manifest.toml";
        if (!fs::exists(manifest_path)) continue;

        DiscoveredBundle b;
        b.path = entry.path().string();

        std::ifstream manifest_file(manifest_path);
        if (!manifest_file.is_open()) continue;

        std::string line;
        while (std::getline(manifest_file, line)) {
            auto extract_value = [&](const std::string& key) -> std::string {
                auto pos = line.find(key);
                if (pos == std::string::npos) return "";
                auto eq = line.find('=', pos + key.size());
                if (eq == std::string::npos) return "";
                auto start = line.find('"', eq);
                if (start == std::string::npos) return "";
                auto end = line.find('"', start + 1);
                if (end == std::string::npos) return "";
                return line.substr(start + 1, end - start - 1);
            };

            auto bn = extract_value("bundle_name");
            if (!bn.empty()) b.bundle_name = bn;

            auto rt = extract_value("runtime");
            if (!rt.empty()) b.runtime = rt;

            if (line.find("provides") != std::string::npos) {
                auto start = line.find('[');
                auto end = line.find(']');
                if (start != std::string::npos && end != std::string::npos) {
                    std::string items = line.substr(start + 1, end - start - 1);
                    size_t pos = 0;
                    while ((pos = items.find('"')) != std::string::npos) {
                        auto close = items.find('"', pos + 1);
                        if (close == std::string::npos) break;
                        b.provides.push_back(items.substr(pos + 1, close - pos - 1));
                        items = items.substr(close + 1);
                    }
                }
            }
        }

        if (!b.bundle_name.empty()) {
            bundles.push_back(std::move(b));
        }
    }

    std::sort(bundles.begin(), bundles.end(),
        [](const DiscoveredBundle& a, const DiscoveredBundle& b) {
            return a.bundle_name < b.bundle_name;
        });

    return bundles;
}

int main() {
    std::string plugin_dir = resolve_plugin_path();
    std::cerr << "plugin directory: " << plugin_dir << std::endl;

    auto rt = polyplug::Runtime::builder().build();

    polyplug::loaders::register_native(rt);
    polyplug::loaders::register_dotnet(rt);
    polyplug::loaders::register_python(rt);
    polyplug::loaders::register_lua(rt);
    polyplug::loaders::register_js(rt);
    polyplug::loaders::register_js_deno(rt);

    try {
        auto bundles = scan_plugin_dir(plugin_dir);
        if (bundles.empty()) {
            std::cerr << "no plugins found in " << plugin_dir
                      << ". Run examples/build_all.sh first." << std::endl;
            return 1;
        }

        std::cerr << "discovered " << bundles.size() << " bundles" << std::endl;

        for (const auto& b : bundles) {
            load_bundle(rt.handle(), b.path);
            std::cerr << "  loaded: " << b.bundle_name << std::endl;
        }

        for (const auto& b : bundles) {
            uint64_t contract_id = 0;
            const char* fn_name = nullptr;

            for (const auto& contract : b.provides) {
                if (contract == "data.Transformer") {
                    contract_id = TRANSFORMER_CONTRACT_ID;
                    fn_name = "transform";
                    break;
                } else if (contract == "data.Reporter") {
                    contract_id = REPORTER_CONTRACT_ID;
                    fn_name = "report";
                    break;
                }
            }

            if (contract_id == 0 || fn_name == nullptr) continue;

            uint64_t bid = fnv1a_64(b.bundle_name);
            uint64_t packed = polyplug_rt_find_by_bundle(rt.handle(), bid, contract_id, 0U);
            if (packed == std::numeric_limits<uint64_t>::max()) {
                throw std::runtime_error(std::string("plugin not found: ") + b.bundle_name);
            }

            OpaqueGuard* guard = polyplug_rt_resolve_plugin(rt.handle(), packed);
            if (guard == nullptr) {
                throw std::runtime_error(std::string("resolve failed: ") + b.bundle_name);
            }

            const PluginVTable* vtable = static_cast<const PluginVTable*>(polyplug_get_vtable(guard));
            if (vtable == nullptr || vtable->functions == nullptr || vtable->function_count == 0U) {
                polyplug_guard_free(guard);
                throw std::runtime_error(std::string("null vtable: ") + b.bundle_name);
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
                throw std::runtime_error(std::string("call failed for ") + b.bundle_name);
            }

            std::string result = string_view_to_string(output_sv);
            std::string label = std::string("[") + b.bundle_name + "]";
            std::cout << std::left << std::setw(30) << label
                      << fn_name << "(\"hello\") = \"" << result << "\""
                      << std::endl;

            polyplug_guard_free(guard);
        }

        return 0;
    } catch (const std::exception& ex) {
        std::cerr << "error: " << ex.what() << std::endl;
        return 1;
    }
}
