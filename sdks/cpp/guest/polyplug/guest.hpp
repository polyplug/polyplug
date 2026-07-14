// THIS FILE IS PART OF polyplug — header-only C++ binding.
// Guest plugin entry point macro and host-allocator string helper.
//
// HOW TO USE
// ----------
//   // In exactly one .cpp file in your plugin:
//   #include <polyplug/guest.hpp>
//
//   POLYPLUG_GUEST_MAIN {
//       // Out-param ABI: register writes its AbiError through the trailing
//       // pointer and returns void; init surfaces it by value.
//       AbiError err{};
//       host->register_guest_contract(host, &kDescriptor, &kInterface, &err);
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

#include "polyplug/abi.hpp"

#include <cstddef>
#include <cstdint>
#include <cstring>
#include <memory>
#include <string>
#include <vector>

static_assert(POLYPLUG_ABI_VERSION == 2,
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

/// `(items, len)` pair for building an `ArrayOf_<T> { items, len }` wrapper —
/// what `ReturnArena::alloc_array` returns.
struct ArrayRef {
    uint64_t items;
    uint64_t len;
};

/// A per-instance return buffer for a native guest's variable-size returns.
///
/// A native guest that returns a `StringView`, `Buffer`, or an `ArrayOf_<T>`
/// wrapper must hand back memory that stays valid until the host copies it out.
/// `ReturnArena` owns one reusable, retain-and-rewind buffer: `reset()` at the
/// start of each call, then `alloc_str` / `alloc_array` fill it, and the returned
/// view borrows it until the next reset. Zero per-call host allocation, zero host
/// free — the borrowed-return model, unlike `alloc_string` (which allocates
/// through the host and the host caller never frees).
///
/// Hold one on the plugin impl and reset it at the top of each method that
/// returns variable-size data. Its blocks are plain C++ heap (guest-internal,
/// never crossing the ABI), so the host never frees them.
class ReturnArena {
public:
    explicit ReturnArena(std::size_t capacity) {
        add_block(capacity == 0 ? kMinBlock : capacity);
    }

    ReturnArena(const ReturnArena&) = delete;
    ReturnArena& operator=(const ReturnArena&) = delete;

    /// Rewind to the first block (retaining all blocks for reuse); invalidates
    /// every view returned since the last reset.
    void reset() {
        cur_block_ = 0;
        cursor_ = 0;
    }

    /// Copy `s` into the buffer; the returned view is valid until `reset()`. An
    /// empty string returns an empty view without allocating.
    StringView alloc_str(const std::string& s) {
        if (s.empty()) {
            return StringView{nullptr, 0};
        }
        void* p = alloc(s.size(), 1);
        if (p == nullptr) {
            return StringView{nullptr, 0};
        }
        std::memcpy(p, s.data(), s.size());
        return StringView{static_cast<uint8_t*>(p), s.size()};
    }

    /// Copy `count` `T` elements into the buffer and return `(items, len)` for an
    /// `ArrayOf_<T>` wrapper. Valid until `reset()`. Any `StringView` embedded in
    /// an element must already point at this arena (from a prior `alloc_str`), so
    /// the whole return shares one lifetime. `count == 0` returns `{0, 0}`.
    template <class T>
    ArrayRef alloc_array(const T* data, std::size_t count) {
        if (count == 0) {
            return ArrayRef{0, 0};
        }
        void* p = alloc(sizeof(T) * count, alignof(T));
        if (p == nullptr) {
            return ArrayRef{0, 0};
        }
        std::memcpy(p, data, sizeof(T) * count);
        return ArrayRef{static_cast<uint64_t>(reinterpret_cast<uintptr_t>(p)), count};
    }

private:
    static constexpr std::size_t kMinBlock = 4096;

    void add_block(std::size_t cap) {
        blocks_.push_back(std::make_unique<std::byte[]>(cap));
        caps_.push_back(cap);
    }

    /// Bump-allocate `size` bytes at `align` from the current block, advancing to
    /// (or allocating) a retained block on exhaustion. Retained blocks keep prior
    /// views valid across `reset()`.
    void* alloc(std::size_t size, std::size_t align) {
        if (size == 0) {
            return nullptr;
        }
        for (;;) {
            std::byte* base = blocks_[cur_block_].get();
            std::size_t cap = caps_[cur_block_];
            std::size_t aligned = (cursor_ + (align - 1)) & ~(align - 1);
            if (aligned <= cap && cap - aligned >= size) {
                cursor_ = aligned + size;
                return base + aligned;
            }
            if (cur_block_ + 1 < blocks_.size()) {
                ++cur_block_;
                cursor_ = 0;
                continue;
            }
            std::size_t grown = caps_.back() * 2;
            std::size_t needed = size + align;
            add_block(grown > needed ? grown : needed);
            cur_block_ = blocks_.size() - 1;
            cursor_ = 0;
        }
    }

    std::vector<std::unique_ptr<std::byte[]>> blocks_;
    std::vector<std::size_t> caps_;
    std::size_t cur_block_ = 0;
    std::size_t cursor_ = 0;
};

}  // namespace polyplug

// ─── Entry point macro ───────────────────────────────────────────────────────

/// Expand to the required extern "C" polyplug_init function signature.
///
/// Usage:
///   POLYPLUG_GUEST_MAIN {
///       // Out-param ABI: register writes the AbiError through the trailing
///       // pointer; init returns it by value.
///       AbiError ok{};
///       host->register_guest_contract(host, &desc, &iface, &ok);
///       return ok;
///   }
///
/// The macro expands to:
///   extern "C" AbiError polyplug_init(const HostApi* host, const BundleInitContext* ctx)
#define POLYPLUG_GUEST_MAIN \
    extern "C" AbiError polyplug_init(const HostApi* host, const BundleInitContext* ctx)
