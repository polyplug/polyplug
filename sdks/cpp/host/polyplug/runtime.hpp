// THIS FILE IS PART OF polyplug — header-only C++ binding.
// RAII Runtime wrapper and fluent Builder for the polyplug plugin runtime.
// Updated for HostInterface-based API (18-04 refactor).

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

// ─── HostInterface Struct (C ABI, 144 bytes) ─────────────────────────────────────

/// HostInterface struct — function table for all runtime operations.
/// Matches Rust #[repr(C)] layout exactly (ABI stability).
/// Size: 144 bytes (18 pointer fields).
struct HostInterface {
    /// Opaque runtime pointer.
    void* runtime;                          // offset 0

    /// Register a guest contract implementation.
    /// Called by plugins during polyplug_init().
    void* register_contract;               // offset 8

    /// Allocate memory using host allocator.
    void* alloc;                           // offset 16

    /// Free memory allocated via alloc.
    void* free;                            // offset 24

    /// Find a guest contract by contract_id and minimum version.
    void* find_guest_contract;             // offset 32

    /// Find all guest contracts matching contract_id.
    void* find_all_guest_contracts;        // offset 40

    /// Resolve a GuestContractHandle to an interface pointer.
    void* resolve_guest_contract;          // offset 48

    /// Call a method on a guest contract instance.
    void* call_guest_method;               // offset 56

    /// Get a host contract instance by contract_id.
    void* get_host_contract;               // offset 64

    /// Resolve a host contract interface by contract_id.
    void* resolve_host_contract_interface; // offset 72

    /// List all loaded bundles.
    void* list_bundles;                    // offset 80

    /// Get dependencies for the calling bundle.
    void* get_dependencies;                // offset 88

    /// Load a plugin bundle from path.
    void* load_bundle;                     // offset 96

    /// Reload a plugin bundle (hot-reload).
    void* reload_bundle;                   // offset 104

    /// Register a host contract interface.
    void* register_host_contract;          // offset 112

    /// Register a language loader.
    void* register_loader;                 // offset 120

    /// Get last error message.
    void* get_last_error;                  // offset 128

    /// Get last error message length.
    void* get_error_len;                   // offset 136
};

static_assert(sizeof(HostInterface) == 144, "HostInterface must be 144 bytes (18 pointer fields)");

struct ResolveHandle;

} // namespace polyplug

