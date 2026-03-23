#include "generated/guest/init.hpp"
#include <polyplug/helpers.hpp>
#include <string>
#include <algorithm>

namespace polyplug_plugin {

class EncoderImpl : public PipelineEncoderPlugin {
public:
    StringView encode(StringView input) override {
        std::string_view sv = polyplug::abi::strip_prefix(input, "TRANSFORMED:");
        std::string s(sv);
        std::replace(s.begin(), s.end(), '|', ',');
        return polyplug::guest::alloc_string("ENCODED:" + s);
    }
};

PipelineEncoderPlugin* create_encoder_impl() {
    return new EncoderImpl();
}

}
