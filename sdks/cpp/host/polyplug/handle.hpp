// THIS FILE IS PART OF polyplug — header-only C++ binding.
// GuestContractHandle utility functions and operator overloads.
//
// GuestContractHandle is a generational index (index + generation). These helpers
// make it ergonomic to compare handles and detect the invalid sentinel value.

#pragma once

#include "../../abi/polyplug/abi.hpp"

#include <cstdint>

static_assert(POLYPLUG_ABI_VERSION == 1,
    "polyplug header version mismatch — recompile against updated headers");

namespace polyplug {

/// Returns true if two GuestContractHandles refer to the same slot and generation.
inline bool operator==(GuestContractHandle a, GuestContractHandle b) noexcept {
    return a.index == b.index && a.generation == b.generation;
}

/// Returns true if two GuestContractHandles differ in slot or generation.
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
///   index = u32::MAX, generation = 0
inline GuestContractHandle invalid_handle() noexcept {
    GuestContractHandle h;
    h.index      = UINT32_MAX;
    h.generation = 0U;
    return h;
}

}  // namespace polyplug