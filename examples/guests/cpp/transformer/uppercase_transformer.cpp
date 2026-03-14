// THIS FILE IS A HAND-WRITTEN polyplug guest plugin.
// uppercase_transformer — C++ native plugin implementing data.Transformer v1.0
// Contract: data.Transformer@1  (TRANSFORMER_CONTRACT_ID = 0x3D53C682F3F5A9EF)

#include "guest-libs/cpp/polyplug_guest.hpp"

#include <cstddef>
#include <cstdint>
#include <cstdlib>
#include <cstring>

// ─── Constants ────────────────────────────────────────────────────────────────

static constexpr uint64_t TRANSFORMER_CONTRACT_ID = 0x3D53C682F3F5A9EFULL;
static constexpr uint32_t EXT_TRACE_ID             = 0xC4EB9AEEu;

// ─── Trace support ────────────────────────────────────────────────────────────

struct StringViewC {
    const uint8_t* ptr;
    size_t         len;
};

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

static StringView alloc_sv(const char* literal, size_t len) {
    char* buf = static_cast<char*>(
        polyplug_host_alloc(len + 1U, alignof(char))
    );
    std::memcpy(buf, literal, len);
    buf[len] = '\0';

    StringView result;
    result.ptr = reinterpret_cast<const uint8_t*>(buf);
    result.len = len;
    return result;
}

// ─── Transform function ───────────────────────────────────────────────────────
// Contract: transform(input: StringView) -> StringView
// Returns: "cpp:transform({input})"

static AbiError transform_fn(const void* args, void* out) noexcept {
    emit_trace("[transformer] transform called");

    if (args == nullptr || out == nullptr) {
        AbiError err;
        err.code        = ABI_ERROR_GENERIC;
        err.message.ptr = nullptr;
        err.message.len = 0U;
        return err;
    }

    const StringView* input  = static_cast<const StringView*>(args);
    StringView*       result = static_cast<StringView*>(out);

    // Build "cpp:transform({input})"
    const char* prefix = "cpp:transform(";
    const char* suffix = ")";
    size_t prefix_len = 14U;
    size_t suffix_len = 1U;
    size_t total_len  = prefix_len + input->len + suffix_len;

    char* buf = static_cast<char*>(
        polyplug_host_alloc(total_len + 1U, alignof(char))
    );
    std::memcpy(buf, prefix, prefix_len);
    if (input->len > 0U && input->ptr != nullptr) {
        std::memcpy(buf + prefix_len, input->ptr, input->len);
    }
    std::memcpy(buf + prefix_len + input->len, suffix, suffix_len);
    buf[total_len] = '\0';

    result->ptr = reinterpret_cast<const uint8_t*>(buf);
    result->len = total_len;

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
    0u,  // contract_version: v1.0
    1u,  // function_count
    reinterpret_cast<void* const*>(
        static_cast<FnPtr const*>(TRANSFORMER_FNS)
    )
};

static const PluginDescriptor TRANSFORMER_DESCRIPTOR = {
    StringView{ reinterpret_cast<const uint8_t*>("transformer-cpp"), 15U },
    StringView{ reinterpret_cast<const uint8_t*>("data.Transformer"), 16U },
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

    const void* trace_ext = registrar->host->get_extension(EXT_TRACE_ID);
    if (trace_ext != nullptr) {
        s_trace_vtable = reinterpret_cast<const TraceVTable*>(trace_ext);
    }

    emit_trace("[transformer] init");

    return registrar->register_plugin(registrar, &TRANSFORMER_DESCRIPTOR, &TRANSFORMER_VTABLE);
}
