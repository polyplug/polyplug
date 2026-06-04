#include <polyplug.hpp>
#include <polyplug_loaders_native.hpp>
#include <polyplug_loaders_python.hpp>
#include <polyplug_loaders_lua.hpp>
#include <polyplug_loaders_js.hpp>

#include "generated/host/types.hpp"
#include "generated/host/host_contracts.hpp"
#include "generated/host/interface_factories.hpp"
#include "generated/host/host_callers.hpp"

#include <cstdint>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <regex>
#include <string>
#include <string_view>
#include <vector>

namespace fs = std::filesystem;

using polyplug_generated::DATA_REPORTER_CONTRACT_ID;
using polyplug_generated::DATA_TRANSFORMER_CONTRACT_ID;
using polyplug_generated::DataReporterContract;
using polyplug_generated::DataTransformerContract;
using polyplug_generated::PIPELINE_DECODER_CONTRACT_ID;
using polyplug_generated::PIPELINE_ENCODER_CONTRACT_ID;
using polyplug_generated::PIPELINE_VALIDATOR_CONTRACT_ID;
using polyplug_generated::PipelineDecoderContract;
using polyplug_generated::PipelineEncoderContract;
using polyplug_generated::PipelineValidatorContract;
using polyplug_generated::LogLevel;
using polyplug_host::HostLogger;
using polyplug_host::create_host_logger_interface;

/// Host-side implementation of the `host.logger` contract.
class ConsoleLogger : public HostLogger {
public:
    void log(StringView message) override {
        std::cout << "[plugin] "
                  << std::string_view(reinterpret_cast<const char*>(message.ptr), message.len)
                  << "\n";
    }

    void log_with_level(const LogLevel& level, StringView message) override {
        const char* level_str = "INFO";
        switch (level) {
            case LogLevel::Debug: level_str = "DEBUG"; break;
            case LogLevel::Info:  level_str = "INFO";  break;
            case LogLevel::Warn:  level_str = "WARN";  break;
            case LogLevel::Error: level_str = "ERROR"; break;
        }
        std::cout << "[plugin][" << level_str << "] "
                  << std::string_view(reinterpret_cast<const char*>(message.ptr), message.len)
                  << "\n";
    }
};

struct BundleInfo {
    std::string dir;
    std::string name;
    std::vector<std::string> provides;
};

static std::string read_file(const std::string& path) {
    std::ifstream file(path);
    return {std::istreambuf_iterator<char>(file), std::istreambuf_iterator<char>()};
}

