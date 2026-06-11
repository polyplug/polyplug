// THIS FILE IS PART OF polyplug — header-only C++ binding.
// Guest plugin entry point macro and host-allocator string helper.
//
// HOW TO USE
// ----------
//   // In exactly one .cpp file in your plugin:
//   #include <polyplug/guest.hpp>
//
//   POLYPLUG_GUEST_MAIN {
//       // host->register_guest_contract(host, &kDescriptor, &kInterface);
//       AbiError err{};
//       err.code        = static_cast<uint32_t>(AbiErrorCode::Ok);
//       err.message.ptr = nullptr;
//       err.message.len = 0;
//       return err;
//   }
//
// HOST ACCESS — INSTANCE FLOW, NO STATICS
// ---------------------------------------
// This header holds NO process-wide state. The HostApi pointer flows from
// `create_instance` (where the host passes it) into the per-instance payload
// the generated glue carries in `GuestContractInstance.data`, and into the
// author factory (`polyplug_create_<plugin>(const HostApi*)`). Every host
// call (allocation, logging, peer dispatch) is therefore routed to the exact
// Runtime that owns the in-flight call.
//
// MEMORY MODEL
// ------------
// Guest-internal C++ heap allocations (operator new/delete) use the plugin's
// own allocator — they never cross the ABI boundary, so the host never frees
// them. Cross-boundary data MUST be allocated through the host allocator
// explicitly: use `polyplug::alloc_string(host, s)` for strings, or
// `host->alloc(host, size, align)` directly for raw buffers.

#pragma once

#include "../../abi/polyplug/abi.hpp"

#include <cstddef>
#include <cstdint>
#include <cstring>
#include <string>

static_assert(POLYPLUG_ABI_VERSION == 1,
    "polyplug header version mismatch — recompile against updated headers");

namespace polyplug {

/// Allocate a StringView from a std::string using the given host's allocator.
///
/// The bytes are owned by the host: cross-boundary string data is allocated
/// through host->alloc (align 1) so the host can later free it with the exact
/// (size, align).
///
/// `host` is the per-instance HostApi pointer the generated glue carries in
/// the instance payload (or the factory parameter) — there is no process-wide
/// host storage.
///
/// Returns an empty StringView when `host` is null or on allocation failure.
inline StringView alloc_string(const HostApi* host, const std::string& s) {
    if (host == nullptr) {
        return StringView{nullptr, 0};
    }
    auto* ptr = static_cast<uint8_t*>(host->alloc(host, s.size(), 1));
    if (ptr == nullptr) {
        return StringView{nullptr, 0};
    }
    std::memcpy(ptr, s.data(), s.size());
    return StringView{ptr, s.size()};
}

}  // namespace polyplug

// ─── Entry point macro ───────────────────────────────────────────────────────

/// Expand to the required extern "C" polyplug_init function signature.
///
/// Usage:
///   POLYPLUG_GUEST_MAIN {
///       // register contracts via host->register_guest_contract(host, &desc, &iface)
///       AbiError ok{};
///       ok.code        = static_cast<uint32_t>(AbiErrorCode::Ok);
///       ok.message.ptr = nullptr;
///       ok.message.len = 0;
///       return ok;
///   }
///
/// The macro expands to:
///   extern "C" AbiError polyplug_init(const HostApi* host, const BundleInitContext* ctx)
#define POLYPLUG_GUEST_MAIN \
    extern "C" AbiError polyplug_init(const HostApi* host, const BundleInitContext* ctx)
