#include "generated/guest/init.hpp"
#include <string>
#include <algorithm>

namespace polyplug_plugin {

class DecoderImpl : public PipelineDecoderGuestContract {
public:
    StringView decode(StringView input) override {
        std::string s = polyplug::abi::to_string(input);
        std::replace(s.begin(), s.end(), ',', '|');
        return polyplug::alloc_string("DECODED:" + s);
    }
};

PipelineDecoderGuestContract* create_decoder_impl() {
    return new DecoderImpl();
}

}  // namespace polyplug_plugin