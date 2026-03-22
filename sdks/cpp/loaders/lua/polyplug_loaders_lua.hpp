// THIS FILE IS PART OF polyplug — header-only C++ binding.
// Lua loader registration for the polyplug plugin runtime.

#pragma once

#include "../host/polyplug/runtime.hpp"

extern "C" {
    struct PolyplugLuaConfig { uint8_t _reserved; };
    void* polyplug_lua_loader_create(const PolyplugLuaConfig* cfg);
    void  polyplug_lua_loader_free(void* ptr);
    uint32_t polyplug_runtime_register_loader(void* rt, void* loader);
}

namespace polyplug::loaders {

inline void register_lua(Runtime& rt) {
    PolyplugLuaConfig cfg{0};
    void* loader = polyplug_lua_loader_create(&cfg);
    if (!loader) throw std::runtime_error("polyplug: lua loader create failed");
    uint32_t err = polyplug_runtime_register_loader(rt.handle(), loader);
    if (err != 0) throw std::runtime_error("polyplug: lua loader register failed");
}

} // namespace polyplug::loaders