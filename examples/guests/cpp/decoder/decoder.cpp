#include <polyplug/abi.hpp>
#include <polyplug/helpers.hpp>
#include <string>
#include <algorithm>

using namespace polyplug;

extern "C" {

POLYPLUG_EXPORT uint32_t polyplug_abi_version() {
    return POLYPLUG_ABI_VERSION;
}

POLYPLUG_EXPORT AbiError pipeline_decoder_decode(StringView input, StringView* out) {
    if (!out) return {ABI_ERROR_GENERIC, {}};
    std::string s = guest::to_string(input);
    std::replace(s.begin(), s.end(), ',', '|');
    *out = guest::alloc_string("DECODED:" + s);
    return ABI_OK;
}

}
