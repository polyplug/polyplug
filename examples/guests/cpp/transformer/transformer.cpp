#include "generated/guest/init.hpp"
#include <string>
#include <sstream>

namespace polyplug_plugin {

class TransformerImpl : public DataTransformerGuestContract {
public:
    explicit TransformerImpl(const HostApi* host) : host_(host) {}

    StringView transform(StringView input) override {
        std::string_view sv = polyplug::abi::strip_prefix(input, "DECODED:");
        std::string s(sv);
        std::istringstream iss(s);
        std::string name, value, count_str;
        if (std::getline(iss, name, '|') && std::getline(iss, value, '|') && std::getline(iss, count_str, '|')) {
            int count = std::stoi(count_str);
            for (auto& c : name) c = ::toupper(c);
            std::ostringstream oss;
            oss << "TRANSFORMED:" << name << "|" << value << " (transformed)|" << (count + 1);
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
DataTransformerGuestContract* polyplug_create_transformer(const HostApi* host) {
    return new TransformerImpl(host);
}

}  // namespace polyplug_plugin
