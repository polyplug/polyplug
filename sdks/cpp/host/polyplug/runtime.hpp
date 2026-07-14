// THIS FILE IS PART OF polyplug — header-only C++ binding.
// RAII Runtime wrapper and fluent Builder for the polyplug plugin runtime.
// Updated for HostApi-based API (18-04 refactor).
// All FFI struct types are imported from auto-generated abi.hpp (per D-26).

#pragma once

#include "../../abi/polyplug/abi.hpp"
#include "error.hpp"
#include "handle.hpp"

#include <algorithm>
#include <array>
#include <cstdint>
#include <filesystem>
#include <functional>
#include <memory>
#include <mutex>
#include <optional>
#include <stdexcept>
#include <string>
#include <string_view>
#include <system_error>
#include <vector>

static_assert(POLYPLUG_ABI_VERSION == 2,
    "polyplug header version mismatch — recompile against updated headers");

struct ResolveHandle;

extern "C" {
    // ─── FFI Exports: create and destroy ─────────────────────────────────────────

    /// Create a new runtime instance.
    /// Pass null for default config, or pointer to RuntimeConfig for custom settings.
    /// Returns HostApi* for all operations.
    const HostApi* polyplug_runtime_create(const RuntimeConfig* config);

    /// Destroy a runtime instance.
    /// Returns false without consuming `host` when destruction must be retried on
    /// its owner thread; returns true after consuming it (and for null).
    bool polyplug_runtime_destroy(const HostApi* host);
    void polyplug_begin_internal_plugin(
        const HostApi* host,
        const uint8_t* manifest_bytes,
        size_t manifest_len,
        uint32_t language,
        uint64_t* out_bundle_id,
        AbiError* out_error);
    void polyplug_commit_internal_plugin(
        const HostApi* host,
        uint64_t bundle_id,
        AbiError* out_error);
    void polyplug_commit_internal_plugin_with_handles(
        const HostApi* host,
        uint64_t bundle_id,
        GuestContractHandle* out_handles,
        size_t handle_capacity,
        size_t* out_handle_count,
        AbiError* out_error);
    void polyplug_abort_internal_plugin(const HostApi* host, uint64_t bundle_id);
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
/// void(*)(void* user_data, const ReloadPhase* phase). Recovers the owning
/// functor from `user_data` and invokes it. The runtime guarantees `phase` is
/// non-null and valid for the duration of the call; the null check is pure
/// defence-in-depth.
inline void on_reload_trampoline(void* user_data, const ReloadPhase* phase) noexcept {
    if (user_data == nullptr || phase == nullptr) {
        return;
    }
    try {
        auto* cb = static_cast<OnReloadFn*>(user_data);
        if (*cb) {
            (*cb)(*phase);
        }
    } catch (...) {
    }
}

/// Base of generated internal-plugin residents. A resident owns every
/// generated callback, interface table, typed factory, and implementation object
/// that may be reached through a registered ABI table.
class InternalPluginResident {
public:
    virtual ~InternalPluginResident() = default;
};
class InternalPluginAbortGuard {
public:
    InternalPluginAbortGuard(const HostApi* host, uint64_t bundle_id) noexcept
        : host_(host), bundle_id_(bundle_id) {}

    ~InternalPluginAbortGuard() noexcept {
        if (!armed_) {
            return;
        }
        try {
            polyplug_abort_internal_plugin(host_, bundle_id_);
        } catch (...) {
        }
    }

    void disarm() noexcept { armed_ = false; }

private:
    const HostApi* host_;
    uint64_t bundle_id_;
    bool armed_ = true;
};


} // namespace detail

/// Result of committing one generated internal plugin.
///
/// `handles` is ordered exactly as the generated provider declarations were
/// staged, allowing generated typed callers to bind the newly committed
/// providers even when older providers implement the same contract.
struct InternalPluginCommit {
    uint64_t bundle_id;
    std::vector<GuestContractHandle> handles;
};

