#include <polyplug/abi.hpp>
#include <polyplug/helpers.hpp>
#include <string>
#include <sstream>

using namespace polyplug;

extern "C" {

POLYPLUG_EXPORT uint32_t polyplug_abi_version() {
    return POLYPLUG_ABI_VERSION;
}

POLYPLUG_EXPORT AbiError data_reporter_report(StringView input, StringView* out) {
    if (!out) return {ABI_ERROR_GENERIC, {}};
    std::string s = guest::to_string(input);
    if (s.find("TRANSFORMED:") == 0) s = s.substr(12);
    std::istringstream iss(s);
    std::string name, value, count_str;
    if (std::getline(iss, name, '|') && std::getline(iss, value, '|') && std::getline(iss, count_str, '|')) {
        std::ostringstream oss;
        oss << "Report: " << name << " has value '" << value << "' with count " << count_str;
        *out = guest::alloc_string(oss.str());
    } else {
        return {ABI_ERROR_GENERIC, {}};
    }
    return ABI_OK;
}

}
