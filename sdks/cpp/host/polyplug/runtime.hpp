// THIS FILE IS PART OF polyplug — header-only C++ binding.
// RAII Runtime wrapper and fluent Builder for the polyplug plugin runtime.
// Updated for HostApi-based API (18-04 refactor).
// All FFI struct types are imported from auto-generated abi.hpp (per D-26).

#pragma once

#include "../../abi/polyplug/abi.hpp"
#include "error.hpp"
#include "handle.hpp"

#include <algorithm>
#include <cstdint>
#include <filesystem>
#include <functional>
#include <memory>
#include <optional>
#include <stdexcept>
#include <string>
#include <string_view>
#include <system_error>
#include <vector>

static_assert(POLYPLUG_ABI_VERSION == 1,
    "polyplug header version mismatch — recompile against updated headers");

struct ResolveHandle;

extern "C" {
    // ─── FFI Exports: create and destroy ─────────────────────────────────────────

    /// Create a new runtime instance.
    /// Pass null for default config, or pointer to RuntimeConfig for custom settings.
    /// Returns HostApi* for all operations.
    const HostApi* polyplug_runtime_create(const RuntimeConfig* config);

    /// Destroy a runtime instance.
    /// Takes HostApi* returned by polyplug_runtime_create.
    void polyplug_runtime_destroy(const HostApi* host);
}

namespace polyplug {

// ─── on_reload trampoline ─────────────────────────────────────────────────────
namespace detail {

/// Owned storage for the user-provided on_reload callback.
///
/// The functor is owned by the Runtime instance (no globals — Rule 12). A stable
/// pointer to this storage is passed to the runtime as `on_reload_user_data` and
/// forwarded back to `on_reload_trampoline` on every invocation.
using OnReloadFn = std::function<void(const ReloadPhase&)>;

/// C ABI trampoline matching RuntimeConfig_on_reload_fn:
/// void(*)(void* user_data, ReloadPhase). Recovers the owning functor from
/// `user_data` and invokes it.
inline void on_reload_trampoline(void* user_data, ReloadPhase phase) {
    if (user_data != nullptr) {
        auto* cb = static_cast<OnReloadFn*>(user_data);
        if (*cb) {
            (*cb)(phase);
        }
    }
}

} // namespace detail

class Runtime {
public:
    class Builder {
    public:
        /// Add a directory to scan for plugin bundles during `build()`.
        /// Mirrors the Rust RuntimeBuilder: every bundle subdirectory containing
        /// a `manifest.toml` is loaded (sorted order) after the runtime is
        /// created. Bundles requiring a language loader can only load after that
        /// loader is registered — hosts registering loaders after `build()` must
        /// load such bundles explicitly instead of using `plugin_dir()`.
        Builder& plugin_dir(std::string_view path) {
            plugin_dirs_.emplace_back(path);
            return *this;
        }

        /// Set the compatibility mode (`Compatibility` discriminant) used for
        /// version resolution. Overrides the `compatibility` field of a config
        /// passed via `config()`.
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
            Runtime rt = create_runtime();
            load_plugin_dirs(rt);
            return rt;
        }

    private:
        Runtime create_runtime() {
            if (config_.has_value() || on_reload_cb_.has_value() || compatibility_.has_value()) {
                // Build a RuntimeConfig from stored options.
                RuntimeConfig cfg{};
                if (config_.has_value()) {
                    cfg = config_.value();
                }
                // compatibility() wins over a config()-supplied value.
                if (compatibility_.has_value()) {
                    cfg.compatibility = static_cast<Compatibility>(compatibility_.value());
                }
                // The owned functor (if any) is heap-allocated so its address is
                // stable; the pointer is handed to the runtime as on_reload_user_data
                // and forwarded back to the trampoline. The Runtime keeps it alive.
                std::unique_ptr<detail::OnReloadFn> cb{};
                if (on_reload_cb_.has_value()) {
                    cb = std::make_unique<detail::OnReloadFn>(std::move(on_reload_cb_.value()));
                    cfg.on_reload = detail::on_reload_trampoline;
                    cfg.on_reload_user_data = cb.get();
                }
                const HostApi* h = polyplug_runtime_create(&cfg);
                if (h == nullptr) {
                    throw std::runtime_error("polyplug_runtime_create returned null");
                }
                return Runtime(h, std::move(cb));
            } else {
                // No config, callback, or compatibility — pass null for defaults.
                const HostApi* h = polyplug_runtime_create(nullptr);
                if (h == nullptr) {
                    throw std::runtime_error("polyplug_runtime_create returned null");
                }
                return Runtime(h, nullptr);
            }
        }

