#include <polyplug/runtime.hpp>
#include <loaders/native/polyplug_loaders_native.hpp>
#include <polyplug/helpers.hpp>
#include <iostream>
#include <filesystem>
#include <fstream>
#include <regex>
#include <vector>
#include <cstdint>

namespace fs = std::filesystem;

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

    // Demonstrate PluginGuard API with find() and resolve_plugin()
    for (const auto& bundle : bundles) {
        for (const auto& contract : bundle.provides) {
            auto at_pos = contract.find('@');
            if (at_pos == std::string::npos) continue;
            
            auto contract_name = contract.substr(0, at_pos);
            auto version_str = contract.substr(at_pos + 1);
            auto major = std::stoi(version_str);
            
            if (contract_name.find("pipeline.Decoder") == 0) {
                auto cid = contract_id(contract_name.c_str(), static_cast<uint32_t>(major));
                auto packed_handle = rt.find(cid, major);
                
                if (packed_handle != UINT64_MAX) {
                    auto guard = rt.resolve_plugin(packed_handle);
                    if (guard) {
                        std::cout << "[" << bundle.name << "] decoder ready (vtable: " 
                                  << (guard.vtable() ? "valid" : "null") << ")\n";
                    }
                }
            }
        }
    }

    std::cout << "\ndone.\n";
    return 0;
}
