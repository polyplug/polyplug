#include "generated/guest/init.hpp"
#include <polyplug/helpers.hpp>
#include <string>
#include <algorithm>

namespace polyplug_plugin {

class DecoderImpl : public PipelineDecoderPlugin {
public:
    StringView decode(StringView input) override {
        std::string s = polyplug::guest::to_string(input);
        std::replace(s.begin(), s.end(), ',', '|');
        return polyplug::guest::alloc_string("DECODED:" + s);
    }
};

PipelineDecoderPlugin* create_decoder_impl() {
    return new DecoderImpl();
}

}  // namespace polyplug_plugin