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
    uint32_t polyplug_runtime_register_host_contract(RuntimeHandle rt, const HostContractVTable* vtable);
}

/// FFI RuntimeConfig matching polyplug_abi::RuntimeConfig (24 bytes).
/// Must be in global namespace to match extern "C" FFI pattern.
/// Layout verified against Rust offset tests.
struct RuntimeConfig {
    uint8_t hot_reload_enabled;           // offset 0, 1 byte
    // padding 3 bytes (offset 1-3)
    uint32_t hot_reload_max_retries;      // offset 4, 4 bytes
    uint64_t hot_reload_retry_interval_ms; // offset 8, 8 bytes
    uint8_t hot_reload_abort_on_max_retries; // offset 16, 1 byte
    // padding 3 bytes (offset 17-19)
    uint32_t compatibility;               // offset 20, 4 bytes (Compatibility enum: Strict=0, Relaxed=1, Yolo=2)
};

static_assert(sizeof(RuntimeConfig) == 24, "RuntimeConfig must be 24 bytes");

namespace polyplug {

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

    /// Resolve a packed handle to a raw resolve_handle.
    ///
    /// In the instance-based model, callers should:
    /// 1. Get resolve_handle from resolve_plugin
    /// 2. Access GuestContractInterface via FFI (ResolveHandle first field)
    /// 3. Call create_instance on interface for stateful access
    /// 4. Make dispatch calls with instance
    /// 5. Call destroy_instance before hot-reload
    /// 6. Call release_plugin when done with the handle
    const ResolveHandle* resolve_plugin(uint64_t packed_handle) const noexcept {
        if (packed_handle == UINT64_MAX) {
            return nullptr;
        }
        return polyplug_runtime_resolve_plugin(handle_, packed_handle);
    }

    /// Release a resolved plugin handle.
    /// Call this when done with a handle to decrement the refcount.
    void release_plugin(const ResolveHandle* handle) const noexcept {
        if (handle != nullptr) {
            polyplug_runtime_release_plugin(handle);
        }
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

    void register_host_contract(const HostContractVTable* vtable) {
        if (vtable == nullptr) {
            throw std::runtime_error("register_host_contract: null vtable pointer");
        }
        uint32_t result = polyplug_runtime_register_host_contract(handle_, vtable);
        if (result != 0) {
            throw std::runtime_error("Failed to register host contract: error " + std::to_string(result));
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

    static void set_config(const polyplug::RuntimeConfig& config) {
        ::RuntimeConfig config_c{};  // Global namespace FFI struct
        config_c.hot_reload_enabled = config.hot_reload_enabled ? 1 : 0;
        config_c.hot_reload_max_retries = config.hot_reload_max_retries;
        config_c.hot_reload_retry_interval_ms = static_cast<uint64_t>(
            config.hot_reload_retry_interval.count());
        config_c.hot_reload_abort_on_max_retries = config.hot_reload_abort_on_max_retries ? 1 : 0;
        config_c.compatibility = config.compatibility;
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