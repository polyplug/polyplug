#include <cstring>

// Host-side helpers
namespace host {

/// Convert StringView to std::string (copies data)
inline std::string to_string(StringView sv) {
    if (!sv.ptr || sv.len == 0) return {};
    return {reinterpret_cast<const char*>(sv.ptr), sv.len};
}

/// Create StringView from string literal (borrowed)
inline StringView string_view(const char* s) {
    return {reinterpret_cast<const uint8_t*>(s), std::strlen(s)};
}

/// Create StringView from std::string (borrowed - ensure string outlives view)
inline StringView string_view(const std::string& s) {
    return {reinterpret_cast<const uint8_t*>(s.data()), s.size()};
}

} // namespace host
