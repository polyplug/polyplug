// THIS FILE IS PART OF polyplug — header-only C++ binding.
// RAII Runtime wrapper and fluent Builder for the polyplug plugin runtime.

#pragma once

#include "abi.hpp"
#include "error.hpp"
#include "handle.hpp"

#include <cstdint>
#include <stdexcept>
#include <string>
#include <string_view>
#include <vector>

static_assert(POLYPLUG_ABI_VERSION == 1,
    "polyplug header version mismatch — recompile against updated headers");

namespace polyplug {

/// RAII guard for a resolved plugin handle.
/// Caches the vtable pointer at construction for zero-overhead access.
/// Move-only; copy is disabled.
class PluginGuard {
public:
    /// Constructs a null guard.
    PluginGuard() noexcept : guard_(nullptr), vtable_(nullptr) {}

    /// Resolves a packed handle and caches the vtable.
    /// If packed_handle is UINT64_MAX or resolution fails, creates a null guard.
    PluginGuard(RuntimeHandle rt, uint64_t packed_handle) noexcept
        : guard_(nullptr), vtable_(nullptr) {
        if (packed_handle != UINT64_MAX && rt != nullptr) {
            guard_ = polyplug_runtime_resolve_plugin(rt, packed_handle);
            if (guard_ != nullptr) {
                vtable_ = static_cast<const PluginVTable*>(
                    polyplug_runtime_plugin_vtable(guard_));
            }
        }
    }

    /// Releases the guard via RAII.
    ~PluginGuard() noexcept {
        if (guard_ != nullptr) {
            polyplug_runtime_plugin_release(guard_);
            guard_ = nullptr;
            vtable_ = nullptr;
        }
    }

    /// Move constructor.
    PluginGuard(PluginGuard&& other) noexcept
        : guard_(other.guard_), vtable_(other.vtable_) {
        other.guard_ = nullptr;
        other.vtable_ = nullptr;
    }

    /// Move assignment.
    PluginGuard& operator=(PluginGuard&& other) noexcept {
        if (this != &other) {
            if (guard_ != nullptr) {
                polyplug_runtime_plugin_release(guard_);
            }
            guard_ = other.guard_;
            vtable_ = other.vtable_;
            other.guard_ = nullptr;
            other.vtable_ = nullptr;
        }
        return *this;
    }

    /// Copy is disabled.
    PluginGuard(const PluginGuard&) = delete;
    PluginGuard& operator=(const PluginGuard&) = delete;

    /// Returns the cached vtable pointer (no FFI call).
    /// Returns nullptr if this is a null guard.
    const PluginVTable* vtable() const noexcept {
        return vtable_;
    }

    /// Returns true if this guard is null (resolution failed or moved-from).
    bool is_null() const noexcept {
        return guard_ == nullptr;
    }

    /// Returns true if this guard holds a valid plugin.
    explicit operator bool() const noexcept {
        return guard_ != nullptr;
    }

private:
    OpaqueGuard* guard_;           ///< Opaque guard handle from resolve_plugin
    const PluginVTable* vtable_;   ///< Cached vtable pointer (no FFI on access)
};

class Runtime {
public:
    class Builder {
    public:
        Builder& plugin_dir(std::string_view path) {
            plugin_dirs_.emplace_back(path);
            return *this;
        }

        Builder& compatibility(uint32_t mode) noexcept {
            compatibility_ = mode;
            return *this;
        }

        Runtime build() {
            RuntimeHandle h = polyplug_runtime_create();
            if (h == nullptr) {
                throw std::runtime_error("polyplug_runtime_create returned null");
            }
            return Runtime(h);
        }

    private:
        std::vector<std::string> plugin_dirs_{};
        uint32_t compatibility_{0U};
    };

    static Builder builder() noexcept {
        return Builder{};
    }

    ~Runtime() noexcept {
        if (handle_ != nullptr) {
            polyplug_runtime_destroy(handle_);
            handle_ = nullptr;
        }
    }

    Runtime(Runtime&& other) noexcept : handle_(other.handle_) {
        other.handle_ = nullptr;
    }

    Runtime& operator=(Runtime&& other) noexcept {
        if (this != &other) {
            if (handle_ != nullptr) {
                polyplug_runtime_destroy(handle_);
            }
            handle_ = other.handle_;
            other.handle_ = nullptr;
        }
        return *this;
    }

    Runtime(const Runtime&) = delete;
    Runtime& operator=(const Runtime&) = delete;

    uint64_t find(uint64_t contract_id, uint32_t min_version) const noexcept {
        return polyplug_runtime_find_by_contract(handle_, contract_id, min_version);
    }

    /// Resolves a packed handle to a PluginGuard with cached vtable.
    /// Returns a null guard if packed_handle is UINT64_MAX or resolution fails.
    PluginGuard resolve_plugin(uint64_t packed_handle) const noexcept {
        return PluginGuard(handle_, packed_handle);
    }

    RuntimeHandle handle() const noexcept {
        return handle_;
    }

    void load_bundle(std::string_view path) {
        auto bytes = reinterpret_cast<const uint8_t*>(path.data());
        uint32_t result = polyplug_runtime_load_bundle(handle_, bytes, path.size());
        if (result != 0) {
            throw std::runtime_error("Failed to load bundle: " + std::string(path));
        }
    }

private:
    explicit Runtime(RuntimeHandle h) noexcept : handle_(h) {}
    RuntimeHandle handle_;
};

} // namespace polyplug
