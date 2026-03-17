#include <polyplug.hpp>
#include <polyplug/helpers.hpp>
#include <iostream>
#include <filesystem>
#include <fstream>
#include <regex>
#include <vector>

namespace fs = std::filesystem;

std::string read_file(const std::string& path) {
    std::ifstream file(path);
    return {std::istreambuf_iterator<char>(file), std::istreambuf_iterator<char>()};
}

struct BundleInfo {
    std::string dir;
    std::string name;
    std::vector<std::string> provides;
};

BundleInfo parse_manifest(const std::string& content) {
    BundleInfo info;
    std::regex name_re(R"(bundle_name\s*=\s*"([^"]+)")");
    std::regex provides_re(R"(provides\s*=\s*\[([^\]]+)\])");
    std::smatch match;
    
    if (std::regex_search(content, match, name_re)) {
        info.name = match[1].str();
    }
    
    if (std::regex_search(content, match, provides_re)) {
        std::string provides_str = match[1].str();
        std::regex item_re(R"("([^"]+)")");
        auto begin = std::sregex_iterator(provides_str.begin(), provides_str.end(), item_re);
        auto end = std::sregex_iterator();
        for (auto it = begin; it != end; ++it) {
            info.provides.push_back((*it)[1].str());
        }
    }
    
    return info;
}

int main() {
    const char* plugin_path_c = std::getenv("POLYPLUG_PLUGIN_PATH");
    std::string plugin_path = plugin_path_c ? plugin_path_c : "examples/plugins";

    std::cerr << "loading plugins from: " << plugin_path << "\n\n";

    auto rt = polyplug::Runtime::builder()
        .plugin_dir(plugin_path)
        .build();

    std::vector<BundleInfo> bundle_infos;
    for (const auto& entry : fs::directory_iterator(plugin_path)) {
        if (!entry.is_directory()) continue;
        std::string manifest_path = entry.path().string() + "/manifest.toml";
        if (fs::exists(manifest_path)) {
            auto content = read_file(manifest_path);
            auto info = parse_manifest(content);
            info.dir = entry.path().string();
            bundle_infos.push_back(info);
        }
    }

    if (bundle_infos.empty()) {
        std::cerr << "no plugins found in " << plugin_path << "\n";
        return 1;
    }

    std::cerr << "discovered " << bundle_infos.size() << " bundles\n\n";

    for (auto& bundle : bundle_infos) {
        rt.load_bundle(bundle.dir);
        std::cerr << "  loaded: " << bundle.name << "\n";
    }

    std::cout << "\n=== Pipeline Host (C++) ===\n\n";

    std::string input_str = "name,value,42";
    std::cout << "Input: \"" << input_str << "\"\n\n";

    for (const auto& bundle : bundle_infos) {
        auto bid = polyplug::bundle_id(bundle.name.c_str());
        
        for (const auto& contract : bundle.provides) {
            auto at_pos = contract.find('@');
            if (at_pos == std::string::npos) continue;
            
            auto contract_name = contract.substr(0, at_pos);
            auto version_str = contract.substr(at_pos + 1);
            auto major = std::stoi(version_str);
            
            auto cid = polyplug::contract_id(contract_name.c_str(), static_cast<uint32_t>(major));
            
            std::cout << "[" << bundle.name << "] provides " << contract << "\n";
        }
    }

    std::cout << "\ndone.\n";

    return 0;
}
