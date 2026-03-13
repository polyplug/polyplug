// THIS FILE IS PART OF polyplug — header-only C++ binding.
// Guest-side ABI types for the polyplug plugin runtime (C++ bindings).
//
// This file is structurally identical to host-libs/cpp/polyplug/abi.hpp.
// It is kept as a separate copy so guest plugins have no compile-time
// dependency on the host-libs tree. Both files MUST remain in sync with
// the Rust source of truth: crates/polyplug-runtime/src/abi/mod.rs
//
// Include this header for: StringView, Buffer, AbiError, PluginHandle,
// PluginVTable, HostVTable, PluginDescriptor, PluginRegistrar, RuntimeConfig.

#pragma once

#include <cstddef>
#include <cstdint>

// ABI version sentinel — bump only on incompatible ABI changes.
#define POLYPLUG_ABI_VERSION 1

// ABI error codes (0-255 reserved for runtime, 256+ plugin-defined)
#define ABI_OK                 0U
#define ABI_ERROR_GENERIC      1U
#define ABI_BUFFER_TOO_SMALL   2U  // caller must reallocate (see Buffer protocol)
#define ABI_ERROR_PANIC        3U  // plugin panicked (caught by exception handler)
#define ABI_ERROR_NOT_FOUND    4U  // plugin/contract not found
#define ABI_ERROR_STALE_HANDLE 5U  // PluginHandle generation mismatch
#define ABI_FUNCTION_NOT_AVAIL 6U  // function_id >= function_count

extern "C" {

/// Non-owning UTF-8 string view.
/// OWNERSHIP: borrowed reference. ptr must remain valid for the call duration.
struct StringView {
    const uint8_t* ptr;  ///< UTF-8 bytes, NOT null-terminated
    size_t         len;  ///< byte count
};

/// Owning byte buffer.
/// OWNERSHIP: ptr is always allocated via polyplug_host_alloc.
struct Buffer {
    void*  ptr;
    size_t len;  ///< bytes currently used
    size_t cap;  ///< bytes allocated
};

/// ABI error — returned by value from all ABI calls.
/// OWNERSHIP: if code != ABI_OK, message.ptr is allocated via host_alloc.
/// Caller must free with polyplug_host_free(message.ptr, message.len, 1).
struct AbiError {
    uint32_t   code;     ///< 0 = success
    StringView message;  ///< empty/NULL if success
};

/// Opaque handle to a loaded plugin — generational index.
/// index = slot in registry array, generation = stale-handle detection.
struct PluginHandle {
    uint32_t index;
    uint32_t generation;
};

/// Plugin VTable — one per contract implemented.
/// OWNERSHIP: Must be 'static (never freed while runtime lives).
struct PluginVTable {
    uint64_t      contract_id;      ///< FNV-1a hash of "name@major"
    uint32_t      contract_version; ///< (minor << 16 | patch)
    uint32_t      function_count;   ///< entries in functions array
    void* const*  functions;        ///< static array of fn ptrs, indexed by function_id
};

/// Host capabilities passed to every plugin at init time.
/// OWNERSHIP: 'static, lives as long as the runtime.
struct HostVTable {
    void*               (*alloc)(size_t size, size_t align);
    void                (*free)(void* ptr, size_t size, size_t align);
    PluginHandle        (*find_by_contract)(uint64_t contract_id, uint32_t min_version);
    PluginHandle        (*find_by_bundle)(uint64_t bundle_id, uint64_t contract_id, uint32_t min_version);
    size_t              (*find_all_by_contract)(uint64_t contract_id, uint32_t min_version, PluginHandle* out, size_t out_cap);
    const PluginVTable* (*resolve_plugin)(PluginHandle handle);
    const void*         (*get_extension)(uint32_t extension_id);
};

/// Metadata about a plugin within a bundle.
struct PluginDescriptor {
    StringView name;           ///< human-readable plugin name
    StringView contract_name;  ///< full contract name for collision detection
    uint32_t   version_major;
    uint32_t   version_minor;
    uint32_t   version_patch;
};

/// Bridge used during polyplug_init only — not stored long-term.
struct PluginRegistrar {
    AbiError (*register_plugin)(
        PluginRegistrar*        self,
        const PluginDescriptor* descriptor,
        const PluginVTable*     vtable
    );
    const HostVTable* host;
};

/// A single extension entry in the runtime config.
struct ExtensionEntry {
    uint32_t    extension_id;  ///< FNV-1a lower 32 bits of extension name
    const void* vtable;        ///< pointer to extension vtable struct
};

/// Configuration passed to polyplug_runtime_init.
struct RuntimeConfig {
    const StringView*    plugin_dirs;      ///< array of plugin_dir_count directories
    size_t               plugin_dir_count;
    uint32_t             compatibility;    ///< 0 = Strict (MVP only)
    const ExtensionEntry* extensions;      ///< array of extension_count entries
    size_t               extension_count;
};

/// Context passed to every guest polyplug_init() function.
/// bundle_path.ptr is runtime-owned and valid for the PluginRuntime lifetime.
/// Do NOT store the raw pointer — copy the string if persistence is needed.
struct PluginContext {
    StringView bundle_path;  ///< Absolute canonical path to bundle directory
};

// ─── Allocator (available to guest code) ─────────────────────────────────────

/// Allocate memory via the host allocator.
void* polyplug_host_alloc(size_t size, size_t align);

/// Free memory previously allocated by polyplug_host_alloc.
void polyplug_host_free(void* ptr, size_t size, size_t align);

/// ABI version sentinel. Guests MUST export this function.
uint32_t polyplug_abi_version();

}  // extern "C"

