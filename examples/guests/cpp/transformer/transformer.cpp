#include <polyplug/abi.hpp>
#include <polyplug/helpers.hpp>
#include <string>
#include <sstream>

using namespace polyplug;

extern "C" {

POLYPLUG_EXPORT uint32_t polyplug_abi_version() {
    return POLYPLUG_ABI_VERSION;
}

POLYPLUG_EXPORT AbiError data_transformer_transform(StringView input, StringView* out) {
    if (!out) return {ABI_ERROR_GENERIC, {}};
    std::string s = guest::to_string(input);
    if (s.find("DECODED:") == 0) s = s.substr(8);
    std::istringstream iss(s);
    std::string name, value, count_str;
    if (std::getline(iss, name, '|') && std::getline(iss, value, '|') && std::getline(iss, count_str, '|')) {
        int count = std::stoi(count_str);
        for (auto& c : name) c = ::toupper(c);
        std::ostringstream oss;
        oss << "TRANSFORMED:" << name << "|" << value << " (transformed)|" << (count + 1);
        *out = guest::alloc_string(oss.str());
    } else {
        return {ABI_ERROR_GENERIC, {}};
    }
    return ABI_OK;
}

}
