#include <polyplug/abi.hpp>
#include <polyplug/helpers.hpp>
#include <string>
#include <sstream>

using namespace polyplug;

extern "C" {

POLYPLUG_EXPORT uint32_t polyplug_abi_version() {
    return POLYPLUG_ABI_VERSION;
}

POLYPLUG_EXPORT AbiError pipeline_validator_validate(StringView input, StringView* out) {
    if (!out) return {ABI_ERROR_GENERIC, {}};
    std::string s = guest::to_string(input);
    if (s.find("DECODED:") == 0) s = s.substr(8);
    std::istringstream iss(s);
    std::string name, value, count_str;
    if (std::getline(iss, name, '|') && std::getline(iss, value, '|') && std::getline(iss, count_str, '|') && !name.empty() && !value.empty()) {
        try {
            std::stoi(count_str);
            *out = guest::alloc_string("VALID:" + s);
            return ABI_OK;
        } catch (...) {}
    }
    *out = guest::alloc_string("INVALID:expected name|value|count");
    return ABI_OK;
}

}
