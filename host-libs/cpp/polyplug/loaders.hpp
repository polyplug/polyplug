// THIS FILE IS PART OF polyplug — header-only C++ binding.
// Loader registration wrappers for non-native guest loaders.
//
// Usage:
//   // After creating a runtime via polyplug_runtime_new():
//   RuntimeHandle rt = polyplug_runtime_new();
//   polyplug::register_dotnet_loader(rt);
//   polyplug::register_python_loader(rt, "3.11");
//   polyplug::register_lua_loader(rt);
//   polyplug::register_js_loader(rt);
//
// Link: -lpolyplug -lpolyplug_dotnet -lpolyplug_python -lpolyplug_lua -lpolyplug_js
//
// NOTE: js_deno (V8) is intentionally excluded — V8 TLS constraints prevent
// building libpolyplug_js_deno.so as a cdylib on Linux.

#pragma once

#include "abi.hpp"

#include <cstddef>
#include <cstdint>
#include <stdexcept>
#include <string_view>

extern "C" {

/// Config for the .NET loader — specify the minimum framework version string.
struct PolyplugDotnetConfig {
    const uint8_t* min_framework_ptr;  ///< UTF-8 bytes of minimum framework string
    size_t         min_framework_len;  ///< byte count
};

/// Config for the Python loader — specify the minimum Python version string.
struct PolyplugPythonConfig {
    const uint8_t* min_version_ptr;  ///< UTF-8 bytes of minimum version string
    size_t         min_version_len;  ///< byte count
};

/// Config for the Lua loader — no required fields.
struct PolyplugLuaConfig {
    uint8_t _reserved;  ///< padding; set to 0
};

/// Config for the JS (QuickJS) loader — no required fields.
struct PolyplugJsConfig {
    uint8_t _reserved;  ///< padding; set to 0
};

/// Config for the native (Rust/C/C++) loader — no required fields.
struct PolyplugNativeConfig {
    uint8_t _reserved;  ///< padding; set to 0
};

/// Create a DotnetLoader from config.
/// Returns an opaque pointer to be passed to polyplug_runtime_register_loader.
/// On failure returns null.
/// OWNERSHIP: pass to polyplug_runtime_register_loader (transfers ownership)
///            OR free with polyplug_dotnet_loader_free (if not registering).
void* polyplug_dotnet_loader_create(const PolyplugDotnetConfig* config);

/// Free a dotnet loader without registering it. No-op on null.
void  polyplug_dotnet_loader_free(void* ptr);

/// Create a PythonLoader from config.
void* polyplug_python_loader_create(const PolyplugPythonConfig* config);

/// Free a python loader without registering it. No-op on null.
void  polyplug_python_loader_free(void* ptr);

/// Create a LuaLoader from config.
void* polyplug_lua_loader_create(const PolyplugLuaConfig* config);

/// Free a lua loader without registering it. No-op on null.
void  polyplug_lua_loader_free(void* ptr);

/// Create a JS (QuickJS) loader from config.
void* polyplug_js_loader_create(const PolyplugJsConfig* config);

/// Free a js loader without registering it. No-op on null.
void  polyplug_js_loader_free(void* ptr);

/// Create a native (Rust/C/C++) loader from config.
void* polyplug_native_loader_create(const PolyplugNativeConfig* config);

/// Free a native loader without registering it. No-op on null.
void  polyplug_native_loader_free(void* ptr);

/// Register an opaque loader pointer into the runtime.
/// loader_ptr must be produced by a polyplug_*_loader_create function.
/// Transfers ownership — do NOT call polyplug_*_loader_free after this.
/// Returns 0 on success, non-zero on error (check polyplug_last_error).
uint32_t polyplug_runtime_register_loader(OpaqueRuntime* rt, void* loader_ptr);

}  // extern "C"

namespace polyplug {

/// Register the .NET guest loader with the given runtime handle.
///
/// min_framework: minimum .NET framework version string (e.g. "10.0").
/// Throws std::runtime_error on failure.
inline void register_dotnet_loader(RuntimeHandle rt,
                                   std::string_view min_framework = "10.0")
{
    PolyplugDotnetConfig cfg{
        reinterpret_cast<const uint8_t*>(min_framework.data()),
        min_framework.size()
    };
    void* const loader = polyplug_dotnet_loader_create(&cfg);
    if (loader == nullptr) {
        throw std::runtime_error(
            "polyplug: dotnet loader create failed");
    }
    if (polyplug_runtime_register_loader(rt, loader) != 0U) {
        throw std::runtime_error(
            "polyplug: dotnet loader register failed");
    }
}

/// Register the Python guest loader with the given runtime handle.
///
/// min_version: minimum Python version string (e.g. "3.11").
/// Throws std::runtime_error on failure.
inline void register_python_loader(RuntimeHandle rt,
                                   std::string_view min_version = "3.11")
{
    PolyplugPythonConfig cfg{
        reinterpret_cast<const uint8_t*>(min_version.data()),
        min_version.size()
    };
    void* const loader = polyplug_python_loader_create(&cfg);
    if (loader == nullptr) {
        throw std::runtime_error(
            "polyplug: python loader create failed");
    }
    if (polyplug_runtime_register_loader(rt, loader) != 0U) {
        throw std::runtime_error(
            "polyplug: python loader register failed");
    }
}

/// Register the Lua guest loader with the given runtime handle.
///
/// Throws std::runtime_error on failure.
inline void register_lua_loader(RuntimeHandle rt)
{
    PolyplugLuaConfig cfg{0U};
    void* const loader = polyplug_lua_loader_create(&cfg);
    if (loader == nullptr) {
        throw std::runtime_error(
            "polyplug: lua loader create failed");
    }
    if (polyplug_runtime_register_loader(rt, loader) != 0U) {
        throw std::runtime_error(
            "polyplug: lua loader register failed");
    }
}

/// Register the JS (QuickJS) guest loader with the given runtime handle.
///
/// Throws std::runtime_error on failure.
inline void register_js_loader(RuntimeHandle rt)
{
    PolyplugJsConfig cfg{0U};
    void* const loader = polyplug_js_loader_create(&cfg);
    if (loader == nullptr) {
        throw std::runtime_error(
            "polyplug: js loader create failed");
    }
    if (polyplug_runtime_register_loader(rt, loader) != 0U) {
        throw std::runtime_error(
            "polyplug: js loader register failed");
    }
}

/// Register the native (Rust/C/C++) guest loader with the given runtime handle.
///
/// Throws std::runtime_error on failure.
inline void register_native_loader(RuntimeHandle rt)
{
    PolyplugNativeConfig cfg{0U};
    void* const loader = polyplug_native_loader_create(&cfg);
    if (loader == nullptr) {
        throw std::runtime_error(
            "polyplug: native loader create failed");
    }
    if (polyplug_runtime_register_loader(rt, loader) != 0U) {
        throw std::runtime_error(
            "polyplug: native loader register failed");
    }
}

}  // namespace polyplug
