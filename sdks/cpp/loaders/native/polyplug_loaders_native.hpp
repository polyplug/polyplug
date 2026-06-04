// THIS FILE IS PART OF polyplug — header-only C++ binding.
// Native loader registration for the polyplug plugin runtime.

#pragma once

#include "../host/polyplug/runtime.hpp"

extern "C" {
    struct PolyplugNativeConfig { uint8_t _reserved; };
    void* polyplug_native_loader_create(const PolyplugNativeConfig* cfg);
    void  polyplug_native_loader_free(void* ptr);
}

namespace polyplug::loaders {

inline void register_native(Runtime& rt) {
    PolyplugNativeConfig cfg{0};
    void* loader = polyplug_native_loader_create(&cfg);
    if (loader == nullptr) {
        throw std::runtime_error("polyplug: native loader create failed");
    }
    const HostInterface* host = rt.host();
    static const char runtime_name[] = "native";
    StringView name{reinterpret_cast<const uint8_t*>(runtime_name), sizeof(runtime_name) - 1};
    AbiError err = host->register_loader(host, name, loader);
    if (err.code != AbiErrorCode::Ok) {
        throw std::runtime_error("polyplug: native loader register failed: " + rt.get_last_error());
    }
}

} // namespace polyplug::loaders