extern "C" {
    // ─── FFI Exports: Only create and destroy ─────────────────────────────────────

    /// Create a new runtime instance with default configuration.
    /// Returns HostInterface* for all operations.
    const polyplug::HostInterface* polyplug_runtime_create();

    /// Create a new runtime instance with options.
    /// Returns HostInterface* for all operations.
    const polyplug::HostInterface* polyplug_runtime_create_with_options(const void* options);

    /// Destroy a runtime instance.
    /// Takes HostInterface* returned by polyplug_runtime_create.
    void polyplug_runtime_destroy(const polyplug::HostInterface* host);
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

        Builder& config(const polyplug::RuntimeConfig& cfg) noexcept {
            config_ = cfg;
            return *this;
        }

        Builder& on_reload(std::function<void(const ReloadPhase&)> cb) noexcept {
            on_reload_cb_ = std::move(cb);
            return *this;
        }

        Runtime build() {
            // Build with options if config or callback set
            if (config_.has_value() || on_reload_cb_.has_value()) {
                RuntimeConfig config_c{};
                if (config_.has_value()) {
                    config_c.hot_reload_enabled = config_->hot_reload_enabled ? 1 : 0;
                    config_c.hot_reload_max_retries = config_->hot_reload_max_retries;
                    config_c.hot_reload_retry_interval_ms = static_cast<uint64_t>(
                        config_->hot_reload_retry_interval.count());
                    config_c.hot_reload_abort_on_max_retries = config_->hot_reload_abort_on_max_retries ? 1 : 0;
                    config_c.compatibility = config_->compatibility;
                }

                // Create options struct (simplified for FFI)
                // Note: on_reload callback would need FFI callback wrapper
                // For now, we pass config only
                const HostInterface* h = polyplug_runtime_create();
                if (h == nullptr) {
                    throw std::runtime_error("polyplug_runtime_create returned null");
                }
                return Runtime(h);
            } else {
                const HostInterface* h = polyplug_runtime_create();
                if (h == nullptr) {
                    throw std::runtime_error("polyplug_runtime_create returned null");
                }
                return Runtime(h);
            }
        }

    private:
        std::vector<std::string> plugin_dirs_{};
        uint32_t compatibility_{0U};
        std::optional<polyplug::RuntimeConfig> config_{};
        std::optional<std::function<void(const ReloadPhase&)>> on_reload_cb_{};
    };

    static Builder builder() noexcept {
        return Builder{};
    }

    ~Runtime() noexcept {
        if (host_ != nullptr) {
            polyplug_runtime_destroy(host_);
            host_ = nullptr;
        }
    }

    Runtime(Runtime&& other) noexcept : host_(other.host_) {
        other.host_ = nullptr;
    }

    Runtime& operator=(Runtime&& other) noexcept {
        if (this != &other) {
            if (host_ != nullptr) {
                polyplug_runtime_destroy(host_);
            }
            host_ = other.host_;
            other.host_ = nullptr;
        }
        return *this;
    }

    Runtime(const Runtime&) = delete;
    Runtime& operator=(const Runtime&) = delete;

    // ─── Operations via HostInterface fields ─────────────────────────────────────

    /// Load a plugin bundle from path.
    /// Calls through HostInterface.load_bundle field.
    void load_bundle(std::string_view path) {
        ensure_host();
        // Cast function pointer and call with self-passing pattern
        auto func = reinterpret_cast<uint32_t(*)(const HostInterface*, const uint8_t*, size_t)>(host_->load_bundle);
        uint32_t result = func(host_, reinterpret_cast<const uint8_t*>(path.data()), path.size());
        if (result != 0) {
            throw std::runtime_error("load_bundle failed: " + get_last_error());
        }
    }

    /// Reload a plugin bundle (hot-reload).
    /// Calls through HostInterface.reload_bundle field.
    void reload_bundle(std::string_view path) {
        ensure_host();
        // Cast function pointer and call with self-passing pattern
        auto func = reinterpret_cast<uint32_t(*)(const HostInterface*, const uint8_t*, size_t)>(host_->reload_bundle);
        uint32_t result = func(host_, reinterpret_cast<const uint8_t*>(path.data()), path.size());
        if (result != 0) {
            throw std::runtime_error("reload_bundle failed: " + get_last_error());
        }
    }

    /// Find a guest contract by contract_id and minimum version.
    /// Calls through HostInterface.find_guest_contract field.
    uint64_t find_guest_contract(uint64_t contract_id, uint32_t min_version) const noexcept {
        ensure_host();
        auto func = reinterpret_cast<uint64_t(*)(const HostInterface*, uint64_t, uint32_t)>(host_->find_guest_contract);
        return func(host_, contract_id, min_version);
    }

    /// Find guest contract by bundle_id (deprecated, not in HostInterface).
    /// Returns NULL_HANDLE (UINT64_MAX) since this was removed from FFI surface.
    uint64_t find_by_bundle(uint64_t bundle_id, uint64_t contract_id, uint32_t min_version) const noexcept {
        // Note: find_by_bundle is not in HostInterface (18-02 removed from FFI surface)
        // This method is deprecated and returns NULL_HANDLE
        return UINT64_MAX;
    }

    /// Find all guest contracts matching contract_id.
    /// Calls through HostInterface.find_all_guest_contracts field.
    std::vector<uint64_t> find_all_guest_contracts(uint64_t contract_id, uint32_t min_version, size_t cap = 64) const {
        ensure_host();
        // The function returns Array<GuestContractHandle> struct { ptr, len }
        struct ArrayResult {
            uint64_t* ptr;
            size_t len;
        };
        auto func = reinterpret_cast<ArrayResult(*)(const HostInterface*, uint64_t, uint32_t)>(host_->find_all_guest_contracts);
        ArrayResult arr = func(host_, contract_id, min_version);
        std::vector<uint64_t> handles;
        handles.reserve(arr.len);
        for (size_t i = 0; i < arr.len && i < cap; ++i) {
            handles.push_back(arr.ptr[i]);
        }
        // Free the array via HostInterface.free
        if (arr.ptr != nullptr && arr.len > 0) {
            auto free_func = reinterpret_cast<void(*)(const HostInterface*, void*, size_t, size_t)>(host_->free);
            free_func(host_, arr.ptr, arr.len * sizeof(uint64_t), alignof(uint64_t));
        }
        return handles;
    }

    /// Resolve a packed handle to a ResolveHandle pointer.
    /// Calls through HostInterface.resolve_guest_contract field.
    const ResolveHandle* resolve_guest_contract(uint64_t packed_handle) const noexcept {
        if (packed_handle == UINT64_MAX) {
            return nullptr;
        }
        ensure_host();
        auto func = reinterpret_cast<const ResolveHandle*(*)(const HostInterface*, uint64_t)>(host_->resolve_guest_contract);
        return func(host_, packed_handle);
    }

    /// Register a host contract interface with the runtime.
    /// Calls through HostInterface.register_host_contract field.
    void register_host_contract(const HostContractInterface* interface) {
        if (interface == nullptr) {
            throw std::runtime_error("register_host_contract: null interface pointer");
        }
        ensure_host();
        auto func = reinterpret_cast<uint32_t(*)(const HostInterface*, const HostContractInterface*)>(host_->register_host_contract);
        uint32_t result = func(host_, interface);
        if (result == 1) {
            throw std::runtime_error("register_host_contract: null interface pointer");
        } else if (result == 2) {
            throw std::runtime_error("register_host_contract: duplicate contract registration");
        } else if (result != 0) {
            throw std::runtime_error("register_host_contract failed: " + get_last_error());
        }
    }

    /// Get the HostInterface pointer.
    const HostInterface* host() const noexcept {
        return host_;
    }

    /// Get last error message.
    std::string get_last_error() const {
        ensure_host();
        auto len_func = reinterpret_cast<size_t(*)(const HostInterface*)>(host_->get_error_len);
        size_t len = len_func(host_);
        if (len == 0) return "";
        std::vector<char> buf(len);
        auto err_func = reinterpret_cast<size_t(*)(const HostInterface*, uint8_t*, size_t)>(host_->get_last_error);
        err_func(host_, reinterpret_cast<uint8_t*>(buf.data()), len);
        return std::string(buf.data(), len);
    }

    // ─── Backward Compatibility Aliases ─────────────────────────────────────────

    /// Alias for find_guest_contract (deprecated).
    uint64_t find(uint64_t contract_id, uint32_t min_version) const noexcept {
        return find_guest_contract(contract_id, min_version);
    }

    /// Alias for find_all_guest_contracts (deprecated).
    std::vector<uint64_t> find_all_by_contract(uint64_t contract_id, uint32_t min_version, size_t cap = 64) const {
        return find_all_guest_contracts(contract_id, min_version, cap);
    }

    /// Alias for resolve_guest_contract (deprecated).
    const ResolveHandle* resolve_plugin(uint64_t packed_handle) const noexcept {
        return resolve_guest_contract(packed_handle);
    }

    /// Release a resolved plugin handle (no-op in HostInterface model).
    /// The reference counting is handled internally.
    void release_plugin(const ResolveHandle* handle) const noexcept {
        // Note: release_plugin is not in HostInterface (18-02 removed from FFI surface)
        // Reference counting is handled internally by the registry
        // This method is deprecated and does nothing
    }

    static void set_config(const polyplug::RuntimeConfig& config) {
        // Note: set_config was removed from FFI surface in 18-02
        // Config is now passed via Runtime::Builder
    }

private:
    explicit Runtime(const HostInterface* h) noexcept : host_(h) {}

    void ensure_host() const {
        if (host_ == nullptr) {
            throw std::runtime_error("Runtime is destroyed");
        }
    }

    const HostInterface* host_ = nullptr;  // HostInterface pointer
};

} // namespace polyplug