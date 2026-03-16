// THIS FILE IS A HAND-WRITTEN polyplug guest plugin.
// encoder — C++ native plugin implementing pipeline.Encoder v1.0
// Contract: pipeline.Encoder@1  (ENCODER_CONTRACT_ID = 0x127D1703C6EFB432)
//
// Input:  "TRANSFORMED:NAME|value (transformed)|43"
// Output: "NAME,value (transformed),43"

#include "guest-libs/cpp/polyplug_guest.hpp"

#include <cstddef>
#include <cstdint>
#include <cstdlib>
#include <cstring>

// ─── Constants ────────────────────────────────────────────────────────────────

static constexpr uint64_t ENCODER_CONTRACT_ID = 0x127D1703C6EFB432ULL;
static constexpr uint32_t EXT_TRACE_ID        = 0xC4EB9AEEu;

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

static StringView alloc_sv(const char* data, size_t len) {
    char* buf = static_cast<char*>(
        polyplug_host_alloc(len + 1U, alignof(char))
    );
    std::memcpy(buf, data, len);
    buf[len] = '\0';

    StringView result;
    result.ptr = reinterpret_cast<const uint8_t*>(buf);
    result.len = len;
    return result;
}

// ─── Encode function ──────────────────────────────────────────────────────────
// Contract: encode(data: StringView) -> StringView
// Input:  "TRANSFORMED:NAME|value (transformed)|43"
// Output: "NAME,value (transformed),43"

static AbiError encode_fn(const void* args, void* out) noexcept {
    emit_trace("[encoder] encode called");

    if (args == nullptr || out == nullptr) {
        AbiError err;
        err.code        = ABI_ERROR_GENERIC;
        err.message.ptr = nullptr;
        err.message.len = 0U;
        return err;
    }

    const StringView* input  = static_cast<const StringView*>(args);
    StringView*       result = static_cast<StringView*>(out);

    const char* src = reinterpret_cast<const char*>(input->ptr);
    size_t src_len  = input->len;

    // Strip "TRANSFORMED:" prefix
    static constexpr char PREFIX[]  = "TRANSFORMED:";
    static constexpr size_t PREFIX_LEN = 12U;

    if (src_len < PREFIX_LEN || std::memcmp(src, PREFIX, PREFIX_LEN) != 0) {
        // Missing prefix — return error
        static constexpr char ERR_MSG[] = "expected TRANSFORMED: prefix";
        AbiError err;
        err.code    = ABI_ERROR_GENERIC;
        err.message = alloc_sv(ERR_MSG, sizeof(ERR_MSG) - 1U);
        return err;
    }

    const char* body     = src + PREFIX_LEN;
    size_t body_len      = src_len - PREFIX_LEN;

    // Find the two '|' separators in "NAME|value (transformed)|43"
    size_t first_pipe  = 0U;
    size_t second_pipe = 0U;
    bool found_first   = false;
    bool found_second  = false;

    for (size_t i = 0U; i < body_len; ++i) {
        if (body[i] == '|') {
            if (!found_first) {
                first_pipe  = i;
                found_first = true;
            } else {
                second_pipe = i;
                found_second = true;
                break;
            }
        }
    }

    if (!found_first || !found_second) {
        static constexpr char ERR_MSG[] = "malformed TRANSFORMED payload: expected two | separators";
        AbiError err;
        err.code    = ABI_ERROR_GENERIC;
        err.message = alloc_sv(ERR_MSG, sizeof(ERR_MSG) - 1U);
        return err;
    }

    // Extract fields: NAME, value (transformed), 43
    const char* field1     = body;
    size_t field1_len      = first_pipe;

    const char* field2     = body + first_pipe + 1U;
    size_t field2_len      = second_pipe - first_pipe - 1U;

    const char* field3     = body + second_pipe + 1U;
    size_t field3_len      = body_len - second_pipe - 1U;

    // Build CSV: "NAME,value (transformed),43"
    size_t total_len = field1_len + 1U + field2_len + 1U + field3_len;

    char* buf = static_cast<char*>(
        polyplug_host_alloc(total_len + 1U, alignof(char))
    );

    size_t offset = 0U;
    std::memcpy(buf + offset, field1, field1_len);
    offset += field1_len;
    buf[offset++] = ',';
    std::memcpy(buf + offset, field2, field2_len);
    offset += field2_len;
    buf[offset++] = ',';
    std::memcpy(buf + offset, field3, field3_len);
    offset += field3_len;
    buf[offset] = '\0';

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

static FnPtr const ENCODER_FNS[] = { &encode_fn };

static PluginVTable ENCODER_VTABLE = {
    ENCODER_CONTRACT_ID,
    0u,  // contract_version: v1.0
    1u,  // function_count
    reinterpret_cast<void* const*>(
        static_cast<FnPtr const*>(ENCODER_FNS)
    )
};

static const PluginDescriptor ENCODER_DESCRIPTOR = {
    StringView{ reinterpret_cast<const uint8_t*>("encoder-cpp"), 11U },
    StringView{ reinterpret_cast<const uint8_t*>("pipeline.Encoder"), 16U },
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

    emit_trace("[encoder] init");

    return registrar->register_plugin(registrar, &ENCODER_DESCRIPTOR, &ENCODER_VTABLE);
}
