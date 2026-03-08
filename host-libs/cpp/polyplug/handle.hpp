// THIS FILE IS PART OF polyplug — header-only C++ binding.
// PluginHandle utility functions and operator overloads.
//
// PluginHandle is a generational index (index + generation). These helpers
// make it ergonomic to compare handles and detect the invalid sentinel value.

#pragma once

#include "abi.hpp"

#include <cstdint>

static_assert(POLYPLUG_ABI_VERSION == 1,
    "polyplug header version mismatch — recompile against updated headers");

namespace polyplug {

/// Returns true if two PluginHandles refer to the same slot and generation.
inline bool operator==(PluginHandle a, PluginHandle b) noexcept {
    return a.index == b.index && a.generation == b.generation;
}

/// Returns true if two PluginHandles differ in slot or generation.
inline bool operator!=(PluginHandle a, PluginHandle b) noexcept {
    return !(a == b);
}

/// Returns true if the handle is a valid (non-null) handle.
///
/// The null/invalid sentinel uses index == UINT32_MAX, matching
/// PluginHandle::null() in the Rust runtime.
inline bool is_valid(PluginHandle h) noexcept {
    return h.index != UINT32_MAX;
}

/// Returns the canonical invalid/null PluginHandle sentinel.
///
/// Mirrors PluginHandle::null() in the Rust runtime:
///   index = u32::MAX, generation = 0
inline PluginHandle invalid_handle() noexcept {
    PluginHandle h;
    h.index      = UINT32_MAX;
    h.generation = 0U;
    return h;
}

}  // namespace polyplug
