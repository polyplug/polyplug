// THIS FILE IS PART OF polyplug — header-only C++ binding.
// Guest plugin entry point macro and host-allocator operator overloads.
//
// Including this header in exactly one translation unit per plugin bundle:
//   1. Overrides global operator new / operator delete to route all
//      heap allocations through polyplug_host_alloc / polyplug_host_free.
//   2. Provides the POLYPLUG_GUEST_MAIN macro that expands to the required
//      extern "C" polyplug_init signature.
//
// HOW TO USE
// ----------
//   // In exactly one .cpp file in your plugin:
//   #include <polyplug/guest.hpp>
//
//   POLYPLUG_GUEST_MAIN {
//       // registrar->register_plugin(registrar, &kDescriptor, &kVTable);
//       AbiError err{};
//       err.code        = ABI_OK;
//       err.message.ptr = nullptr;
//       err.message.len = 0;
//       return err;
//   }
//
// WARNING: operator new / operator delete are replaced globally for the
// entire DSO. This is intentional — all heap memory in a plugin bundle must
// be owned by the host allocator so the host can free it safely.

#pragma once

#include "../../abi/polyplug/abi.hpp"

#include <cstddef>
#include <new>

static_assert(POLYPLUG_ABI_VERSION == 1,
    "polyplug header version mismatch — recompile against updated headers");

// ─── Host-allocator operator new / operator delete ───────────────────────────
//
// These replacements are defined inline and marked with the "replaceable"
// attribute pattern. Because this header is meant to be included in exactly
// one TU per DSO, there is no ODR violation.

/// Allocate sz bytes through the host allocator.
/// Alignment is std::max_align_t (the maximum fundamental alignment).
inline void* operator new(std::size_t sz) {
    void* p = polyplug_host_alloc(sz, alignof(std::max_align_t));
    if (p == nullptr) {
        throw std::bad_alloc{};
    }
    return p;
}

/// Allocate sz bytes, alignment-aware, through the host allocator.
inline void* operator new(std::size_t sz, std::align_val_t al) {
    void* p = polyplug_host_alloc(sz, static_cast<std::size_t>(al));
    if (p == nullptr) {
        throw std::bad_alloc{};
    }
    return p;
}

/// Free memory through the host allocator (unsized form).
/// Required by C++ ABI alongside the sized form.
inline void operator delete(void* ptr) noexcept {
    // Size is unknown here; pass 0. The host allocator must tolerate size=0
    // as a sentinel meaning "unknown size" (implementation-defined behaviour).
    // In practice this path is only hit by code that does not use sized delete.
    if (ptr != nullptr) {
        polyplug_host_free(ptr, 0U, alignof(std::max_align_t));
    }
}

/// Free memory through the host allocator (sized form).
/// sz is required by the C++14 sized-deallocation signature.
inline void operator delete(void* ptr, std::size_t sz) noexcept {
    if (ptr != nullptr) {
        polyplug_host_free(ptr, sz, alignof(std::max_align_t));
    }
}

/// Free memory through the host allocator (alignment-aware form).
inline void operator delete(void* ptr, std::size_t sz, std::align_val_t al) noexcept {
    if (ptr != nullptr) {
        polyplug_host_free(ptr, sz, static_cast<std::size_t>(al));
    }
}

/// Array form of operator new.
inline void* operator new[](std::size_t sz) {
    void* p = polyplug_host_alloc(sz, alignof(std::max_align_t));
    if (p == nullptr) {
        throw std::bad_alloc{};
    }
    return p;
}

/// Array form of operator new (alignment-aware).
inline void* operator new[](std::size_t sz, std::align_val_t al) {
    void* p = polyplug_host_alloc(sz, static_cast<std::size_t>(al));
    if (p == nullptr) {
        throw std::bad_alloc{};
    }
    return p;
}

/// Array form of operator delete (unsized).
/// Required by C++ ABI alongside the sized array form.
inline void operator delete[](void* ptr) noexcept {
    if (ptr != nullptr) {
        polyplug_host_free(ptr, 0U, alignof(std::max_align_t));
    }
}

/// Array form of operator delete (sized).
inline void operator delete[](void* ptr, std::size_t sz) noexcept {
    if (ptr != nullptr) {
        polyplug_host_free(ptr, sz, alignof(std::max_align_t));
    }
}

/// Array form of operator delete (sized, alignment-aware).
inline void operator delete[](void* ptr, std::size_t sz, std::align_val_t al) noexcept {
    if (ptr != nullptr) {
        polyplug_host_free(ptr, sz, static_cast<std::size_t>(al));
    }
}

// ─── Host VTable Storage ───────────────────────────────────────────────────────

namespace polyplug {
namespace detail {

inline const HostVTable*& host_vtable_storage() noexcept {
    static const HostVTable* stored = nullptr;
    return stored;
}

}

inline void store_host_vtable(const HostVTable* vtable) noexcept {
    detail::host_vtable_storage() = vtable;
}

inline const HostVTable* get_host_vtable() noexcept {
    return detail::host_vtable_storage();
}

}

// ─── Entry point macro ───────────────────────────────────────────────────────

/// Expand to the required extern "C" polyplug_init function signature.
///
/// Usage:
///   POLYPLUG_GUEST_MAIN {
///       // register contracts via registrar->register_plugin(...)
///       AbiError ok{};
///       ok.code        = ABI_OK;
///       ok.message.ptr = nullptr;
///       ok.message.len = 0;
///       return ok;
///   }
///
/// The macro expands to:
///   extern "C" AbiError polyplug_init(PluginRegistrar* registrar)
#define POLYPLUG_GUEST_MAIN \
    extern "C" AbiError polyplug_init(PluginRegistrar* registrar)