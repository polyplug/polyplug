// THIS FILE IS PART OF polyplug — header-only C++ binding.
// Guest plugin entry point macro and host-allocator operator overloads.
//
// Including this header in exactly one translation unit per plugin bundle:
//   1. Overrides global operator new / operator delete to route all
//      heap allocations through the host allocator function pointers on the
//      HostInterface (host->alloc / host->free), stored per-DSO by
//      store_host_interface() inside polyplug_init.
//   2. Provides the POLYPLUG_GUEST_MAIN macro that expands to the required
//      extern "C" polyplug_init signature.
//
// HOW TO USE
// ----------
//   // In exactly one .cpp file in your plugin:
//   #include <polyplug/guest.hpp>
//
//   POLYPLUG_GUEST_MAIN {
//       // host->register_contract(host, &kDescriptor, &kInterface);
//       AbiError err{};
//       err.code        = static_cast<uint32_t>(AbiErrorCode::Ok);
//       err.message.ptr = nullptr;
//       err.message.len = 0;
//       return err;
//   }
//
// WARNING: operator new / operator delete are replaced globally for the
// entire DSO. This is intentional — all heap memory in a plugin bundle must
// be owned by the host allocator so the host can free it safely.
//
// The host allocator is only reachable once store_host_interface() has run
// (first statement of polyplug_init). Any allocation attempted before that —
// e.g. during C++ static initialization — has no host allocator to call and
// throws std::bad_alloc, matching the operator-new contract.

#pragma once

#include "../../abi/polyplug/abi.hpp"

#include <cstddef>
#include <cstdint>
#include <cstring>
#include <new>
#include <string>

static_assert(POLYPLUG_ABI_VERSION == 1,
    "polyplug header version mismatch — recompile against updated headers");

// ─── Host-allocator operator new / operator delete ───────────────────────────
//
// These are global replacement allocation functions ([replacement.functions]).
// The standard forbids replacement operator new / operator delete from being
// declared 'inline' (ill-formed, no diagnostic required), so they are plain
// non-inline definitions. This header is therefore — like the guest entry
// point it accompanies — meant to be included in EXACTLY ONE translation unit
// per plugin DSO (see polyplug_guest.hpp). Including it in more than one TU of
// the same DSO is an ODR violation, exactly as it would be for any hand-written
// global operator new / operator delete replacement.
//
// SIZE TRACKING
// -------------
// `polyplug_host_free(ptr, size, align)` requires the EXACT size and align used
// at allocation (it reconstructs the Rust Layout and no-ops when size==0). The
// unsized `operator delete(void*)` has no size, so we cannot forward a correct
// size to the host. Forwarding 0 makes the host no-op → a permanent leak.
//
// To give every deallocation path a correct size, every allocation over-
// allocates a fixed-size header that records the original (size, align). The
// user pointer is returned just past the header; all delete forms recover the
// base pointer and the true (size, align) from that header. These pointers are
// strictly guest-internal C++ object storage — nothing allocated through these
// operators is ever handed to the host to free (cross-boundary data uses
// `polyplug::alloc_string` / Buffer with an explicit size), so prefixing a
// header is sound.

// ─── HostInterface Storage ───────────────────────────────────────────────────────
//
// The host allocator lives behind function pointers on the HostInterface, which
// polyplug_init receives and stores here for the rest of the DSO's lifetime.

