#include "generated/guest/init.hpp"
#include <string>
#include <algorithm>

namespace polyplug_plugin {

class EncoderImpl : public PipelineEncoderGuestContract {
public:
    explicit EncoderImpl(const HostApi* host) : host_(host) {}

    StringView encode(StringView input) override {
        std::string_view sv = polyplug::abi::strip_prefix(input, "TRANSFORMED:");
        std::string s(sv);
        std::replace(s.begin(), s.end(), '|', ',');
        return polyplug::alloc_string(host_, s);
    }

private:
    // Host handle for this runtime, captured at instance creation.
    const HostApi* host_;
};

// Factory called by the generated create_instance for every host-created
// instance. Ownership of the returned object transfers to the instance.
PipelineEncoderGuestContract* polyplug_create_encoder(const HostApi* host) {
    return new EncoderImpl(host);
}

}  // namespace polyplug_plugin
