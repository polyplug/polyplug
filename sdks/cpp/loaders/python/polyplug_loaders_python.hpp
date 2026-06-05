// THIS FILE IS PART OF polyplug — header-only C++ binding.
// Python loader registration for the polyplug plugin runtime.

#pragma once

#include "../host/polyplug/runtime.hpp"

#include <string_view>

extern "C" {
    struct PolyplugPythonConfig { const uint8_t* min_version_ptr; size_t min_version_len; };
    void* polyplug_python_loader_create(const PolyplugPythonConfig* cfg);
    void  polyplug_python_loader_free(void* ptr);
}

namespace polyplug::loaders {

inline void register_python(Runtime& rt, std::string_view min_version = "3.11") {
    PolyplugPythonConfig cfg{
        reinterpret_cast<const uint8_t*>(min_version.data()),
        min_version.size()
    };
    void* loader = polyplug_python_loader_create(&cfg);
    if (loader == nullptr) {
        throw std::runtime_error("polyplug: python loader create failed");
    }
    const HostInterface* host = rt.host();
    static const char runtime_name[] = "python";
    StringView name{reinterpret_cast<const uint8_t*>(runtime_name), sizeof(runtime_name) - 1};
    AbiError err = host->register_loader(host, name, loader);
    if (err.code != static_cast<uint32_t>(AbiErrorCode::Ok)) {
        throw std::runtime_error("polyplug: python loader register failed: " + rt.get_last_error());
    }
}

} // namespace polyplug::loaders