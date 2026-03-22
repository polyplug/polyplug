// THIS FILE IS PART OF polyplug — header-only C++ binding.
// Host-side helper utilities for the polyplug plugin runtime.

#pragma once

#include "../../abi/polyplug/abi.hpp"

#include <cstring>
#include <stdexcept>
#include <string_view>

namespace polyplug {
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

/// Call a plugin function by vtable index (native dispatch only).
/// @param vtable Plugin interface pointer
/// @param func_idx Function index (0-based)
/// @param input Input string
/// @return Output string from plugin
inline std::string call_plugin_fn(const PluginInterface* vtable, uint32_t func_idx, std::string_view input) {
    if (!vtable || func_idx >= vtable->function_count) {
        throw std::runtime_error("invalid function index");
    }
    
    // Native dispatch: access via dispatch.native.functions[func_idx]
    auto funcs = reinterpret_cast<void**>(vtable->dispatch.native.functions);
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
} // namespace polyplug