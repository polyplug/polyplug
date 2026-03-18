#pragma once
#include "../../polyplug/runtime.hpp"
extern "C" {
    struct PolyplugJsConfig { uint8_t _reserved; };
    void* polyplug_js_loader_create(const PolyplugJsConfig* cfg);
    void  polyplug_js_loader_free(void* ptr);
    uint32_t polyplug_runtime_register_loader(void* rt, void* loader);
}
namespace polyplug::loaders {
inline void register_js(Runtime& rt) {
    PolyplugJsConfig cfg{0};
    void* loader = polyplug_js_loader_create(&cfg);
    if (!loader) throw std::runtime_error("polyplug: js loader create failed");
    uint32_t err = polyplug_runtime_register_loader(rt.handle(), loader);
    if (err != 0) throw std::runtime_error("polyplug: js loader register failed");
}
} // namespace polyplug::loaders