// cpp_reporter — C++ native plugin implementing data.Reporter v1.
// Contract: report(data: StringView) -> StringView
// Input:  "name,value,42"
// Output: "REPORTED:name|value|42"

#include "generated/guest/contracts.hpp"
#include "generated/guest/vtables.hpp"
#include "polyplug/abi.hpp"

#include <cstddef>
#include <cstdint>
#include <cstring>

namespace polyplug_plugin {

static constexpr uint32_t EXT_TRACE_ID = 0xC4EB9AEEu;

struct TraceVTable {
    void (*emit)(StringView msg, const void* state);
    const void* state;
};

static const TraceVTable* g_trace_vtable = nullptr;

static void emit_trace(const char* msg) noexcept {
    if (g_trace_vtable == nullptr || g_trace_vtable->emit == nullptr) return;
    StringView sv{reinterpret_cast<const uint8_t*>(msg), std::strlen(msg)};
    g_trace_vtable->emit(sv, g_trace_vtable->state);
}

class Reporter : public DataReporterPlugin {
public:
    StringView report(StringView data) override {
        emit_trace("[cpp_reporter] report called");
        return data;
    }
};

DataReporterPlugin* create_data_Reporter_impl() {
    return new Reporter();
}

extern "C" AbiError polyplug_init(PluginRegistrar* registrar, const PluginContext* ctx) {
    if (!registrar || !ctx) return AbiError{ABI_ERROR_GENERIC, StringView{nullptr, 0}};
    
    const void* trace_ext = registrar->host->get_extension(EXT_TRACE_ID);
    if (trace_ext) g_trace_vtable = reinterpret_cast<const TraceVTable*>(trace_ext);
    
    emit_trace("[cpp_reporter] init");
    return ::polyplug_init(registrar, ctx);
}

}  // namespace polyplug_plugin
