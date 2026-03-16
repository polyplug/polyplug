// uppercase_transformer — C++ native plugin implementing data.Transformer v1.
// Contract: transform(data: StringView) -> StringView
// Input:  "name,value,42"
// Output: "TRANSFORMED:name|value|42"

#include "generated/guest/contracts.hpp"
#include "generated/guest/vtables.hpp"
#include "polyplug/abi.hpp"

#include <cstddef>
#include <cstdint>
#include <cstring>

namespace polyplug_plugin {

// Trace support
static constexpr uint32_t EXT_TRACE_ID = 0xC4EB9AEEu;

struct TraceVTable {
    void (*emit)(StringView msg, const void* state);
    const void* state;
};

static const TraceVTable* g_trace_vtable = nullptr;

static void emit_trace(const char* msg) noexcept {
    if (g_trace_vtable == nullptr || g_trace_vtable->emit == nullptr) {
        return;
    }
    StringView sv;
    sv.ptr = reinterpret_cast<const uint8_t*>(msg);
    sv.len = std::strlen(msg);
    g_trace_vtable->emit(sv, g_trace_vtable->state);
}

// Transformer implementation
class UppercaseTransformer : public DataTransformerPlugin {
public:
    StringView transform(StringView data) override {
        emit_trace("[uppercase_transformer] transform called");

        // Simple transformation: just return input as-is for now
        // In real implementation, would uppercase or transform the data
        return data;
    }
};

// Factory function called by generated init
DataTransformerPlugin* create_data_Transformer_impl() {
    return new UppercaseTransformer();
}

// Initialize trace extension (called before create_*_impl)
extern "C" AbiError polyplug_init(PluginRegistrar* registrar, const PluginContext* ctx) {
    if (!registrar || !ctx) {
        return AbiError{ABI_ERROR_GENERIC, StringView{nullptr, 0}};
    }

    // Get trace extension if available
    const void* trace_ext = registrar->host->get_extension(EXT_TRACE_ID);
    if (trace_ext != nullptr) {
        g_trace_vtable = reinterpret_cast<const TraceVTable*>(trace_ext);
    }

    emit_trace("[uppercase_transformer] init");

    // Call generated init to register plugins
    return ::polyplug_init(registrar, ctx);
}

}  // namespace polyplug_plugin
