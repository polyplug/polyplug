// THIS FILE IS PART OF polyplug — header-only C++ binding.
// RAII Runtime wrapper and fluent Builder for the polyplug plugin runtime.

#pragma once

#include "../../abi/polyplug/abi.hpp"
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

struct OpaqueRuntime;
using RuntimeHandle = OpaqueRuntime*;

struct ResolveHandle;

} // namespace polyplug

extern "C" {
    RuntimeHandle polyplug_runtime_create();
    void polyplug_runtime_destroy(RuntimeHandle rt);
    uint32_t polyplug_runtime_load_bundle(RuntimeHandle rt, const uint8_t* path, size_t path_len);
    uint32_t polyplug_runtime_reload_bundle(RuntimeHandle rt, const uint8_t* path, size_t path_len);
    uint64_t polyplug_runtime_find_by_contract(RuntimeHandle rt, uint64_t contract_id, uint32_t min_version);
    uint64_t polyplug_runtime_find_by_bundle(RuntimeHandle rt, uint64_t bundle_id, uint64_t contract_id, uint32_t min_version);
    size_t polyplug_runtime_find_all_by_contract(RuntimeHandle rt, uint64_t contract_id, uint32_t min_version, uint64_t* out, size_t out_cap);
    const polyplug::ResolveHandle* polyplug_runtime_resolve_plugin(RuntimeHandle rt, uint64_t packed_handle);
    void polyplug_runtime_release_plugin(const polyplug::ResolveHandle* handle);
    size_t polyplug_runtime_last_error(RuntimeHandle rt, uint8_t* buf, size_t buf_len);
    void polyplug_runtime_on_reload(void (*cb)(void* phase));
    void polyplug_runtime_set_config(const void* config);
}

namespace polyplug {

/// C-compatible runtime configuration for FFI boundary.
struct RuntimeConfigC {
    uint8_t hot_reload_enabled;
    uint32_t hot_reload_max_retries;
    uint64_t hot_reload_retry_interval_ms;
    uint8_t hot_reload_abort_on_max_retries;
};

/// RAII guard for a resolved plugin handle.
/// Holds a ref-counted ResolveHandle that keeps the vtable alive.
/// Move-only; copy is disabled.
class PluginGuard {
public:
    PluginGuard() noexcept : handle_(nullptr) {}

    explicit PluginGuard(const ResolveHandle* handle) noexcept : handle_(handle) {}

    ~PluginGuard() noexcept {
        release();
    }

    PluginGuard(PluginGuard&& other) noexcept : handle_(other.handle_) {
        other.handle_ = nullptr;
    }

    PluginGuard& operator=(PluginGuard&& other) noexcept {
        if (this != &other) {
            release();
            handle_ = other.handle_;
            other.handle_ = nullptr;
        }
        return *this;
    }

    PluginGuard(const PluginGuard&) = delete;
    PluginGuard& operator=(const PluginGuard&) = delete;

    const PluginInterface* vtable() const noexcept {
        if (handle_ == nullptr) {
            return nullptr;
        }
        // ResolveHandle's first field is the vtable pointer
        return static_cast<const PluginInterface*>(static_cast<const void* const*>(static_cast<const void*>(handle_))[0]);
    }

    bool is_null() const noexcept {
        return handle_ == nullptr;
    }

    explicit operator bool() const noexcept {
        return !is_null();
    }

    void reset() noexcept {
        release();
    }

private:
    void release() noexcept {
        if (handle_ != nullptr) {
            polyplug_runtime_release_plugin(handle_);
            handle_ = nullptr;
        }
    }

    const ResolveHandle* handle_;
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

    PluginGuard resolve_plugin(uint64_t packed_handle) const noexcept {
        if (packed_handle == UINT64_MAX) {
            return PluginGuard(nullptr);
        }
        const ResolveHandle* h = polyplug_runtime_resolve_plugin(handle_, packed_handle);
        return PluginGuard(h);
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
        config_c.hot_reload_enabled = config.hot_reload_enabled ? 1 : 0;
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