        /// Scan each stored plugin directory for bundle subdirectories
        /// (containing `manifest.toml`) and load them in sorted order —
        /// mirroring the Rust builder's scan-and-load-at-build semantics.
        void load_plugin_dirs(Runtime& rt) const {
            namespace fs = std::filesystem;
            for (const std::string& dir : plugin_dirs_) {
                std::error_code ec{};
                if (!fs::is_directory(dir, ec)) {
                    continue;
                }
                std::vector<std::string> bundle_dirs{};
                for (const fs::directory_entry& entry : fs::directory_iterator(dir, ec)) {
                    if (entry.is_directory() && fs::exists(entry.path() / "manifest.toml")) {
                        bundle_dirs.push_back(entry.path().string());
                    }
                }
                std::sort(bundle_dirs.begin(), bundle_dirs.end());
                for (const std::string& bundle_dir : bundle_dirs) {
                    rt.load_bundle(bundle_dir);
                }
            }
        }

        std::vector<std::string> plugin_dirs_{};
        std::optional<uint32_t> compatibility_{};
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

    Runtime(Runtime&& other) noexcept
        : host_(other.host_), on_reload_cb_(std::move(other.on_reload_cb_)) {
        other.host_ = nullptr;
    }

    Runtime& operator=(Runtime&& other) noexcept {
        if (this != &other) {
            if (host_ != nullptr) {
                polyplug_runtime_destroy(host_);
            }
            host_ = other.host_;
            on_reload_cb_ = std::move(other.on_reload_cb_);
            other.host_ = nullptr;
        }
        return *this;
    }

    Runtime(const Runtime&) = delete;
    Runtime& operator=(const Runtime&) = delete;

    // ─── Operations via HostApi fields ─────────────────────────────────────

    /// Load a plugin bundle from path.
    /// Calls through HostApi.load_bundle field.
    void load_bundle(std::string_view path) {
        ensure_host();
        // Cast function pointer and call with self-passing pattern.
        // Returns AbiError, not uint32_t.
        auto func = reinterpret_cast<AbiError(*)(const HostApi*, const uint8_t*, size_t)>(host_->load_bundle);
        AbiError result = func(host_, reinterpret_cast<const uint8_t*>(path.data()), path.size());
        if (result.code != static_cast<uint32_t>(AbiErrorCode::Ok)) {
            throw std::runtime_error("load_bundle failed: " + get_last_error());
        }
    }

    /// Reload a plugin bundle (hot-reload).
    /// Calls through HostApi.reload_bundle field.
    void reload_bundle(std::string_view path) {
        ensure_host();
        auto func = reinterpret_cast<AbiError(*)(const HostApi*, const uint8_t*, size_t)>(host_->reload_bundle);
        AbiError result = func(host_, reinterpret_cast<const uint8_t*>(path.data()), path.size());
        if (result.code != static_cast<uint32_t>(AbiErrorCode::Ok)) {
            throw std::runtime_error("reload_bundle failed: " + get_last_error());
        }
    }

    /// Unload a plugin bundle by bundle ID.
    /// Calls through HostApi.unload_bundle field.
    void unload_bundle(uint64_t bundle_id) {
        ensure_host();
        auto func = reinterpret_cast<AbiError(*)(const HostApi*, uint64_t)>(host_->unload_bundle);
        AbiError result = func(host_, bundle_id);
        if (result.code != static_cast<uint32_t>(AbiErrorCode::Ok)) {
            throw std::runtime_error("unload_bundle failed: " + get_last_error());
        }
    }

