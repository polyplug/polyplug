// THIS FILE IS PART OF polyplug — header-only C++ binding.
// Lua loader registration for the polyplug plugin runtime.

#pragma once

#include "../host/polyplug/runtime.hpp"

extern "C" {
    void* polyplug_lua_loader_create();
    void  polyplug_lua_loader_free(void* ptr);
}

namespace polyplug::loaders {

inline void register_lua(Runtime& rt) {
    void* loader = polyplug_lua_loader_create();
    if (loader == nullptr) {
        throw std::runtime_error("polyplug: lua loader create failed");
    }
    const HostApi* host = rt.host();
    AbiError err{};
    host->register_loader(host, loader, &err);
    if (err.code != static_cast<uint32_t>(AbiErrorCode::Ok)) {
        throw std::runtime_error("polyplug: lua loader register failed: " + rt.get_last_error());
    }
}

} // namespace polyplug::loaders