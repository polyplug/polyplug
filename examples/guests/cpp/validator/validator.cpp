// THIS FILE IS A HAND-WRITTEN polyplug guest plugin.
// validator — C++ native plugin implementing pipeline.Validator v1.0
// Contract: pipeline.Validator@1  (VALIDATOR_CONTRACT_ID = 0xA553FAB5D11C7AF0)

#include "guest-libs/cpp/polyplug_guest.hpp"

#include <cstddef>
#include <cstdint>
#include <cstdlib>
#include <cstring>

// ─── Constants ────────────────────────────────────────────────────────────────

static constexpr uint64_t VALIDATOR_CONTRACT_ID = 0xA553FAB5D11C7AF0ULL;
static constexpr uint32_t EXT_TRACE_ID          = 0xC4EB9AEEu;

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

// ─── Validate function ───────────────────────────────────────────────────────
// Contract: validate(data: StringView) -> StringView
// Input:  "DECODED:name|value|42"
// Output: "VALID:name|value|42" or "INVALID:reason"

static AbiError validate_fn(const void* args, void* out) noexcept {
    emit_trace("[validator] validate called");

    if (args == nullptr || out == nullptr) {
        AbiError err;
        err.code        = ABI_ERROR_GENERIC;
        err.message.ptr = nullptr;
        err.message.len = 0U;
        return err;
    }

    const StringView* input  = static_cast<const StringView*>(args);
    StringView*       result = static_cast<StringView*>(out);

    const char* data = reinterpret_cast<const char*>(input->ptr);
    size_t      len  = input->len;

    // Check for "DECODED:" prefix (8 chars)
    static const char DECODED_PREFIX[] = "DECODED:";
    static constexpr size_t PREFIX_LEN = 8U;

    if (len < PREFIX_LEN || std::memcmp(data, DECODED_PREFIX, PREFIX_LEN) != 0) {
        const char* inv = "INVALID:missing DECODED: prefix";
        *result = alloc_sv(inv, std::strlen(inv));

        AbiError ok;
        ok.code        = ABI_OK;
        ok.message.ptr = nullptr;
        ok.message.len = 0U;
        return ok;
    }

    // Count pipe separators after prefix — expect exactly 2 for "name|value|42"
    size_t pipe_count = 0U;
    for (size_t i = PREFIX_LEN; i < len; ++i) {
        if (data[i] == '|') {
            ++pipe_count;
        }
    }

    if (pipe_count != 2U) {
        const char* inv = "INVALID:expected 3 pipe-separated fields";
        *result = alloc_sv(inv, std::strlen(inv));

        AbiError ok;
        ok.code        = ABI_OK;
        ok.message.ptr = nullptr;
        ok.message.len = 0U;
        return ok;
    }

    // Format is valid — build "VALID:" + payload after DECODED: prefix
    static const char VALID_PREFIX[] = "VALID:";
    static constexpr size_t VALID_PREFIX_LEN = 6U;
    size_t payload_len = len - PREFIX_LEN;
    size_t total_len   = VALID_PREFIX_LEN + payload_len;

    char* buf = static_cast<char*>(
        polyplug_host_alloc(total_len + 1U, alignof(char))
    );
    std::memcpy(buf, VALID_PREFIX, VALID_PREFIX_LEN);
    std::memcpy(buf + VALID_PREFIX_LEN, data + PREFIX_LEN, payload_len);
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

static FnPtr const VALIDATOR_FNS[] = { &validate_fn };

static PluginVTable VALIDATOR_VTABLE = {
    VALIDATOR_CONTRACT_ID,
    0u,  // contract_version: v1.0
    1u,  // function_count
    reinterpret_cast<void* const*>(
        static_cast<FnPtr const*>(VALIDATOR_FNS)
    )
};

static const PluginDescriptor VALIDATOR_DESCRIPTOR = {
    StringView{ reinterpret_cast<const uint8_t*>("validator-cpp"), 13U },
    StringView{ reinterpret_cast<const uint8_t*>("pipeline.Validator"), 18U },
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

    emit_trace("[validator] init");

    return registrar->register_plugin(registrar, &VALIDATOR_DESCRIPTOR, &VALIDATOR_VTABLE);
}
