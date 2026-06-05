// THIS FILE IS PART OF polyplug — header-only C++ binding.
// GuestContractHandle utility functions and operator overloads.
//
// GuestContractHandle is a simple index handle. These helpers
// make it ergonomic to compare handles and detect the invalid sentinel value.

#pragma once

#include "../../abi/polyplug/abi.hpp"

#include <cstdint>

static_assert(POLYPLUG_ABI_VERSION == 1,
    "polyplug header version mismatch — recompile against updated headers");

namespace polyplug {

/// Returns true if two GuestContractHandles refer to the same slot.
///
/// Note: GuestContractHandle currently has only an index field (no generation).
/// This is intentional — the generational index pattern uses generation at the
/// registry level, not in the handle itself. The handle is validated by
/// `resolve_guest_contract`, which checks the slot's current generation.
inline bool operator==(GuestContractHandle a, GuestContractHandle b) noexcept {
    return a.index == b.index;
}

/// Returns true if two GuestContractHandles differ in slot.
inline bool operator!=(GuestContractHandle a, GuestContractHandle b) noexcept {
    return !(a == b);
}

/// Returns true if the handle is a valid (non-null) handle.
///
/// The null/invalid sentinel uses index == UINT32_MAX, matching
/// GuestContractHandle::null() in the Rust runtime.
inline bool is_valid(GuestContractHandle h) noexcept {
    return h.index != UINT32_MAX;
}

/// Returns the canonical invalid/null GuestContractHandle sentinel.
///
/// Mirrors GuestContractHandle::null() in the Rust runtime:
///   index = u32::MAX
inline GuestContractHandle invalid_handle() noexcept {
    GuestContractHandle h{};
    h.index = UINT32_MAX;
    return h;
}

}  // namespace polyplug