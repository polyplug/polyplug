// THIS FILE IS A HAND-WRITTEN polyplug guest plugin.
// validator — C++ native plugin implementing pipeline.validator v1.0
// Contract: pipeline.validator@1  (VALIDATOR_CONTRACT_ID = 0x027ABCEBF8020D90)

#include "guest-libs/cpp/polyplug_guest.hpp"

#include <cctype>
#include <cstddef>
#include <cstdint>
#include <cstdlib>
#include <cstring>

// ─── DataRecord — mirrors showcase/abi_types.md layout ───────────────────────
// Layout verified: name@0[16], value@16[16], count@32[4], _pad@36[4]  total=40
struct DataRecord {
    StringView name;
    StringView value;
    uint32_t   count;
};

// ─── ValidationResult — output type for validator ─────────────────────────────
// Layout: valid@0[1], _pad@1[7], reason@8[16]  total=24
struct ValidationResult {
    uint8_t    valid;   // 1 = valid, 0 = invalid
    uint8_t    _pad[7];
    StringView reason;
};

// ─── Constants ────────────────────────────────────────────────────────────────

static constexpr uint64_t VALIDATOR_CONTRACT_ID = 0x027ABCEBF8020D90ULL;
static constexpr uint32_t EXT_TRACE_ID          = 0xC4EB9AEEu;

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

/// Allocate a StringView backed by a host-allocated copy of the literal.
static StringView alloc_sv(const char* literal) noexcept {
    size_t len = std::strlen(literal);
    char* buf = static_cast<char*>(
        polyplug_host_alloc(len + 1U, alignof(char))
    );
    std::memcpy(buf, literal, len + 1U);
    StringView sv;
    sv.ptr = reinterpret_cast<const uint8_t*>(buf);
    sv.len = len;
    return sv;
}

/// Returns true when sv contains only printable ASCII characters (0x20..0x7E).
static bool is_printable_ascii(StringView sv) noexcept {
    for (size_t i = 0U; i < sv.len; ++i) {
        unsigned char c = static_cast<unsigned char>(sv.ptr[i]);
        if (c < 0x20u || c > 0x7Eu) {
            return false;
        }
    }
    return true;
}

// ─── Validate function ────────────────────────────────────────────────────────
//
// Validation rules:
//   1. name must be non-empty
//   2. value must be non-empty
//   3. name and value must contain only printable ASCII
//   4. count must be > 0

static AbiError validate_fn(const void* args, void* out) noexcept {
    emit_trace("[validator] validate called");

    if (args == nullptr || out == nullptr) {
        AbiError err;
        err.code        = ABI_ERROR_GENERIC;
        err.message.ptr = nullptr;
        err.message.len = 0U;
        return err;
    }

    const DataRecord* record = static_cast<const DataRecord*>(args);
    ValidationResult* result = static_cast<ValidationResult*>(out);

    // Rule 1: name non-empty
    if (record->name.len == 0U) {
        emit_trace("[validator] INVALID: name is empty");
        result->valid  = 0u;
        result->reason = alloc_sv("name must not be empty");
        AbiError ok;
        ok.code        = ABI_OK;
        ok.message.ptr = nullptr;
        ok.message.len = 0U;
        return ok;
    }

    // Rule 2: value non-empty
    if (record->value.len == 0U) {
        emit_trace("[validator] INVALID: value is empty");
        result->valid  = 0u;
        result->reason = alloc_sv("value must not be empty");
        AbiError ok;
        ok.code        = ABI_OK;
        ok.message.ptr = nullptr;
        ok.message.len = 0U;
        return ok;
    }

    // Rule 3a: name printable ASCII
    if (!is_printable_ascii(record->name)) {
        emit_trace("[validator] INVALID: name contains non-printable characters");
        result->valid  = 0u;
        result->reason = alloc_sv("name must contain only printable ASCII");
        AbiError ok;
        ok.code        = ABI_OK;
        ok.message.ptr = nullptr;
        ok.message.len = 0U;
        return ok;
    }

    // Rule 3b: value printable ASCII
    if (!is_printable_ascii(record->value)) {
        emit_trace("[validator] INVALID: value contains non-printable characters");
        result->valid  = 0u;
        result->reason = alloc_sv("value must contain only printable ASCII");
        AbiError ok;
        ok.code        = ABI_OK;
        ok.message.ptr = nullptr;
        ok.message.len = 0U;
        return ok;
    }

    // Rule 4: count > 0
    if (record->count == 0U) {
        emit_trace("[validator] INVALID: count is zero");
        result->valid  = 0u;
        result->reason = alloc_sv("count must be greater than zero");
        AbiError ok;
        ok.code        = ABI_OK;
        ok.message.ptr = nullptr;
        ok.message.len = 0U;
        return ok;
    }

    emit_trace("[validator] VALID");
    result->valid  = 1u;
    result->reason = alloc_sv("ok");

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
    0u,  // contract_version: v1.0 → (minor << 16 | patch) = 0
    1u,  // function_count
    reinterpret_cast<void* const*>(
        static_cast<FnPtr const*>(VALIDATOR_FNS)
    )
};

static const PluginDescriptor VALIDATOR_DESCRIPTOR = {
    StringView{ reinterpret_cast<const uint8_t*>("validator-cpp"), 13U },
    StringView{ reinterpret_cast<const uint8_t*>("pipeline.validator"),  18U },
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

    emit_trace("[validator] init");

    return registrar->register_plugin(registrar, &VALIDATOR_DESCRIPTOR, &VALIDATOR_VTABLE);
}
