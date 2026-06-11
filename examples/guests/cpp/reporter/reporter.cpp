#include "generated/guest/init.hpp"
#include "generated/guest/host_contracts.hpp"
#include <optional>
#include <string>
#include <string_view>
#include <sstream>

namespace polyplug_plugin {

class ReporterImpl : public DataReporterGuestContract {
public:
    explicit ReporterImpl(const HostApi* host) : host_(host) {}

    StringView report(StringView input) override {
        std::string_view full = polyplug::abi::to_string_view(input);

        // Mirror the rust reporter guest: exercise the GENERATED host.logger
        // caller when the host registered the contract. Logging is optional —
        // a host without the contract yields nullopt and the report proceeds.
        // min_version is PACKED (major << 16 | minor): request major 1, minor 0.
        std::optional<HostLoggerContract> logger =
            HostLoggerContract::from_host(host_, 0x00010000U);
        if (logger && logger->is_valid()) {
            logger->log(std::string("[plugin] Starting report for: ") + std::string(full));
            logger->log_with_level(polyplug_generated::LogLevel::Info,
                                   "[plugin] Step 1: Parsing input");
            logger->log_with_level(polyplug_generated::LogLevel::Debug,
                                   "[plugin] Input length: " + std::to_string(full.size()));
        }

        std::string_view sv = polyplug::abi::strip_prefix(input, "TRANSFORMED:");
        std::string s(sv);
        std::istringstream iss(s);
        std::string name, value, count_str;

        if (logger && logger->is_valid()) {
            logger->log_with_level(polyplug_generated::LogLevel::Warn,
                                   "[plugin] Step 2: Processing data");
        }

        if (std::getline(iss, name, '|') && std::getline(iss, value, '|') && std::getline(iss, count_str, '|')) {
            if (logger && logger->is_valid()) {
                logger->log_with_level(polyplug_generated::LogLevel::Error,
                                       "[plugin] Step 3: Finalizing report");
            }
            std::ostringstream oss;
            oss << "Report: " << name << " has value '" << value << "' with count " << count_str;
            return polyplug::alloc_string(host_, oss.str());
        }
        return polyplug::alloc_string(host_, "ERROR:invalid input format");
    }

private:
    // Host handle for this runtime, captured at instance creation.
    const HostApi* host_;
};

// Factory called by the generated create_instance for every host-created
// instance. Ownership of the returned object transfers to the instance.
DataReporterGuestContract* polyplug_create_reporter(const HostApi* host) {
    return new ReporterImpl(host);
}

}  // namespace polyplug_plugin
