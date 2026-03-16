#include <polyplug/abi.hpp>
#include <polyplug/helpers.hpp>
#include <string>
#include <algorithm>

using namespace polyplug;

extern "C" {

POLYPLUG_EXPORT uint32_t polyplug_abi_version() {
    return POLYPLUG_ABI_VERSION;
}

POLYPLUG_EXPORT AbiError pipeline_encoder_encode(StringView input, StringView* out) {
    if (!out) return {ABI_ERROR_GENERIC, {}};
    std::string s = guest::to_string(input);
    if (s.find("TRANSFORMED:") == 0) s = s.substr(12);
    std::replace(s.begin(), s.end(), '|', ',');
    *out = guest::alloc_string(s);
    return ABI_OK;
}

}