#ifdef __cplusplus
#include <string>
#include <string_view>
#include <span>

inline std::string_view StringView_as_string_view(const StringView& sv) noexcept {
    return {reinterpret_cast<const char*>(sv.ptr), sv.len};
}
inline std::string StringView_to_string(const StringView& sv) {
    return std::string(reinterpret_cast<const char*>(sv.ptr), sv.len);
}
inline std::span<const uint8_t> Buffer_as_span(const Buffer& b) noexcept {
    return {static_cast<const uint8_t*>(b.ptr), b.len};
}
inline std::span<uint8_t> Buffer_as_mut_span(Buffer& b) noexcept {
    return {static_cast<uint8_t*>(b.ptr), b.cap};
}
#endif // __cplusplus

// ─── Compile-time contract ID computation ────────────────────────────────────
//
// Computes FNV-1a 64-bit hash of the canonical string "name@major_version".
// Identical algorithm to the Rust implementation in polyplug::abi::contract_id.
//
// Usage:
//   constexpr uint64_t MY_CONTRACT = polyplug::fnv1a_contract_id("my.contract", 1);
namespace polyplug {

namespace detail {

/// FNV-1a 64-bit hash of a NUL-terminated string prefix.
/// Internal helper — prefer fnv1a_contract_id for contract IDs.
constexpr uint64_t fnv1a_64_str(const char* s, uint64_t hash) noexcept {
    return (*s == '\0') ? hash : fnv1a_64_str(s + 1, (hash ^ static_cast<uint64_t>(static_cast<unsigned char>(*s))) * UINT64_C(0x00000100000001B3));
}

/// Append a decimal uint32 to the hash without heap allocation.
constexpr uint64_t fnv1a_64_u32(uint32_t v, uint64_t hash) noexcept {
    if (v < 10U) {
        uint64_t h2 = hash ^ static_cast<uint64_t>('0' + v);
        return h2 * UINT64_C(0x00000100000001B3);
    }
    uint64_t h2 = fnv1a_64_u32(v / 10U, hash);
    uint64_t h3 = h2 ^ static_cast<uint64_t>('0' + (v % 10U));
    return h3 * UINT64_C(0x00000100000001B3);
}

}  // namespace detail

/// Compute the polyplug contract ID for "name@major_version" using FNV-1a 64-bit.
/// Produces the same value as `polyplug::abi::contract_id(name, major_version)` in Rust.
///
/// Known values:
///   fnv1a_contract_id("test.add",    1) == 0xCC4232FAB0410D2BU
///   fnv1a_contract_id("image.decode", 1) == 0xA1BA05DD7DA18569U
constexpr uint64_t fnv1a_contract_id(const char* name, uint32_t major_version) noexcept {
    constexpr uint64_t FNV_OFFSET = UINT64_C(0xcbf29ce484222325);
    uint64_t h = detail::fnv1a_64_str(name, FNV_OFFSET);
    // hash '@'
    h = (h ^ static_cast<uint64_t>('@')) * UINT64_C(0x00000100000001B3);
    // hash decimal digits of major_version
    h = detail::fnv1a_64_u32(major_version, h);
    return h;
}

}  // namespace polyplug
