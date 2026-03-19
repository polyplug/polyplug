#include <polyplug/runtime.hpp>
#include <loaders/native/polyplug_loaders_native.hpp>
#include <iostream>
#include <filesystem>
#include <fstream>
#include <regex>
#include <vector>
#include <cstdint>
#include <chrono>
#include <unordered_map>
#include <memory>
#include <mutex>

namespace fs = std::filesystem;

/// Instance tracking for hot-reload: bundle_id -> list of plugin guards.
/// Instances are cleared in Preparing phase and re-created in Reloaded phase.
static std::unordered_map<uint64_t, std::vector<std::unique_ptr<polyplug::PluginGuard>>> g_instances;
static std::mutex g_instances_mutex;

struct BundleInfo {
    std::string dir;
    std::string name;
    std::vector<std::string> provides;
};

BundleInfo parse_manifest(const std::string& content) {
    BundleInfo info;
    std::regex name_re("bundle_name\\s*=\\s*\"([^\"]+)\"");
    std::regex provides_re("provides\\s*=\\s*\\[([^\\]]+)\\]");
    std::smatch match;
    
    if (std::regex_search(content, match, name_re)) {
        info.name = match[1].str();
    }
    
    if (std::regex_search(content, match, provides_re)) {
        std::string provides_str = match[1].str();
        std::regex item_re("\"([^\"]+)\"");
        auto begin = std::sregex_iterator(provides_str.begin(), provides_str.end(), item_re);
        auto end = std::sregex_iterator();
        for (auto it = begin; it != end; ++it) {
            info.provides.push_back((*it)[1].str());
        }
    }
    
    return info;
}

int main() {
    const char* plugin_path = std::getenv("POLYPLUG_PLUGIN_PATH");
    if (!plugin_path) plugin_path = "examples/plugins";

    std::cerr << "loading plugins from: " << plugin_path << "\n\n";

    // Register hot-reload callback before creating runtime
    polyplug::Runtime::on_reload([](const ::ReloadPhase& phase) {
        switch (static_cast<::ReloadPhaseType>(phase.type)) {
            case ::ReloadPhaseType_Preparing: {
                std::string name = StringView_to_string(phase.bundle_name);
                std::cerr << "[HOT-RELOAD] Preparing: " << name
                          << " (bundle_id=0x" << std::hex << phase.bundle_id << std::dec
                          << ", retry " << phase.retry_count << ")\n";
                std::lock_guard<std::mutex> lock(g_instances_mutex);
                auto it = g_instances.find(phase.bundle_id);
                if (it != g_instances.end()) {
                    g_instances.erase(it);
                    std::cerr << "[HOT-RELOAD] Cleared instances for bundle " << name << "\n";
                }
                break;
            }
            case ::ReloadPhaseType_Reloaded: {
                std::string name = StringView_to_string(phase.bundle_name);
                std::cerr << "[HOT-RELOAD] Reloaded: " << name
                          << " (bundle_id=0x" << std::hex << phase.bundle_id << std::dec << ")\n";
                break;
            }
            case ::ReloadPhaseType_Failed: {
                std::string name = StringView_to_string(phase.bundle_name);
                std::string reason = StringView_to_string(phase.reason);
                std::cerr << "[HOT-RELOAD] Failed: " << name
                          << " (bundle_id=0x" << std::hex << phase.bundle_id << std::dec
                          << ") - " << reason << "\n";
                break;
            }
        }
    });

    // Configure hot-reload behavior (matching Rust example)
    polyplug::RuntimeConfig config{};
    config.hot_reload_max_retries = 5;
    config.hot_reload_retry_interval = std::chrono::milliseconds(200);
    config.hot_reload_abort_on_max_retries = false;
    polyplug::Runtime::set_config(config);

    auto rt = polyplug::Runtime::builder()
        .plugin_dir(plugin_path)
        .build();

    // Register native loader
    polyplug::loaders::register_native(rt);

    // Discover bundles by parsing manifest.toml files
    std::vector<BundleInfo> bundles;
    for (const auto& entry : fs::directory_iterator(plugin_path)) {
        if (!entry.is_directory()) continue;
        std::string manifest_path = entry.path().string() + "/manifest.toml";
        if (fs::exists(manifest_path)) {
            std::ifstream file(manifest_path);
            std::string content{std::istreambuf_iterator<char>(file), std::istreambuf_iterator<char>()};
            auto info = parse_manifest(content);
            info.dir = entry.path().string();
            bundles.push_back(info);
        }
    }

    if (bundles.empty()) {
        std::cerr << "no plugins found in " << plugin_path << "\n";
        return 1;
    }

    std::cerr << "discovered " << bundles.size() << " bundles\n\n";

    for (const auto& bundle : bundles) {
        rt.load_bundle(bundle.dir);
        std::cerr << "  loaded: " << bundle.name << "\n";
    }

    std::cout << "\n=== Pipeline Host (C++) ===\n\n";

    const std::string input = "name,value,42";
    std::cout << "Input: \"" << input << "\"\n\n";

    for (const auto& bundle : bundles) {
        for (const auto& contract : bundle.provides) {
            auto at_pos = contract.find('@');
            if (at_pos == std::string::npos) continue;
            
            auto contract_name = contract.substr(0, at_pos);
            auto version_str = contract.substr(at_pos + 1);
            auto major = std::stoi(version_str);
            
            if (contract_name.find("pipeline.Decoder") == 0) {
                auto cid = polyplug::fnv1a_contract_id(contract_name.c_str(), static_cast<uint32_t>(major));
                auto packed_handle = rt.find(cid, major);
                
                if (packed_handle != UINT64_MAX) {
                    auto guard = std::make_unique<polyplug::PluginGuard>(rt.resolve_plugin(packed_handle));
                    if (*guard) {
                        const auto* vtable = guard->vtable();
                        if (vtable) {
                            constexpr uint64_t FNV_OFFSET = 0xcbf29ce484222325ULL;
                            constexpr uint64_t FNV_PRIME = 0x00000100000001B3ULL;
                            uint64_t bundle_id_val = FNV_OFFSET;
                            for (char c : bundle.name) {
                                bundle_id_val ^= static_cast<uint64_t>(static_cast<unsigned char>(c));
                                bundle_id_val *= FNV_PRIME;
                            }
                            {
                                std::lock_guard<std::mutex> lock(g_instances_mutex);
                                g_instances[bundle_id_val].push_back(std::move(guard));
                            }
                            std::cout << "[" << bundle.name << "] decoder ready (vtable: valid)\n";
                            
                            if (vtable->function_count > 0) {
                                auto funcs = reinterpret_cast<void* const*>(vtable->functions);
                                auto dispatch_fn = reinterpret_cast<uint32_t (*)(const void*, void*)>(funcs[0]);
                                
                                StringView input_sv{reinterpret_cast<const uint8_t*>(input.data()), input.size()};
                                StringView output_sv{nullptr, 0};
                                
                                uint32_t err_code = dispatch_fn(&input_sv, &output_sv);
                                
                                if (err_code == 0 && output_sv.ptr && output_sv.len > 0) {
                                    std::string result(reinterpret_cast<const char*>(output_sv.ptr), output_sv.len);
                                    polyplug_host_free(const_cast<uint8_t*>(output_sv.ptr), output_sv.len, 1);
                                    std::cout << "[" << bundle.name << "] decode(\"" << input << "\") = \"" << result << "\"\n";
                                } else {
                                    std::cerr << "[" << bundle.name << "] decode failed: error code " << err_code << "\n";
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    std::cout << "\ndone.\n";
    return 0;
}
