#pragma once
#include "../../polyplug/runtime.hpp"
extern "C" {
    struct PolyplugNativeConfig { uint8_t _reserved; };
    void* polyplug_native_loader_create(const PolyplugNativeConfig* cfg);
    void  polyplug_native_loader_free(void* ptr);
    uint32_t polyplug_runtime_register_loader(void* rt, void* loader);
}
namespace polyplug::loaders {
inline void register_native(Runtime& rt) {
    PolyplugNativeConfig cfg{0};
    void* loader = polyplug_native_loader_create(&cfg);
    if (!loader) throw std::runtime_error("polyplug: native loader create failed");
    uint32_t err = polyplug_runtime_register_loader(rt.handle(), loader);
    if (err != 0) throw std::runtime_error("polyplug: native loader register failed");
}
} // namespace polyplug::loaders