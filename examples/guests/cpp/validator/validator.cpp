#include "generated/guest/init.hpp"
#include <string>
#include <sstream>

namespace polyplug_plugin {

class ValidatorImpl : public PipelineValidatorGuestContract {
public:
    StringView validate(StringView input) override {
        std::string_view sv = polyplug::abi::strip_prefix(input, "DECODED:");
        std::string s(sv);
        std::istringstream iss(s);
        std::string name, value, count_str;
        if (std::getline(iss, name, '|') && std::getline(iss, value, '|') && std::getline(iss, count_str, '|') && !name.empty() && !value.empty()) {
            try {
                std::stoi(count_str);
                return polyplug::alloc_string("VALID:" + s);
            } catch (...) {}
        }
        return polyplug::alloc_string("INVALID:expected name|value|count");
    }
};

PipelineValidatorGuestContract* create_validator_impl() {
    return new ValidatorImpl();
}

}  // namespace polyplug_plugin