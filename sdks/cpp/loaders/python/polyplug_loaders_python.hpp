// THIS FILE IS PART OF polyplug — header-only C++ binding.
// Python loader registration for the polyplug plugin runtime.

#pragma once

#include "../host/polyplug/runtime.hpp"

#include <string_view>

extern "C" {
    struct PolyplugPythonConfig { const uint8_t* min_version_ptr; size_t min_version_len; };
    void* polyplug_python_loader_create(const PolyplugPythonConfig* cfg);
    void  polyplug_python_loader_free(void* ptr);
    uint32_t polyplug_runtime_register_loader(void* rt, void* loader);
}

namespace polyplug::loaders {

inline void register_python(Runtime& rt, std::string_view min_version = "3.11") {
    PolyplugPythonConfig cfg{
        reinterpret_cast<const uint8_t*>(min_version.data()),
        min_version.size()
    };
    void* loader = polyplug_python_loader_create(&cfg);
    if (!loader) throw std::runtime_error("polyplug: python loader create failed");
    uint32_t err = polyplug_runtime_register_loader(rt.handle(), loader);
    if (err != 0) throw std::runtime_error("polyplug: python loader register failed");
}

} // namespace polyplug::loaders