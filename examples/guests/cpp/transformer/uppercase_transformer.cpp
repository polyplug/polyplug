// THIS FILE IS A HAND-WRITTEN polyplug guest plugin.
// uppercase_transformer — C++ native plugin implementing pipeline.transformer v1.0
// Contract: pipeline.transformer@1  (TRANSFORMER_CONTRACT_ID = 0x0E3044133E12EB05)

#include "guest-libs/cpp/polyplug_guest.hpp"

#include <cctype>
#include <cstddef>
#include <cstdint>
#include <cstdlib>
#include <cstring>

// ─── DataRecord — mirrors examples/abi_types.md layout ───────────────────────
// Layout verified: name@0[16], value@16[16], count@32[4], _pad@36[4]  total=40
struct DataRecord {
    StringView name;
    StringView value;
    uint32_t   count;
};

// ─── Constants ────────────────────────────────────────────────────────────────

static constexpr uint64_t TRANSFORMER_CONTRACT_ID = 0x0E3044133E12EB05ULL;
static constexpr uint32_t EXT_TRACE_ID             = 0xC4EB9AEEu;

// ─── Trace support ────────────────────────────────────────────────────────────
//
// TraceVTable ABI (Rust, crates/polyplug/src/extensions/trace/mod.rs):
//   struct TraceVTable { emit: fn(StringView, *const ()), state: *const () }
// StringView = { ptr: const uint8_t*, len: size_t }
// So emit args on x86-64: rdi=ptr, rsi=len, rdx=state

struct StringViewC {
    const uint8_t* ptr;
    size_t         len;
};

// Matches Rust TraceVTable layout exactly.
struct TraceVTable {
    void (*emit)(StringViewC msg, const void* state);
    const void* state;
};

static const TraceVTable* s_trace_vtable = nullptr;

static void emit_trace(const char* msg) noexcept {
    if (s_trace_vtable == nullptr || s_trace_vtable->emit == nullptr) {
        return;
    }
    StringViewC sv;
    sv.ptr = reinterpret_cast<const uint8_t*>(msg);
    sv.len = std::strlen(msg);
    s_trace_vtable->emit(sv, s_trace_vtable->state);
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Produce an uppercase copy of sv.
/// Caller is responsible for freeing buf via polyplug_host_free when done.
/// Returns a StringView pointing to the newly allocated buffer.
static StringView uppercase_sv(StringView sv) {
    char* buf = static_cast<char*>(
        polyplug_host_alloc(sv.len + 1U, alignof(char))
    );
    for (size_t i = 0U; i < sv.len; ++i) {
        buf[i] = static_cast<char>(
            std::toupper(static_cast<unsigned char>(sv.ptr[i]))
        );
    }
    buf[sv.len] = '\0';

    StringView result;
    result.ptr = reinterpret_cast<const uint8_t*>(buf);
    result.len = sv.len;
    return result;
}

// ─── Transform function ───────────────────────────────────────────────────────

static AbiError transform_fn(const void* args, void* out) noexcept {
    emit_trace("[uppercase_transformer] transform called");

    if (args == nullptr || out == nullptr) {
        AbiError err;
        err.code        = ABI_ERROR_GENERIC;
        err.message.ptr = nullptr;
        err.message.len = 0U;
        return err;
    }

    const DataRecord* record = static_cast<const DataRecord*>(args);
    DataRecord*       result = static_cast<DataRecord*>(out);

    result->name  = uppercase_sv(record->name);
    result->value = uppercase_sv(record->value);
    result->count = record->count;

    AbiError ok;
    ok.code        = ABI_OK;
    ok.message.ptr = nullptr;
    ok.message.len = 0U;
    return ok;
}

// ─── Static VTable and Descriptor ────────────────────────────────────────────

using FnPtr = AbiError (*)(const void*, void*);

static FnPtr const TRANSFORMER_FNS[] = { &transform_fn };

static PluginVTable TRANSFORMER_VTABLE = {
    TRANSFORMER_CONTRACT_ID,
    0u,  // contract_version: v1.0 → (minor << 16 | patch) = 0
    1u,  // function_count
    reinterpret_cast<void* const*>(
        static_cast<FnPtr const*>(TRANSFORMER_FNS)
    )
};

static const PluginDescriptor TRANSFORMER_DESCRIPTOR = {
    StringView{ reinterpret_cast<const uint8_t*>("uppercase-transformer-cpp"), 25U },
    StringView{ reinterpret_cast<const uint8_t*>("pipeline.transformer"),      20U },
    1u,  // version_major
    0u,  // version_minor
    0u   // version_patch
};

// ─── ABI exports ─────────────────────────────────────────────────────────────

extern "C" {

uint32_t polyplug_abi_version() {
    return POLYPLUG_ABI_VERSION;
}

}  // extern "C"

POLYPLUG_GUEST_MAIN {
    if (registrar == nullptr) {
        AbiError err;
        err.code        = ABI_ERROR_GENERIC;
        err.message.ptr = nullptr;
        err.message.len = 0U;
        return err;
    }

    // Acquire trace extension if available.
    // TraceVTable layout: { emit: fn(StringViewC, *const ()), state: *const () }
    const void* trace_ext = registrar->host->get_extension(EXT_TRACE_ID);
    if (trace_ext != nullptr) {
        s_trace_vtable = reinterpret_cast<const TraceVTable*>(trace_ext);
    }

    emit_trace("[uppercase_transformer] init");

    return registrar->register_plugin(registrar, &TRANSFORMER_DESCRIPTOR, &TRANSFORMER_VTABLE);
}
