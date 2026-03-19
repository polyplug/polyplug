// THIS FILE IS PART OF polyplug — header-only C++ binding.
// RAII Runtime wrapper and fluent Builder for the polyplug plugin runtime.

#pragma once

#include "abi.hpp"
#include "error.hpp"
#include "handle.hpp"
#include "runtime_config.hpp"

#include <cstdint>
#include <functional>
#include <stdexcept>
#include <string>
#include <string_view>
#include <vector>

static_assert(POLYPLUG_ABI_VERSION == 1,
    "polyplug header version mismatch — recompile against updated headers");

namespace polyplug {

/// RAII guard for a resolved plugin handle.
/// Stores runtime + handle for hot-reload safety.
/// Re-resolves vtable on each call to detect stale handles.
/// Move-only; copy is disabled.
class PluginGuard {
public:
    /// Constructs a null guard.
    PluginGuard() noexcept : rt_(nullptr), handle_(UINT64_MAX) {}

    /// Stores runtime + handle for hot-reload safety.
    /// Does NOT cache vtable - re-resolves on each vtable() call.
    PluginGuard(RuntimeHandle rt, uint64_t packed_handle) noexcept
        : rt_(rt), handle_(packed_handle) {}

    /// No release needed — no owned resources.
    ~PluginGuard() noexcept = default;

    /// Move constructor.
    PluginGuard(PluginGuard&& other) noexcept
        : rt_(other.rt_), handle_(other.handle_) {
        other.rt_ = nullptr;
        other.handle_ = UINT64_MAX;
    }

    /// Move assignment.
    PluginGuard& operator=(PluginGuard&& other) noexcept {
        if (this != &other) {
            rt_ = other.rt_;
            handle_ = other.handle_;
            other.rt_ = nullptr;
            other.handle_ = UINT64_MAX;
        }
        return *this;
    }

    /// Copy is disabled.
    PluginGuard(const PluginGuard&) = delete;
    PluginGuard& operator=(const PluginGuard&) = delete;

    /// Re-resolves vtable on each call (hot-reload safe).
    /// Returns nullptr if this is a null guard or resolution fails.
    const PluginVTable* vtable() const noexcept {
        if (rt_ == nullptr || handle_ == UINT64_MAX) {
            return nullptr;
        }
        return static_cast<const PluginVTable*>(
            polyplug_runtime_resolve_plugin(rt_, handle_));
    }

    /// Returns the stored handle.
    uint64_t handle() const noexcept {
        return handle_;
    }

    /// Returns true if this guard is null (no runtime or null handle).
    bool is_null() const noexcept {
        return rt_ == nullptr || handle_ == UINT64_MAX;
    }

    /// Returns true if this guard holds a valid plugin.
    explicit operator bool() const noexcept {
        return !is_null();
    }

    void reset() noexcept {
        rt_ = nullptr;
        handle_ = UINT64_MAX;
    }

private:
    RuntimeHandle rt_;       ///< Runtime pointer (not owned)
    uint64_t handle_;        ///< Packed plugin handle
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

    /// Resolves a packed handle to a PluginGuard.
    /// Guard stores runtime + handle for hot-reload safety.
    /// Returns a null guard if packed_handle is UINT64_MAX.
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

    void reload_bundle(std::string_view path) {
        auto bytes = reinterpret_cast<const uint8_t*>(path.data());
        uint32_t result = polyplug_runtime_reload_bundle(handle_, bytes, path.size());
        if (result != 0) {
            throw std::runtime_error("Failed to reload bundle: " + std::string(path));
        }
    }

    static void on_reload(std::function<void(const ReloadPhase&)> callback) {
        on_reload_cb_() = std::move(callback);
        polyplug_runtime_on_reload([](ReloadPhase phase) {
            auto& cb = on_reload_cb_();
            if (cb) {
                cb(phase);
            }
        });
    }

    static void set_config(const RuntimeConfig& config) {
        RuntimeConfigC config_c{};
        config_c.hot_reload_max_retries = config.hot_reload_max_retries;
        config_c.hot_reload_retry_interval_ms = static_cast<uint64_t>(
            config.hot_reload_retry_interval.count());
        config_c.hot_reload_abort_on_max_retries = config.hot_reload_abort_on_max_retries ? 1 : 0;
        polyplug_runtime_set_config(&config_c);
    }

private:
    explicit Runtime(RuntimeHandle h) noexcept : handle_(h) {}
    RuntimeHandle handle_;
    static std::function<void(const ReloadPhase&)>& on_reload_cb_() {
        static std::function<void(const ReloadPhase&)> cb;
        return cb;
    }
};

} // namespace polyplug