#include "generated/guest/init.hpp"
#include <string>
#include <algorithm>

namespace polyplug_plugin {

class DecoderImpl : public PipelineDecoderGuestContract {
public:
    explicit DecoderImpl(const HostApi* host) : host_(host) {}

    StringView decode(StringView input) override {
        std::string s = polyplug::abi::to_string(input);
        std::replace(s.begin(), s.end(), ',', '|');
        return polyplug::alloc_string(host_, "DECODED:" + s);
    }

private:
    // Host handle for this runtime, captured at instance creation.
    const HostApi* host_;
};

// Factory called by the generated create_instance for every host-created
// instance. Ownership of the returned object transfers to the instance.
PipelineDecoderGuestContract* polyplug_create_decoder(const HostApi* host) {
    return new DecoderImpl(host);
}

}  // namespace polyplug_plugin
