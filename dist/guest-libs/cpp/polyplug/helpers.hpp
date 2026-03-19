// polyplug/helpers.hpp — String conversion helpers for guest plugins
#ifndef POLYPLUG_GUEST_HELPERS_HPP
#define POLYPLUG_GUEST_HELPERS_HPP

#include "abi.hpp"
#include <string>
#include <cstring>

namespace polyplug {
namespace guest {

/// Convert StringView to std::string
inline std::string to_string(StringView sv) {
    if (!sv.ptr || sv.len == 0) return {};
    return {reinterpret_cast<const char*>(sv.ptr), sv.len};
}

/// Allocate StringView from std::string using host allocator
inline StringView alloc_string(const std::string& s) {
    auto* ptr = polyplug_host_alloc(s.size(), 1);
    if (!ptr) return {nullptr, 0};
    std::copy(s.begin(), s.end(), ptr);
    return {ptr, s.size()};
}

} // namespace guest
} // namespace polyplug

#endif // POLYPLUG_GUEST_HELPERS_HPP
