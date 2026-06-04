#include "generated/guest/init.hpp"
#include <string>
#include <algorithm>

namespace polyplug_plugin {

class EncoderImpl : public PipelineEncoderGuestContract {
public:
    StringView encode(StringView input) override {
        std::string_view sv = polyplug::abi::strip_prefix(input, "TRANSFORMED:");
        std::string s(sv);
        std::replace(s.begin(), s.end(), '|', ',');
        return polyplug::abi::alloc_string(s);
    }
};

PipelineEncoderGuestContract* create_encoder_impl() {
    return new EncoderImpl();
}

}