static BundleInfo parse_manifest(const std::string& content) {
    BundleInfo info;
    std::regex name_re(R"RE(^name\s*=\s*"([^"]+)")RE", std::regex::multiline);
    std::regex provides_re(R"RE(provides\s*=\s*\[([^\]]+)\])RE");
    std::smatch match;

    if (std::regex_search(content, match, name_re)) {
        info.name = match[1].str();
    }

    if (std::regex_search(content, match, provides_re)) {
        std::string provides_str = match[1].str();
        std::regex item_re(R"RE("([^"]+)")RE");
        auto begin = std::sregex_iterator(provides_str.begin(), provides_str.end(), item_re);
        auto end = std::sregex_iterator();
        for (auto it = begin; it != end; ++it) {
            info.provides.push_back((*it)[1].str());
        }
    }

    return info;
}

/// Convert a UTF-8 StringView returned by a contract into an owned std::string.
static std::string sv_to_string(StringView sv) {
    if (sv.ptr == nullptr || sv.len == 0) {
        return {};
    }
    return std::string(reinterpret_cast<const char*>(sv.ptr), sv.len);
}

/// Wrap a std::string_view as a borrowed StringView for passing into a contract.
static StringView as_view(std::string_view s) {
    return StringView{reinterpret_cast<const uint8_t*>(s.data()), s.size()};
}

int main() {
    const char* plugin_path_c = std::getenv("POLYPLUG_PLUGIN_PATH");
    std::string plugin_path = plugin_path_c ? plugin_path_c : "examples/plugins";

    std::cerr << "loading plugins from: " << plugin_path << "\n\n";

    // Build the runtime with a hot-reload callback registered via the builder.
    // ReloadPhaseType is an enum class — use :: scoped access.
    auto rt = polyplug::Runtime::builder()
        .plugin_dir(plugin_path)
        .on_reload([](const ReloadPhase& phase) {
            switch (phase.phase_type) {
                case ReloadPhaseType::Preparing: {
                    std::string name = polyplug::abi::to_string(phase.bundle_name);
                    std::cerr << "[HOT-RELOAD] Preparing: " << name
                              << " (bundle_id=0x" << std::hex << phase.bundle_id << std::dec << ")\n";
                    break;
                }
                case ReloadPhaseType::Reloaded: {
                    std::string name = polyplug::abi::to_string(phase.bundle_name);
                    std::cerr << "[HOT-RELOAD] Reloaded: " << name
                              << " (bundle_id=0x" << std::hex << phase.bundle_id << std::dec << ")\n";
                    break;
                }
                case ReloadPhaseType::Failed: {
                    std::string name = polyplug::abi::to_string(phase.bundle_name);
                    std::string reason = polyplug::abi::to_string(phase.reason);
                    std::cerr << "[HOT-RELOAD] Failed: " << name
                              << " (bundle_id=0x" << std::hex << phase.bundle_id << std::dec
                              << ") - " << reason << "\n";
                    break;
                }
            }
        })
        .build();

    polyplug::loaders::register_native(rt);
    polyplug::loaders::register_python(rt);
    polyplug::loaders::register_lua(rt);
    polyplug::loaders::register_js(rt);

    // Register the host.logger contract so plugins can call back into the host.
    const HostContractInterface* logger_iface =
        create_host_logger_interface(std::make_unique<ConsoleLogger>());
    rt.register_host_contract(logger_iface);

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

    std::cout << "\n=== Hot-Reload Host (C++) ===\n";
    std::cout << "[hot-reload] Callback registered — will fire on bundle reload events.\n\n";

    // Discovery printout: every bundle and the contracts it provides.
    for (const auto& bundle : bundle_infos) {
        const uint64_t bid = polyplug::bundle_id(bundle.name);

        for (const auto& contract : bundle.provides) {
            const auto at_pos = contract.find('@');
            if (at_pos == std::string::npos) continue;

            const std::string contract_name = contract.substr(0, at_pos);
            const std::string version_str = contract.substr(at_pos + 1);
            const auto major = static_cast<uint32_t>(std::stoi(version_str));

            const uint64_t cid = polyplug::guest_contract_id(contract_name, major);

            std::cout << "[" << bundle.name << "] provides " << contract
                      << " (bundle_id=0x" << std::hex << bid
                      << ", contract_id=0x" << cid << std::dec << ")\n";
        }
    }

    // Run the 5-stage pipeline using generated host callers (same as host.cpp).
    const std::string input = "name,value,42";
    std::cout << "\nInput: \"" << input << "\"\n\n";

    const HostInterface* host = rt.host();

    GuestContractHandle decoder_h = rt.find_guest_contract(PIPELINE_DECODER_CONTRACT_ID, 0);
    if (polyplug::is_valid(decoder_h)) {
        if (auto decoder = PipelineDecoderContract::create(decoder_h, host)) {
            StringView out = decoder->decode(as_view(input));
            std::cout << "[decoder] decode(\"" << input << "\") = \"" << sv_to_string(out) << "\"\n";
        }
    }

    const std::string decoded = "DECODED:name|value|42";
    GuestContractHandle transformer_h = rt.find_guest_contract(DATA_TRANSFORMER_CONTRACT_ID, 0);
    if (polyplug::is_valid(transformer_h)) {
        if (auto transformer = DataTransformerContract::create(transformer_h, host)) {
            StringView out = transformer->transform(as_view(decoded));
            std::cout << "[transformer] transform(\"" << decoded << "\") = \"" << sv_to_string(out) << "\"\n";
        }
    }

    const std::string transformed = "TRANSFORMED:NAME|value (transformed)|43";
    GuestContractHandle encoder_h = rt.find_guest_contract(PIPELINE_ENCODER_CONTRACT_ID, 0);
    if (polyplug::is_valid(encoder_h)) {
        if (auto encoder = PipelineEncoderContract::create(encoder_h, host)) {
            StringView out = encoder->encode(as_view(transformed));
            std::cout << "[encoder] encode(\"" << transformed << "\") = \"" << sv_to_string(out) << "\"\n";
        }
    }

    GuestContractHandle reporter_h = rt.find_guest_contract(DATA_REPORTER_CONTRACT_ID, 0);
    if (polyplug::is_valid(reporter_h)) {
        if (auto reporter = DataReporterContract::create(reporter_h, host)) {
            StringView out = reporter->report(as_view(transformed));
            std::cout << "[reporter] report(\"" << transformed << "\") = \"" << sv_to_string(out) << "\"\n";
        }
    }

    GuestContractHandle validator_h = rt.find_guest_contract(PIPELINE_VALIDATOR_CONTRACT_ID, 0);
    if (polyplug::is_valid(validator_h)) {
        if (auto validator = PipelineValidatorContract::create(validator_h, host)) {
            StringView out = validator->validate(as_view(decoded));
            std::cout << "[validator] validate(\"" << decoded << "\") = \"" << sv_to_string(out) << "\"\n";
        }
    }

    std::cout << "\n[hot-reload] Exiting — no bundle change detected (reload triggers externally).\n";
    std::cout << "\ndone.\n";

    return 0;
}
