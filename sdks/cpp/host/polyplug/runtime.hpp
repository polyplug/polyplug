// THIS FILE IS PART OF polyplug — header-only C++ binding.
// RAII Runtime wrapper and fluent Builder for the polyplug plugin runtime.
// Updated for HostInterface-based API (18-04 refactor).
// All FFI struct types are imported from auto-generated abi.hpp (per D-26).

#pragma once

#include "polyplug/abi.hpp"
#include "error.hpp"
#include "handle.hpp"

#include <cstdint>
#include <functional>
#include <optional>
#include <stdexcept>
#include <string>
#include <string_view>
#include <vector>

static_assert(POLYPLUG_ABI_VERSION == 1,
    "polyplug header version mismatch — recompile against updated headers");

struct ResolveHandle;

extern "C" {
    // ─── FFI Exports: create and destroy ─────────────────────────────────────────

    /// Create a new runtime instance.
    /// Pass null for default config, or pointer to RuntimeConfig for custom settings.
    /// Returns HostInterface* for all operations.
    const HostInterface* polyplug_runtime_create(const RuntimeConfig* config);

    /// Destroy a runtime instance.
    /// Takes HostInterface* returned by polyplug_runtime_create.
    void polyplug_runtime_destroy(const HostInterface* host);
}

namespace polyplug {

// ─── Static callback storage for on_reload trampoline ─────────────────────────
namespace detail {

/// Storage for the user-provided on_reload callback.
/// The C ABI requires a plain function pointer (RuntimeConfig_on_reload_fn),
/// so we store the std::function here and invoke it from a static trampoline.
inline std::function<void(const ReloadPhase&)>& on_reload_storage() noexcept {
    static std::function<void(const ReloadPhase&)> cb;
    return cb;
}

/// C ABI trampoline that dispatches to the stored std::function.
/// Signature matches RuntimeConfig_on_reload_fn: void(*)(ReloadPhase).
inline void on_reload_trampoline(ReloadPhase phase) {
    auto& cb = on_reload_storage();
    if (cb) {
        cb(phase);
    }
}

} // namespace detail

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

        Builder& config(const RuntimeConfig& cfg) noexcept {
            config_ = cfg;
            return *this;
        }

        Builder& on_reload(std::function<void(const ReloadPhase&)> cb) noexcept {
            on_reload_cb_ = std::move(cb);
            return *this;
        }

        Runtime build() {
            if (config_.has_value() || on_reload_cb_.has_value()) {
                // Build a RuntimeConfig from stored options.
                RuntimeConfig cfg{};
                if (config_.has_value()) {
                    cfg = config_.value();
                }
                if (on_reload_cb_.has_value()) {
                    // Store the callback in static storage so the C trampoline
                    // can invoke it. The trampoline function pointer is passed
                    // to the runtime via RuntimeConfig.on_reload.
                    detail::on_reload_storage() = std::move(on_reload_cb_.value());
                    cfg.on_reload = detail::on_reload_trampoline;
                }
                const HostInterface* h = polyplug_runtime_create(&cfg);
                if (h == nullptr) {
                    throw std::runtime_error("polyplug_runtime_create returned null");
                }
                return Runtime(h);
            } else {
                // No config or callback — pass null for defaults.
                const HostInterface* h = polyplug_runtime_create(nullptr);
                if (h == nullptr) {
                    throw std::runtime_error("polyplug_runtime_create returned null");
                }
                return Runtime(h);
            }
        }

