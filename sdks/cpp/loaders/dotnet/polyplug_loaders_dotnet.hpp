// THIS FILE IS PART OF polyplug — header-only C++ binding.
// .NET loader registration for the polyplug plugin runtime.

#pragma once

#include "../host/polyplug/runtime.hpp"

#include <string_view>

extern "C" {
    struct PolyplugDotnetConfig { const uint8_t* min_framework_ptr; size_t min_framework_len; };
    void* polyplug_dotnet_loader_create(const PolyplugDotnetConfig* cfg);
    void  polyplug_dotnet_loader_free(void* ptr);
}

namespace polyplug::loaders {

inline void register_dotnet(Runtime& rt, std::string_view min_framework = "10.0") {
    PolyplugDotnetConfig cfg{
        reinterpret_cast<const uint8_t*>(min_framework.data()),
        min_framework.size()
    };
    void* loader = polyplug_dotnet_loader_create(&cfg);
    if (loader == nullptr) {
        throw std::runtime_error("polyplug: dotnet loader create failed");
    }
    const HostApi* host = rt.host();
    static const char runtime_name[] = "dotnet";
    StringView name{reinterpret_cast<const uint8_t*>(runtime_name), sizeof(runtime_name) - 1};
    AbiError err = host->register_loader(host, name, loader);
    if (err.code != static_cast<uint32_t>(AbiErrorCode::Ok)) {
        throw std::runtime_error("polyplug: dotnet loader register failed: " + rt.get_last_error());
    }
}

} // namespace polyplug::loaders