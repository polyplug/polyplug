// THIS FILE IS PART OF polyplug — header-only C++ binding.
// Lua loader registration for the polyplug plugin runtime.

#pragma once

#include "../host/polyplug/runtime.hpp"

extern "C" {
    struct PolyplugLuaConfig { uint8_t _reserved; };
    void* polyplug_lua_loader_create(const PolyplugLuaConfig* cfg);
    void  polyplug_lua_loader_free(void* ptr);
}

namespace polyplug::loaders {

inline void register_lua(Runtime& rt) {
    PolyplugLuaConfig cfg{0};
    void* loader = polyplug_lua_loader_create(&cfg);
    if (loader == nullptr) {
        throw std::runtime_error("polyplug: lua loader create failed");
    }
    const HostInterface* host = rt.host();
    static const char runtime_name[] = "lua";
    StringView name{reinterpret_cast<const uint8_t*>(runtime_name), sizeof(runtime_name) - 1};
    AbiError err = host->register_loader(host, name, loader);
    if (err.code != static_cast<uint32_t>(AbiErrorCode::Ok)) {
        throw std::runtime_error("polyplug: lua loader register failed: " + rt.get_last_error());
    }
}

} // namespace polyplug::loaders