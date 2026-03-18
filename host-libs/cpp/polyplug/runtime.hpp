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
    PluginGuard() noexcept : vtable_(nullptr) {}

    /// Resolves a packed handle and caches the vtable.
    /// If packed_handle is UINT64_MAX or resolution fails, creates a null guard.
    PluginGuard(RuntimeHandle rt, uint64_t packed_handle) noexcept
        : vtable_(nullptr) {
        if (packed_handle != UINT64_MAX && rt != nullptr) {
            vtable_ = static_cast<const PluginVTable*>(
                polyplug_runtime_resolve_plugin(rt, packed_handle));
        }
    }

    /// No release needed — vtable pointer is borrowed from runtime.
    ~PluginGuard() noexcept = default;

    /// Move constructor.
    PluginGuard(PluginGuard&& other) noexcept
        : vtable_(other.vtable_) {
        other.vtable_ = nullptr;
    }

    /// Move assignment.
    PluginGuard& operator=(PluginGuard&& other) noexcept {
        if (this != &other) {
            vtable_ = other.vtable_;
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
        return vtable_ == nullptr;
    }

    /// Returns true if this guard holds a valid plugin.
    explicit operator bool() const noexcept {
        return vtable_ != nullptr;
    }

private:
    const PluginVTable* vtable_;   ///< Cached vtable pointer (borrowed from runtime)
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

    uint64_t find_by_bundle(uint64_t bundle_id, uint64_t contract_id, uint32_t min_version) const noexcept {
        return polyplug_runtime_find_by_bundle(handle_, bundle_id, contract_id, min_version);
    }

    std::vector<uint64_t> find_all_by_contract(uint64_t contract_id, uint32_t min_version, size_t cap = 64) const {
        std::vector<uint64_t> handles(cap);
        size_t count = polyplug_runtime_find_all_by_contract(
            handle_, contract_id, min_version, handles.data(), cap);
        handles.resize(count);
        return handles;
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
