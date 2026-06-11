#include "generated/guest/init.hpp"
#include <string>
#include <sstream>

namespace polyplug_plugin {

class ValidatorImpl : public PipelineValidatorGuestContract {
public:
    explicit ValidatorImpl(const HostApi* host) : host_(host) {}

    StringView validate(StringView input) override {
        std::string_view sv = polyplug::abi::strip_prefix(input, "DECODED:");
        std::string s(sv);
        std::istringstream iss(s);
        std::string name, value, count_str;
        if (std::getline(iss, name, '|') && std::getline(iss, value, '|') && std::getline(iss, count_str, '|') && !name.empty() && !value.empty()) {
            try {
                std::stoi(count_str);
                return polyplug::alloc_string(host_, "VALID:" + s);
            } catch (...) {}
        }
        return polyplug::alloc_string(host_, "INVALID:expected name|value|count");
    }

private:
    // Host handle for this runtime, captured at instance creation.
    const HostApi* host_;
};

// Factory called by the generated create_instance for every host-created
// instance. Ownership of the returned object transfers to the instance.
PipelineValidatorGuestContract* polyplug_create_validator(const HostApi* host) {
    return new ValidatorImpl(host);
}

}  // namespace polyplug_plugin