struct LoadedBundleDescriptor {
    uint64_t id;
    std::string name;
    Version version;
    SupportedLanguage runtime;
    BundleSourceKind source_kind;
};

struct RegisteredContractDescriptor {
    GuestContractHandle handle;
    uint64_t bundle_id;
    uint64_t contract_id;
    std::string plugin_name;
    std::string contract_name;
    Version version;
};

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

        /// Set the bundle signature enforcement policy (`SignaturePolicy`
        /// discriminant). Overrides the `signature_policy` field of a config
        /// passed via `config()`. Defaults to `SignaturePolicy::Off` (unsigned
        /// bundles load normally) when never set.
        Builder& signature_policy(SignaturePolicy policy) noexcept {
            signature_policy_ = policy;
            return *this;
        }

        /// Pin the trusted Ed25519 verifying-key allowlist (key pinning). Each
        /// entry is a 32-byte verifying key. When non-empty AND
        /// `signature_policy` is not `Off`, the runtime requires every bundle's
        /// embedded signing key to be a member of this allowlist (a re-signed
        /// bundle with an attacker key is rejected). Empty (the default) =
        /// Trust-On-First-Use: the embedded key is trusted without pinning.
        ///
        /// The keys are copied into the Builder. The runtime copies the key
        /// bytes out of `RuntimeConfig.trusted_keys` during
        /// `polyplug_runtime_create`; the buffer is only needed for that call,
        /// so `build()` holds it in a local that is released as soon as create
        /// returns.
        Builder& trusted_keys(const std::vector<std::array<uint8_t, 32>>& keys) {
            std::vector<Ed25519PublicKey> pinned{};
            pinned.reserve(keys.size());
            for (const std::array<uint8_t, 32>& key : keys) {
                Ed25519PublicKey pk{};
                std::copy(key.begin(), key.end(), pk.bytes);
                pinned.push_back(pk);
            }
            trusted_keys_ = std::move(pinned);
            return *this;
        }

        /// Pin the trusted Ed25519 verifying-key allowlist from `Ed25519PublicKey`
        /// values directly. See the `std::array` overload for the ownership and
        /// pinning semantics.
        Builder& trusted_keys(std::vector<Ed25519PublicKey> keys) {
            trusted_keys_ = std::move(keys);
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
            if (config_.has_value() || on_reload_cb_.has_value() || compatibility_.has_value()
                || signature_policy_.has_value() || !trusted_keys_.empty()) {
                // Build a RuntimeConfig from stored options.
                RuntimeConfig cfg{};
                if (config_.has_value()) {
                    cfg = config_.value();
                }
                // compatibility() wins over a config()-supplied value.
                if (compatibility_.has_value()) {
                    cfg.compatibility = static_cast<Compatibility>(compatibility_.value());
                }
                // signature_policy() wins over a config()-supplied value.
                if (signature_policy_.has_value()) {
                    cfg.signature_policy = signature_policy_.value();
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
                // The runtime copies the key bytes during
                // polyplug_runtime_create, so the buffer only needs to live
                // across that call. Keep it in a local that stays alive until
                // create returns, then let it destruct at end of scope.
                std::vector<Ed25519PublicKey> pinned_keys = std::move(trusted_keys_);
                if (!pinned_keys.empty()) {
                    cfg.trusted_keys = pinned_keys.data();
                    cfg.trusted_keys_len = pinned_keys.size();
                    cfg.trusted_keys__align = alignof(Ed25519PublicKey);
                }
                const HostApi* h = polyplug_runtime_create(&cfg);
                if (h == nullptr) {
                    throw std::runtime_error("polyplug_runtime_create returned null");
                }
                return Runtime(h, std::move(cb));
            } else {
                // No config, callback, compatibility, or signature policy — pass
                // null for defaults.
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
        std::optional<SignaturePolicy> signature_policy_{};
        std::vector<Ed25519PublicKey> trusted_keys_{};
        std::optional<RuntimeConfig> config_{};
        std::optional<std::function<void(const ReloadPhase&)>> on_reload_cb_{};
    };

    static Builder builder() noexcept {
        return Builder{};
    }

    /// Destroy this runtime and release its callbacks and internal-plugin residents
    /// only after native ownership was consumed. A false result means native ownership
    /// remains live for an owner-thread retry; once native destroy returns true, this
    /// Runtime is terminal even if native teardown caught a panic.
    bool destroy() noexcept {
        if (host_ == nullptr) {
            return true;
        }
        if (!polyplug_runtime_destroy(host_)) {
            return false;
        }
        host_ = nullptr;
        on_reload_cb_.reset();
        std::lock_guard<std::mutex> lock(internal_plugin_mutex_);
        internal_plugin_residents_.clear();
        return true;
    }

    ~Runtime() noexcept {
        if (!destroy()) {
            on_reload_cb_.release();
            std::lock_guard<std::mutex> lock(internal_plugin_mutex_);
            for (ResidentEntry& entry : internal_plugin_residents_) {
                entry.resident.release();
            }
        }
    }

    Runtime(Runtime&& other) noexcept
        : host_(other.host_), on_reload_cb_(std::move(other.on_reload_cb_)) {
        std::lock_guard<std::mutex> lock(other.internal_plugin_mutex_);
        internal_plugin_residents_ = std::move(other.internal_plugin_residents_);
        other.host_ = nullptr;
    }

    Runtime& operator=(Runtime&& other) noexcept {
        if (this != &other) {
            std::scoped_lock lock(internal_plugin_mutex_, other.internal_plugin_mutex_);
            if (host_ != nullptr) {
                if (!polyplug_runtime_destroy(host_)) {
                    return *this;
                }
                host_ = nullptr;
                on_reload_cb_.reset();
                internal_plugin_residents_.clear();
            }
            host_ = other.host_;
            on_reload_cb_ = std::move(other.on_reload_cb_);
            internal_plugin_residents_ = std::move(other.internal_plugin_residents_);
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
        // Out-param ABI: the typed field returns void and writes its AbiError
        // through the trailing pointer. Calling the field directly (no cast)
        // keeps the signature sourced from the auto-generated mirror (Rule 10).
        AbiError result{};
        host_->load_bundle(host_, reinterpret_cast<const uint8_t*>(path.data()), path.size(), &result);
        if (result.code != static_cast<uint32_t>(AbiErrorCode::Ok)) {
            throw std::runtime_error("load_bundle failed: " + get_last_error());
        }
    }

    /// Reload a plugin bundle (hot-reload).
    /// Calls through HostApi.reload_bundle field.
    void reload_bundle(std::string_view path) {
        ensure_host();
        AbiError result{};
        host_->reload_bundle(host_, reinterpret_cast<const uint8_t*>(path.data()), path.size(), &result);
        if (result.code != static_cast<uint32_t>(AbiErrorCode::Ok)) {
            throw std::runtime_error("reload_bundle failed: " + get_last_error());
        }
    }


    /// Register a generated internal plugin and return its exact committed handles.
    ///
    /// The internal plugin reports its provider count before staging. Core commits
    /// only when that count matches the staged descriptors, then writes handles in
    /// staging order. This prevents same-contract providers from being rebound through
    /// a post-commit registry lookup.
    template <typename InternalPlugin>
    InternalPluginCommit register_internal_plugin_with_handles(InternalPlugin& internal_plugin) {
        ensure_host();
        if (internal_plugin.internal_plugin_resident() == nullptr) {
            throw std::runtime_error(
                "register_internal_plugin_with_handles: internal plugin has no resident");
        }
        const std::string_view manifest = internal_plugin.internal_plugin_manifest();
        std::lock_guard<std::mutex> lock(internal_plugin_mutex_);
        internal_plugin_residents_.reserve(internal_plugin_residents_.size() + 1U);

        AbiError result{};
        uint64_t bundle_id = 0;
        polyplug_begin_internal_plugin(
            host_,
            reinterpret_cast<const uint8_t*>(manifest.data()),
            manifest.size(),
            static_cast<uint32_t>(internal_plugin.internal_plugin_language()),
            &bundle_id,
            &result);
        if (result.code != static_cast<uint32_t>(AbiErrorCode::Ok)) {
            throw std::runtime_error(
                "register_internal_plugin_with_handles failed: " + get_last_error());
        }
        detail::InternalPluginAbortGuard abort_guard{host_, bundle_id};
        result = internal_plugin.register_guest_contracts(host_);
        if (result.code != static_cast<uint32_t>(AbiErrorCode::Ok)) {
            throw std::runtime_error(
                "register_internal_plugin_with_handles failed: " + get_last_error());
        }

        std::vector<GuestContractHandle> handles(internal_plugin.internal_plugin_provider_count());
        size_t handle_count = 0;
        abort_guard.disarm();
        polyplug_commit_internal_plugin_with_handles(
            host_,
            bundle_id,
            handles.data(),
            handles.size(),
            &handle_count,
            &result);
        if (result.code != static_cast<uint32_t>(AbiErrorCode::Ok)
            || handle_count != handles.size()) {
            throw std::runtime_error(
                "register_internal_plugin_with_handles failed: " + get_last_error());
        }
        std::unique_ptr<detail::InternalPluginResident> resident =
            internal_plugin.take_internal_plugin_resident();
        if (resident == nullptr) {
            throw std::logic_error(
                "internal plugin resident disappeared during registration");
        }
        internal_plugin_residents_.push_back(ResidentEntry{bundle_id, std::move(resident)});
        return InternalPluginCommit{bundle_id, std::move(handles)};
    }

    /// Unload a plugin bundle by bundle ID.
    ///
    /// The resident remains owned by this Runtime when logical unload fails. Core
    /// drains active calls, instances, and leases before it reports success; only
    /// then is the backing C++ resident released.
    void unload_bundle(uint64_t bundle_id) {
        ensure_host();
        std::lock_guard<std::mutex> lock(internal_plugin_mutex_);
        AbiError result{};
        host_->unload_bundle(host_, bundle_id, &result);
        if (result.code != static_cast<uint32_t>(AbiErrorCode::Ok)) {
            throw std::runtime_error("unload_bundle failed: " + get_last_error());
        }
        for (auto it = internal_plugin_residents_.begin(); it != internal_plugin_residents_.end(); ++it) {
            if (it->bundle_id == bundle_id) {
                internal_plugin_residents_.erase(it);
                break;
            }
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
    /// Calls through HostApi.find_all_guest_contracts with an explicit output buffer.
    /// The ABI Array is freed via host->free after copying into the vector.
    std::vector<GuestContractHandle> find_all_guest_contracts(uint64_t contract_id, uint32_t min_version, size_t cap = 64) const {
        ensure_host();
        Array arr{};
        host_->find_all_guest_contracts(host_, contract_id, min_version, &arr);

        std::vector<GuestContractHandle> handles;
        handles.reserve(arr.len);
        auto* ptr = static_cast<GuestContractHandle*>(arr.items);
        for (size_t i = 0; i < arr.len && i < cap; ++i) {
            handles.push_back(ptr[i]);
        }
        if (arr.items != nullptr) {
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
        AbiError result{};
        host_->register_host_contract(host_, interface, &result);
        if (result.code != static_cast<uint32_t>(AbiErrorCode::Ok)) {
            throw std::runtime_error("register_host_contract failed: " + get_last_error());
        }
    }

    /// Snapshot all loaded bundle descriptors, copying and releasing caller-owned ABI strings.
    std::vector<LoadedBundleDescriptor> bundle_descriptors() const {
        ensure_host();
        if (host_->reserved == nullptr) {
            return {};
        }
        const RuntimeIntrospection* introspection = introspection_table();
        std::vector<LoadedBundleDescriptor> descriptors;
        const std::vector<uint64_t> bundle_ids = loaded_bundle_ids();
        descriptors.reserve(bundle_ids.size());
        for (const uint64_t bundle_id : bundle_ids) {
            BundleDescriptorView view{};
            if (!introspection->get_bundle_descriptor(host_, bundle_id, &view)) {
                continue;
            }
            DescriptorAllocation name{
                host_, Array{view.name, view.name_len, view.name__align}};
            descriptors.push_back(LoadedBundleDescriptor{
                view.id,
                name.copy(),
                view.version,
                view.runtime,
                view.source_kind,
            });
        }
        return descriptors;
    }

    /// Snapshot every registered guest-contract descriptor and its owning bundle.
    std::vector<RegisteredContractDescriptor> registered_contract_descriptors() const {
        ensure_host();
        if (host_->reserved == nullptr) {
            return {};
        }
        const RuntimeIntrospection* introspection = introspection_table();
        Array handles{};
        introspection->list_registered_guest_contracts(host_, &handles);
        std::vector<RegisteredContractDescriptor> descriptors;
        auto* items = static_cast<GuestContractHandle*>(handles.items);
        descriptors.reserve(handles.len);
        for (size_t index = 0; index < handles.len; ++index) {
            RegisteredContractDescriptorView view{};
            if (!introspection->get_registered_contract_descriptor(host_, items[index], &view)) {
                continue;
            }
            DescriptorAllocation name{
                host_, Array{view.plugin.name, view.plugin.name_len, view.plugin.name__align}};
            DescriptorAllocation contract_name{
                host_,
                Array{
                    view.plugin.contract_name,
                    view.plugin.contract_name_len,
                    view.plugin.contract_name__align,
                }};
            descriptors.push_back(RegisteredContractDescriptor{
                view.handle,
                view.bundle_id,
                view.contract_id,
                name.copy(),
                contract_name.copy(),
                view.plugin.version,
            });
        }
        if (handles.items != nullptr) {
            host_->free(host_, static_cast<uint8_t*>(handles.items),
                        handles.len * sizeof(GuestContractHandle), handles.align);
        }
        return descriptors;
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
    struct DescriptorAllocation {
        const HostApi* host;
        Array array;

        ~DescriptorAllocation() {
            if (array.items != nullptr) {
                host->free(
                    host, static_cast<uint8_t*>(array.items), array.len, array.align);
            }
        }

        std::string copy() const {
            if (array.items == nullptr || array.len == 0) {
                return {};
            }
            return std::string(
                reinterpret_cast<const char*>(array.items), array.len);
        }
    };

    struct ResidentEntry {
        uint64_t bundle_id;
        std::unique_ptr<detail::InternalPluginResident> resident;
    };

    Runtime(const HostApi* h, std::unique_ptr<detail::OnReloadFn> cb) noexcept
        : host_(h), on_reload_cb_(std::move(cb)) {}

    const RuntimeIntrospection* introspection_table() const {
        if (host_->reserved == nullptr) {
            throw std::runtime_error("runtime does not expose metadata introspection");
        }
        return static_cast<const RuntimeIntrospection*>(host_->reserved);
    }

    std::vector<uint64_t> loaded_bundle_ids() const {
        Array bundles{};
        host_->list_bundles(host_, &bundles);
        std::vector<uint64_t> ids;
        auto* items = static_cast<uint64_t*>(bundles.items);
        if (items != nullptr) {
            ids.assign(items, items + bundles.len);
            host_->free(host_, static_cast<uint8_t*>(bundles.items),
                        bundles.len * sizeof(uint64_t), bundles.align);
        }
        return ids;
    }

    void ensure_host() const {
        if (host_ == nullptr) {
            throw std::runtime_error("Runtime is destroyed");
        }
    }

    const HostApi* host_ = nullptr;  // HostApi pointer
    // Owns the on_reload functor referenced by RuntimeConfig.on_reload_user_data.
    // Must outlive the runtime so the trampoline's user_data stays valid.
    std::unique_ptr<detail::OnReloadFn> on_reload_cb_{};
    std::mutex internal_plugin_mutex_{};
    std::vector<ResidentEntry> internal_plugin_residents_{};
};

} // namespace polyplug
