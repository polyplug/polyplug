#include <polyplug/runtime.hpp>
#include <polyplug/loaders.hpp>
#include <polyplug/scanner.hpp>
#include <polyplug/helpers.hpp>
#include <iostream>
#include <filesystem>

namespace fs = std::filesystem;

int main() {
    const char* plugin_path = std::getenv("POLYPLUG_PLUGIN_PATH");
    if (!plugin_path) plugin_path = "examples/plugins";

    std::cerr << "loading plugins from: " << plugin_path << "\n\n";

    auto rt = polyplug::Runtime::builder()
        .plugin_dir(plugin_path)
        .loader(polyplug::NativeLoader{})
        .build();

    auto bundles = polyplug::scanner::scan_dir(plugin_path);
    if (bundles.empty()) {
        std::cerr << "no plugins found in " << plugin_path << "\n";
        return 1;
    }

    std::cerr << "discovered " << bundles.size() << " bundles\n\n";

    for (const auto& [path, manifest] : bundles) {
        rt.load_bundle(path);
        std::cerr << "  loaded: " << manifest.bundle_name << "\n";
    }

    std::cout << "\n=== Pipeline Host (C++) ===\n\n";

    for (const auto& [path, manifest] : bundles) {
        for (const auto& c : manifest.provides) {
            if (c.find("pipeline.Decoder") == 0) {
                auto handle = rt.find_by_bundle(manifest.bundle_name, "pipeline.Decoder", 1);
                if (handle) {
                    std::cout << "[" << manifest.bundle_name << "] decoder ready\n";
                }
            }
        }
    }

    std::cout << "\ndone.\n";
    return 0;
}
