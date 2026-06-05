// Runtime test for the guest header's host-allocator operator new/delete.
//
// Verifies that every deallocation path (sized, unsized, array, aligned) frees
// the SAME (ptr, size, align) that was allocated — i.e. the header-based size
// tracking lets the unsized `operator delete(void*)` reconstruct the true size
// instead of passing 0 (which the host treats as a no-op -> leak).
//
// Build (from repo root):
//   g++ -std=c++17 -I sdks/cpp/abi -I sdks/cpp/guest
//       sdks/cpp/guest/test_alloc_tracking.cpp -o /tmp/test_alloc_tracking
//   /tmp/test_alloc_tracking
//
// The guest operators route through the HostApi stored by
// store_host_interface(); this test supplies a HostApi whose alloc/free
// function pointers do the bookkeeping below, then stores it before allocating.
//
// NOTE: this TU replaces the GLOBAL operator new/delete (via guest.hpp), so the
// bookkeeping here must use malloc/free directly and a fixed C array — using any
// std container would recurse back into the replaced operators.

#include <cstddef>
#include <cstdint>
#include <cstdio>
#include <cstdlib>

namespace {

constexpr std::size_t kMaxLive = 4096;

struct LiveBlock {
    void* ptr;
    std::size_t size;
    std::size_t align;
    bool used;
};

LiveBlock g_live[kMaxLive];
std::size_t g_live_count = 0;
std::size_t g_zero_size_frees = 0;
int g_failures = 0;

void record_alloc(void* ptr, std::size_t size, std::size_t align) {
    for (std::size_t i = 0; i < kMaxLive; ++i) {
        if (!g_live[i].used) {
            g_live[i] = LiveBlock{ptr, size, align, true};
            ++g_live_count;
            return;
        }
    }
    std::printf("FAIL: live-block table full\n");
    ++g_failures;
}

void record_free(void* ptr, std::size_t size, std::size_t align) {
    for (std::size_t i = 0; i < kMaxLive; ++i) {
        if (g_live[i].used && g_live[i].ptr == ptr) {
            if (g_live[i].size != size) {
                std::printf("FAIL: freed size %zu != allocated size %zu\n", size,
                            g_live[i].size);
                ++g_failures;
            }
            if (g_live[i].align != align) {
                std::printf("FAIL: freed align %zu != allocated align %zu\n", align,
                            g_live[i].align);
                ++g_failures;
            }
            g_live[i].used = false;
            --g_live_count;
            return;
        }
    }
    std::printf("FAIL: free of unknown/wrong base pointer %p\n", ptr);
    ++g_failures;
}

}  // namespace

#include "polyplug/guest.hpp"

namespace {

// Tracking allocator behind the HostApi::alloc function pointer.
std::uint8_t* tracking_alloc(const HostApi* /*self*/, std::size_t size,
                             std::size_t align) noexcept {
    std::size_t rounded = ((size + align - 1U) / align) * align;
    if (rounded == 0) {
        rounded = align;
    }
    void* p = std::aligned_alloc(align, rounded);
    if (p == nullptr) {
        return nullptr;
    }
    record_alloc(p, size, align);
    return static_cast<std::uint8_t*>(p);
}

// Tracking deallocator behind the HostApi::free function pointer.
void tracking_free(const HostApi* /*self*/, std::uint8_t* ptr, std::size_t size,
                   std::size_t align) noexcept {
    if (ptr == nullptr) {
        return;
    }
    if (size == 0) {
        // This is exactly the leak the fix prevents: the real host no-ops on size==0.
        ++g_zero_size_frees;
        return;
    }
    record_free(static_cast<void*>(ptr), size, align);
    std::free(static_cast<void*>(ptr));
}

}  // namespace

struct alignas(64) OverAligned {
    char data[200];
};

int main() {
    // Supply a HostApi whose alloc/free do the bookkeeping, then store it
    // so the guest operators (and any cross-boundary helper) can reach it.
    HostApi host{};
    host.alloc = tracking_alloc;
    host.free = tracking_free;
    polyplug::store_host_interface(&host);

    // 1. Scalar new + unsized delete (the path that used to leak).
    {
        int* p = new int(42);
        if (*p != 42) {
            ++g_failures;
        }
        delete p;
    }
    // 2. Scalar new + explicit sized delete.
    {
        auto* p = new double(3.14);
        ::operator delete(p, sizeof(double));
    }
    // 3. Array new + array delete.
    {
        int* arr = new int[16];
        arr[0] = 1;
        arr[15] = 2;
        delete[] arr;
    }
    // 4. Over-aligned new + delete.
    {
        auto* p = new OverAligned();
        if (reinterpret_cast<std::uintptr_t>(p) % 64 != 0) {
            std::printf("FAIL: over-aligned pointer not 64-aligned\n");
            ++g_failures;
        }
        delete p;
    }
    // 5. A batch to make any per-allocation leak visible.
    {
        for (long i = 0; i < 1000; ++i) {
            delete new long(i);
        }
    }

    if (g_live_count != 0) {
        std::printf("FAIL: %zu blocks leaked\n", g_live_count);
        ++g_failures;
    }
    if (g_zero_size_frees != 0) {
        std::printf("FAIL: %zu frees passed size==0 (would leak in real host)\n",
                    g_zero_size_frees);
        ++g_failures;
    }
    if (g_failures != 0) {
        std::printf("FAILED: %d check(s) failed\n", g_failures);
        return 1;
    }
    std::printf("OK: all allocations freed with correct size/align\n");
    return 0;
}