    private:
        std::vector<std::string> plugin_dirs_{};
        uint32_t compatibility_{0U};
        std::optional<RuntimeConfig> config_{};
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
        // Cast function pointer and call with self-passing pattern.
        // Returns AbiError, not uint32_t.
        auto func = reinterpret_cast<AbiError(*)(const HostInterface*, const uint8_t*, size_t)>(host_->load_bundle);
        AbiError result = func(host_, reinterpret_cast<const uint8_t*>(path.data()), path.size());
        if (result.code != AbiErrorCode::Ok) {
            throw std::runtime_error("load_bundle failed: " + get_last_error());
        }
    }

    /// Reload a plugin bundle (hot-reload).
    /// Calls through HostInterface.reload_bundle field.
    void reload_bundle(std::string_view path) {
        ensure_host();
        auto func = reinterpret_cast<AbiError(*)(const HostInterface*, const uint8_t*, size_t)>(host_->reload_bundle);
        AbiError result = func(host_, reinterpret_cast<const uint8_t*>(path.data()), path.size());
        if (result.code != AbiErrorCode::Ok) {
            throw std::runtime_error("reload_bundle failed: " + get_last_error());
        }
    }

    /// Find a guest contract by contract_id and minimum version.
    /// Calls through HostInterface.find_guest_contract field.
    /// Returns GuestContractHandle (4 bytes: single u32 index), or invalid_handle() if not found.
    GuestContractHandle find_guest_contract(uint64_t contract_id, uint32_t min_version) const {
        ensure_host();
        auto func = reinterpret_cast<GuestContractHandle(*)(const HostInterface*, uint64_t, uint32_t)>(host_->find_guest_contract);
        return func(host_, contract_id, min_version);
    }

    /// Find all guest contracts matching contract_id.
    /// Calls through HostInterface.find_all_guest_contracts field.
    /// Returns vector of GuestContractHandle (ABI Array with 3 fields: items, len, align).
    std::vector<GuestContractHandle> find_all_guest_contracts(uint64_t contract_id, uint32_t min_version, size_t cap = 64) const {
        ensure_host();
        // The function returns Array (3 fields: void* items, size_t len, size_t align).
        auto func = reinterpret_cast<Array(*)(const HostInterface*, uint64_t, uint32_t)>(host_->find_all_guest_contracts);
        Array arr = func(host_, contract_id, min_version);

        std::vector<GuestContractHandle> handles;
        handles.reserve(arr.len);
        auto* ptr = static_cast<GuestContractHandle*>(arr.items);
        for (size_t i = 0; i < arr.len && i < cap; ++i) {
            handles.push_back(ptr[i]);
        }
        // Free the array via HostInterface.free (size = len * sizeof(GuestContractHandle)).
        if (arr.items != nullptr && arr.len > 0) {
            auto free_func = reinterpret_cast<void(*)(const HostInterface*, void*, size_t, size_t)>(host_->free);
            free_func(host_, arr.items, arr.len * sizeof(GuestContractHandle), arr.align);
        }
        return handles;
    }

    /// Resolve a GuestContractHandle to a GuestContractInterface pointer.
    /// Calls through HostInterface.resolve_guest_contract field.
    /// Returns null if the handle is invalid or contract was unloaded.
    const GuestContractInterface* resolve_guest_contract(GuestContractHandle handle) const {
        if (!is_valid(handle)) {
            return nullptr;
        }
        ensure_host();
        auto func = reinterpret_cast<const GuestContractInterface*(*)(const HostInterface*, GuestContractHandle)>(host_->resolve_guest_contract);
        return func(host_, handle);
    }

    /// Register a host contract interface with the runtime.
    /// Calls through HostInterface.register_host_contract field.
    void register_host_contract(const HostContractInterface* interface) {
        if (interface == nullptr) {
            throw std::runtime_error("register_host_contract: null interface pointer");
        }
        ensure_host();
        auto func = reinterpret_cast<AbiError(*)(const HostInterface*, const HostContractInterface*)>(host_->register_host_contract);
        AbiError result = func(host_, interface);
        if (result.code != AbiErrorCode::Ok) {
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
    GuestContractHandle find(uint64_t contract_id, uint32_t min_version) const {
        return find_guest_contract(contract_id, min_version);
    }

    /// Alias for find_all_guest_contracts (deprecated).
    std::vector<GuestContractHandle> find_all_by_contract(uint64_t contract_id, uint32_t min_version, size_t cap = 64) const {
        return find_all_guest_contracts(contract_id, min_version, cap);
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
