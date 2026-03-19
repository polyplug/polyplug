#include <cstring>
#include <stdexcept>
#include <string_view>

constexpr uint64_t FNV_OFFSET = 0xcbf29ce484222325ULL;
constexpr uint64_t FNV_PRIME = 0x00000100000001B3ULL;

constexpr uint64_t fnv1a_64_str(const char* s) {
    uint64_t h = FNV_OFFSET;
    while (*s) {
        h ^= static_cast<uint64_t>(static_cast<unsigned char>(*s));
        h *= FNV_PRIME;
        ++s;
    }
    return h;
}

constexpr uint64_t fnv1a_64_u32(uint32_t v, uint64_t h) {
    if (v < 10U) {
        uint64_t h2 = h ^ static_cast<uint64_t>('0' + v);
        return h2 * FNV_PRIME;
    }
    uint64_t h2 = fnv1a_64_u32(v / 10U, h);
    uint64_t h3 = h2 ^ static_cast<uint64_t>('0' + (v % 10U));
    return h3 * FNV_PRIME;
}

constexpr uint64_t contract_id(const char* name, uint32_t major_version) {
    uint64_t h = fnv1a_64_str(name);
    h = (h ^ static_cast<uint64_t>('@')) * FNV_PRIME;
    h = fnv1a_64_u32(major_version, h);
    return h;
}

constexpr uint64_t bundle_id(const char* name) {
    return fnv1a_64_str(name);
}

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

/// Call a plugin function by vtable index.
/// @param vtable Plugin vtable pointer
/// @param func_idx Function index (0-based)
/// @param input Input string
/// @return Output string from plugin
inline std::string call_plugin_fn(const PluginVTable* vtable, uint32_t func_idx, std::string_view input) {
    if (!vtable || func_idx >= vtable->function_count) {
        throw std::runtime_error("invalid function index");
    }
    
    auto funcs = reinterpret_cast<void**>(vtable->functions);
    auto func_ptr = reinterpret_cast<uint32_t (*)(const void*, void*)>(funcs[func_idx]);
    
    StringView input_sv = string_view(input);
    StringView output_sv{nullptr, 0};
    
    uint32_t err_code = func_ptr(&input_sv, &output_sv);
    
    if (err_code == 0 && output_sv.ptr && output_sv.len > 0) {
        std::string result = to_string(output_sv);
        polyplug_host_free(const_cast<uint8_t*>(output_sv.ptr), output_sv.len, 1);
        return result;
    }
    
    throw std::runtime_error("plugin returned error code=" + std::to_string(err_code));
}

} // namespace host
