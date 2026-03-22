// THIS FILE IS PART OF polyplug — header-only C++ binding.
// Guest-side helper utilities for the polyplug plugin runtime.

#pragma once

#include "../../abi/polyplug/abi.hpp"

#include <cstring>
#include <string>

namespace polyplug {
namespace guest {

/// Convert StringView to std::string
inline std::string to_string(StringView sv) {
    if (!sv.ptr || sv.len == 0) return {};
    return {reinterpret_cast<const char*>(sv.ptr), sv.len};
}

/// Allocate StringView from std::string using host allocator
inline StringView alloc_string(const std::string& s) {
    auto* ptr = static_cast<uint8_t*>(polyplug_host_alloc(s.size(), 1));
    if (!ptr) {
        StringView sv{};
        sv.ptr = nullptr;
        sv.len = 0;
        return sv;
    }
    std::memcpy(ptr, s.data(), s.size());
    StringView sv{};
    sv.ptr = ptr;
    sv.len = s.size();
    return sv;
}

} // namespace guest
} // namespace polyplug