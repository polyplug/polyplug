// THIS FILE IS PART OF polyplug — header-only C++ binding.
// JavaScript loader registration for the polyplug plugin runtime.

#pragma once

#include "../host/polyplug/runtime.hpp"

extern "C" {
    void* polyplug_js_loader_create();
    void  polyplug_js_loader_free(void* ptr);
}

namespace polyplug::loaders {

inline void register_js(Runtime& rt) {
    void* loader = polyplug_js_loader_create();
    if (loader == nullptr) {
        throw std::runtime_error("polyplug: js loader create failed");
    }
    const HostApi* host = rt.host();
    AbiError err{};
    host->register_loader(host, loader, &err);
    if (err.code != static_cast<uint32_t>(AbiErrorCode::Ok)) {
        throw std::runtime_error("polyplug: js loader register failed: " + rt.get_last_error());
    }
}

} // namespace polyplug::loaders