    /// Find a guest contract by contract_id and minimum version.
    /// Calls through HostApi.find_guest_contract field.
    /// Returns GuestContractHandle (8 bytes: u32 index + u32 generation), or invalid_handle() if not found.
    GuestContractHandle find_guest_contract(uint64_t contract_id, uint32_t min_version) const {
        ensure_host();
        auto func = reinterpret_cast<GuestContractHandle(*)(const HostApi*, uint64_t, uint32_t)>(host_->find_guest_contract);
        return func(host_, contract_id, min_version);
    }

    /// Find all guest contracts matching contract_id.
    /// Calls through HostApi.find_all_guest_contracts field.
    /// Returns vector of GuestContractHandle (8 bytes each: u32 index + u32 generation).
    /// The ABI Array (items, len, align) is freed via host->free after copying into the vector.
    std::vector<GuestContractHandle> find_all_guest_contracts(uint64_t contract_id, uint32_t min_version, size_t cap = 64) const {
        ensure_host();
        Array arr = host_->find_all_guest_contracts(host_, contract_id, min_version);

        std::vector<GuestContractHandle> handles;
        handles.reserve(arr.len);
        auto* ptr = static_cast<GuestContractHandle*>(arr.items);
        for (size_t i = 0; i < arr.len && i < cap; ++i) {
            handles.push_back(ptr[i]);
        }
        // Free the array via HostApi.free (size = len * sizeof(GuestContractHandle)).
        if (arr.items != nullptr && arr.len > 0) {
            host_->free(host_, static_cast<uint8_t*>(arr.items),
                        arr.len * sizeof(GuestContractHandle), arr.align);
        }
        return handles;
    }

    /// Resolve a GuestContractHandle to a GuestContractInterface pointer.
    /// Calls through HostApi.resolve_guest_contract field.
    /// Returns null if the handle is invalid or contract was unloaded.
    const GuestContractInterface* resolve_guest_contract(GuestContractHandle handle) const {
        if (!is_valid(handle)) {
            return nullptr;
        }
        ensure_host();
        auto func = reinterpret_cast<const GuestContractInterface*(*)(const HostApi*, GuestContractHandle)>(host_->resolve_guest_contract);
        return func(host_, handle);
    }

    /// Register a host contract interface with the runtime.
    /// Calls through HostApi.register_host_contract field.
    void register_host_contract(const HostContractInterface* interface) {
        if (interface == nullptr) {
            throw std::runtime_error("register_host_contract: null interface pointer");
        }
        ensure_host();
        auto func = reinterpret_cast<AbiError(*)(const HostApi*, const HostContractInterface*)>(host_->register_host_contract);
        AbiError result = func(host_, interface);
        if (result.code != static_cast<uint32_t>(AbiErrorCode::Ok)) {
            throw std::runtime_error("register_host_contract failed: " + get_last_error());
        }
    }

    /// Get the HostApi pointer.
    const HostApi* host() const noexcept {
        return host_;
    }

    /// Get last error message.
    std::string get_last_error() const {
        ensure_host();
        auto len_func = reinterpret_cast<size_t(*)(const HostApi*)>(host_->get_error_len);
        size_t len = len_func(host_);
        if (len == 0) return "";
        std::vector<char> buf(len);
        auto err_func = reinterpret_cast<size_t(*)(const HostApi*, uint8_t*, size_t)>(host_->get_last_error);
        err_func(host_, reinterpret_cast<uint8_t*>(buf.data()), len);
        return std::string(buf.data(), len);
    }

private:
    Runtime(const HostApi* h, std::unique_ptr<detail::OnReloadFn> cb) noexcept
        : host_(h), on_reload_cb_(std::move(cb)) {}

    void ensure_host() const {
        if (host_ == nullptr) {
            throw std::runtime_error("Runtime is destroyed");
        }
    }

    const HostApi* host_ = nullptr;  // HostApi pointer
    // Owns the on_reload functor referenced by RuntimeConfig.on_reload_user_data.
    // Must outlive the runtime so the trampoline's user_data stays valid.
    std::unique_ptr<detail::OnReloadFn> on_reload_cb_{};
};

} // namespace polyplug