namespace polyplug {
namespace detail {

inline const HostInterface*& host_interface_storage() noexcept {
    static const HostInterface* stored = nullptr;
    return stored;
}

}

inline void store_host_interface(const HostInterface* iface) noexcept {
    detail::host_interface_storage() = iface;
}

inline const HostInterface* get_host_interface() noexcept {
    return detail::host_interface_storage();
}

/// Allocate a StringView from a std::string using the host allocator.
///
/// The bytes are owned by the host: cross-boundary string data is allocated
/// through host->alloc (align 1) so the host can later free it with the exact
/// (size, align). This is distinct from the operator-new path, which prefixes
/// an AllocHeader for guest-internal C++ storage only.
///
/// Returns an empty StringView when the host interface is unavailable (called
/// before polyplug_init stored it) or on allocation failure.
inline StringView alloc_string(const std::string& s) {
    const HostInterface* host = get_host_interface();
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

}

namespace polyplug::detail {

/// Per-allocation header recording the full block size and alignment so the
/// unsized delete can reconstruct the exact (size, align) the host requires.
struct AllocHeader {
    std::size_t total_size;  // header + payload, as passed to host->alloc
    std::size_t align;       // alignment passed to host->alloc
};

/// Bytes reserved before the user pointer. Must be a multiple of the alignment
/// so the user pointer keeps the requested alignment; for over-aligned requests
/// we round the header up to the requested alignment.
constexpr std::size_t header_slot(std::size_t align) noexcept {
    constexpr std::size_t base =
        sizeof(AllocHeader) < alignof(std::max_align_t) ? alignof(std::max_align_t)
                                                        : sizeof(AllocHeader);
    // Round `base` up to a multiple of `align`.
    return ((base + align - 1U) / align) * align;
}

/// Allocate `payload` bytes with `align`, prefixed by an AllocHeader.
/// Returns the user pointer (just past the header), or nullptr on failure or
/// when the host interface has not been stored yet (e.g. before polyplug_init).
inline void* tracked_alloc(std::size_t payload, std::size_t align) noexcept {
    const HostInterface* host = polyplug::get_host_interface();
    if (host == nullptr) {
        return nullptr;
    }
    const std::size_t slot = header_slot(align);
    const std::size_t total = slot + payload;
    auto* base = static_cast<std::uint8_t*>(host->alloc(host, total, align));
    if (base == nullptr) {
        return nullptr;
    }
    auto* header = reinterpret_cast<AllocHeader*>(base + slot - sizeof(AllocHeader));
    header->total_size = total;
    header->align = align;
    return base + slot;
}

/// Free a user pointer previously returned by tracked_alloc, recovering the
/// base pointer and exact (size, align) from the header. A no-op when the host
/// interface is unavailable (the pointer could not have come from tracked_alloc).
inline void tracked_free(void* user) noexcept {
    if (user == nullptr) {
        return;
    }
    const HostInterface* host = polyplug::get_host_interface();
    if (host == nullptr) {
        return;
    }
    auto* user_bytes = static_cast<std::uint8_t*>(user);
    auto* header = reinterpret_cast<AllocHeader*>(user_bytes - sizeof(AllocHeader));
    const std::size_t total_size = header->total_size;
    const std::size_t align = header->align;
    const std::size_t slot = header_slot(align);
    auto* base = user_bytes - slot;
    host->free(host, base, total_size, align);
}

}  // namespace polyplug::detail

/// Allocate sz bytes through the host allocator.
/// Alignment is std::max_align_t (the maximum fundamental alignment).
void* operator new(std::size_t sz) {
    void* p = polyplug::detail::tracked_alloc(sz, alignof(std::max_align_t));
    if (p == nullptr) {
        throw std::bad_alloc{};
    }
    return p;
}

/// Allocate sz bytes, alignment-aware, through the host allocator.
void* operator new(std::size_t sz, std::align_val_t al) {
    void* p = polyplug::detail::tracked_alloc(sz, static_cast<std::size_t>(al));
    if (p == nullptr) {
        throw std::bad_alloc{};
    }
    return p;
}

/// Free memory through the host allocator (unsized form).
/// Required by C++ ABI alongside the sized form.
void operator delete(void* ptr) noexcept {
    polyplug::detail::tracked_free(ptr);
}

/// Free memory through the host allocator (sized form).
/// The C++14 size hint is ignored: the header carries the authoritative size.
void operator delete(void* ptr, std::size_t /*sz*/) noexcept {
    polyplug::detail::tracked_free(ptr);
}

/// Free memory through the host allocator (alignment-aware form).
void operator delete(void* ptr, std::size_t /*sz*/, std::align_val_t /*al*/) noexcept {
    polyplug::detail::tracked_free(ptr);
}

/// Array form of operator new.
void* operator new[](std::size_t sz) {
    void* p = polyplug::detail::tracked_alloc(sz, alignof(std::max_align_t));
    if (p == nullptr) {
        throw std::bad_alloc{};
    }
    return p;
}

/// Array form of operator new (alignment-aware).
void* operator new[](std::size_t sz, std::align_val_t al) {
    void* p = polyplug::detail::tracked_alloc(sz, static_cast<std::size_t>(al));
    if (p == nullptr) {
        throw std::bad_alloc{};
    }
    return p;
}

/// Array form of operator delete (unsized).
/// Required by C++ ABI alongside the sized array form.
void operator delete[](void* ptr) noexcept {
    polyplug::detail::tracked_free(ptr);
}

/// Array form of operator delete (sized). Size hint ignored (see above).
void operator delete[](void* ptr, std::size_t /*sz*/) noexcept {
    polyplug::detail::tracked_free(ptr);
}

/// Array form of operator delete (sized, alignment-aware). Hints ignored.
void operator delete[](void* ptr, std::size_t /*sz*/, std::align_val_t /*al*/) noexcept {
    polyplug::detail::tracked_free(ptr);
}

// ─── Entry point macro ───────────────────────────────────────────────────────

/// Expand to the required extern "C" polyplug_init function signature.
///
/// Usage:
///   POLYPLUG_GUEST_MAIN {
///       // register contracts via host->register_contract(host, &desc, &iface)
///       AbiError ok{};
///       ok.code        = static_cast<uint32_t>(AbiErrorCode::Ok);
///       ok.message.ptr = nullptr;
///       ok.message.len = 0;
///       return ok;
///   }
///
/// The macro expands to:
///   extern "C" AbiError polyplug_init(const HostInterface* host, const BundleInitContext* ctx)
#define POLYPLUG_GUEST_MAIN \
    extern "C" AbiError polyplug_init(const HostInterface* host, const BundleInitContext* ctx)
