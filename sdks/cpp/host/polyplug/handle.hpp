// THIS FILE IS PART OF polyplug — header-only C++ binding.
// GuestContractHandle utility functions and operator overloads.
//
// GuestContractHandle is a generational index handle (index + generation, 8 bytes).
// These helpers make it ergonomic to compare handles and detect the invalid sentinel value.

#pragma once

#include "../../abi/polyplug/abi.hpp"

#include <cstdint>

static_assert(POLYPLUG_ABI_VERSION == 2,
    "polyplug header version mismatch — recompile against updated headers");

namespace polyplug {

/// Returns true if two GuestContractHandles are identical (same slot and generation).
///
/// Both the index and generation fields must match. A handle minted against an
/// older generation of the same slot is NOT equal to a freshly-minted handle,
/// even though both refer to the same registry index.
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
/// Only the index field is checked by is_valid(); the generation value is
/// irrelevant for a null handle.
inline GuestContractHandle invalid_handle() noexcept {
    GuestContractHandle h{};
    h.index = UINT32_MAX;
    h.generation = 0U;
    return h;
}

}  // namespace polyplug