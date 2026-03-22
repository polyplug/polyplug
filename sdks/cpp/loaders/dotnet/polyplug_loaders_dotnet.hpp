// THIS FILE IS PART OF polyplug — header-only C++ binding.
// .NET loader registration for the polyplug plugin runtime.

#pragma once

#include "../host/polyplug/runtime.hpp"

#include <string_view>

extern "C" {
    struct PolyplugDotnetConfig { const uint8_t* min_framework_ptr; size_t min_framework_len; };
    void* polyplug_dotnet_loader_create(const PolyplugDotnetConfig* cfg);
    void  polyplug_dotnet_loader_free(void* ptr);
    uint32_t polyplug_runtime_register_loader(void* rt, void* loader);
}

namespace polyplug::loaders {

inline void register_dotnet(Runtime& rt, std::string_view min_framework = "10.0") {
    PolyplugDotnetConfig cfg{
        reinterpret_cast<const uint8_t*>(min_framework.data()),
        min_framework.size()
    };
    void* loader = polyplug_dotnet_loader_create(&cfg);
    if (!loader) throw std::runtime_error("polyplug: dotnet loader create failed");
    uint32_t err = polyplug_runtime_register_loader(rt.handle(), loader);
    if (err != 0) throw std::runtime_error("polyplug: dotnet loader register failed");
}

} // namespace polyplug::loaders