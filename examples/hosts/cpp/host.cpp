#include <polyplug.hpp>
#include <polyplug/helpers.hpp>
#include <iostream>
#include <filesystem>
#include <fstream>

namespace fs = std::filesystem;

std::string read_file(const std::string& path) {
    std::ifstream file(path);
    std::string content((std::istreambuf_iterator<char>(file)),
                         std::istreambuf_iterator<char>());
    return content;
}

std::string extract_bundle_name(const std::string& content) {
    size_t pos = content.find("bundle_name");
    if (pos == std::string::npos) return "unknown";
    pos = content.find('"', pos);
    if (pos == std::string::npos) return "unknown";
    size_t end = content.find('"', pos + 1);
    if (end == std::string::npos) return "unknown";
    return content.substr(pos + 1, end - pos - 1);
}

int main() {
    const char* plugin_path_c = std::getenv("POLYPLUG_PLUGIN_PATH");
    std::string plugin_path = plugin_path_c ? plugin_path_c : "examples/plugins";

    std::cerr << "loading plugins from: " << plugin_path << "\n\n";

    auto rt = polyplug::Runtime::builder()
        .plugin_dir(plugin_path)
        .build();

    // Scan for manifest.toml files
    std::vector<std::string> bundles;
    for (const auto& entry : fs::directory_iterator(plugin_path)) {
        if (!entry.is_directory()) continue;
        std::string manifest_path = entry.path().string() + "/manifest.toml";
        if (fs::exists(manifest_path)) {
            bundles.push_back(entry.path().string());
        }
    }

    if (bundles.empty()) {
        std::cerr << "no plugins found in " << plugin_path << "\n";
        return 1;
    }

    std::cerr << "discovered " << bundles.size() << " bundles\n\n";

    for (const auto& bundle_dir : bundles) {
        rt.load_bundle(bundle_dir);
        std::string manifest_path = bundle_dir + "/manifest.toml";
        std::string content = read_file(manifest_path);
        std::string name = extract_bundle_name(content);
        std::cerr << "  loaded: " << name << "\n";
    }

    std::cout << "\n=== Pipeline Host (C++) ===\n\n";
    std::cout << "C++ host loaded all plugins successfully!\n";
    std::cout << "\ndone.\n";

    return 0;
}
