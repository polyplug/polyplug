#include <polyplug/runtime.hpp>
#include <loaders/native/polyplug_loaders_native.hpp>
#include <iostream>
#include <filesystem>
#include <fstream>
#include <cstdint>
#include <memory>

#include "host_callers.hpp"
#include "types.hpp"
#include "host_contracts.hpp"
#include "vtable_factories.hpp"

namespace fs = std::filesystem;

class ConsoleLogger : public polyplug_host::HostLogger {
public:
    void log(StringView message) override {
        std::string_view msg(reinterpret_cast<const char*>(message.ptr), message.len);
        std::cout << "[PLUGIN LOG] " << msg << "\n";
    }
};

int main() {
    const char* plugin_path = std::getenv("POLYPLUG_PLUGIN_PATH");
    if (!plugin_path) plugin_path = "examples/host_contracts/logger/plugins";

    std::cerr << "loading plugins from: " << plugin_path << "\n\n";

    auto rt = polyplug::Runtime::builder()
        .plugin_dir(plugin_path)
        .build();

    polyplug::loaders::register_native(rt);

    auto vtable = polyplug_host::create_host_logger_vtable(std::make_unique<ConsoleLogger>());
    rt.register_host_contract(polyplug_host::HOSTLOGGER_CONTRACT_ID, vtable);

    std::vector<std::string> bundles;
    for (const auto& entry : fs::directory_iterator(plugin_path)) {
        if (!entry.is_directory()) continue;
        std::string manifest_path = entry.path().string() + "/manifest.toml";
        if (fs::exists(manifest_path)) {
            rt.load_bundle(entry.path().string());
            bundles.push_back(entry.path().filename().string());
            std::cerr << "  loaded: " << entry.path().filename().string() << "\n";
        }
    }

    if (bundles.empty()) {
        std::cerr << "no plugins found in " << plugin_path << "\n";
        return 1;
    }

    std::cerr << "\ndiscovered " << bundles.size() << " bundles\n\n";

    std::cout << "\n=== Logger Host (C++) ===\n\n";

    const std::string input = "hello world";
    std::cout << "Input: \"" << input << "\"\n\n";

    auto cid = polyplug::fnv1a_contract_id("example.worker", 1);
    auto packed_handle = rt.find(cid, 1);

    if (packed_handle != UINT64_MAX) {
        auto guard = rt.resolve_plugin(packed_handle);
        if (guard) {
            const auto* vtable = guard.vtable();
            if (vtable && vtable->function_count > 0) {
                auto funcs = reinterpret_cast<void* const*>(vtable->functions);
                auto dispatch_fn = reinterpret_cast<uint32_t (*)(const void*, void*)>(funcs[0]);

                StringView input_sv{reinterpret_cast<const uint8_t*>(input.data()), input.size()};
                StringView output_sv{nullptr, 0};

                uint32_t err_code = dispatch_fn(&input_sv, &output_sv);

                if (err_code == 0 && output_sv.ptr && output_sv.len > 0) {
                    std::string result(reinterpret_cast<const char*>(output_sv.ptr), output_sv.len);
                    polyplug_host_free(const_cast<uint8_t*>(output_sv.ptr), output_sv.len, 1);
                    std::cout << "[host] do_work(\"" << input << "\") = \"" << result << "\"\n";
                } else {
                    std::cerr << "[host] do_work failed: error code " << err_code << "\n";
                }
            }
        }
    }

    std::cout << "\ndone.\n";
    return 0;
}