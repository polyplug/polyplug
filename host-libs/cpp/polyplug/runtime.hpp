// THIS FILE IS PART OF polyplug — header-only C++ binding.
// RAII Runtime wrapper and fluent Builder for the polyplug plugin runtime.
//
// Usage:
//   auto rt = polyplug::Runtime::builder()
//                 .plugin_dir("/usr/lib/myplugins")
//                 .compatibility(0)
//                 .build();
//   PluginHandle h  = rt.find(contract_id, min_version);
//   AbiError     err = rt.call(h, fn_id, &args, &out);

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

/// RAII wrapper around an opaque RuntimeHandle.
///
/// Owns the runtime lifetime: ~Runtime() calls polyplug_runtime_destroy().
/// Non-copyable. Move-constructible.
class Runtime {
public:
    // ── Builder ──────────────────────────────────────────────────────────────

    /// Fluent builder for Runtime construction.
    ///
    /// Call Runtime::builder() to obtain an instance, chain configuration
    /// methods, then call build() to obtain a Runtime.
    class Builder {
    public:
        /// Add a directory to scan for plugin bundles.
        /// May be called multiple times to add multiple directories.
        Builder& plugin_dir(std::string_view path) {
            plugin_dirs_.emplace_back(path);
            return *this;
        }

        /// Set the compatibility mode.
        ///   0 = Strict (MVP — the only mode currently implemented)
        Builder& compatibility(uint32_t mode) noexcept {
            compatibility_ = mode;
            return *this;
        }

        /// Construct the Runtime by calling polyplug_runtime_init.
        ///
        /// Throws std::runtime_error if the runtime cannot be initialized
        /// (e.g. polyplug_runtime_init returns null).
        Runtime build() {
            RuntimeHandle h = polyplug_runtime_new();
            if (h == nullptr) {
                throw std::runtime_error(
                    "polyplug_runtime_new returned null — "
                    "runtime initialisation failed");
            }
            return Runtime(h);
        }

    private:
        std::vector<std::string>  plugin_dirs_{};
        uint32_t                  compatibility_{0U};
    };

    // ── Static factory ────────────────────────────────────────────────────────

    /// Returns a new Builder. Equivalent to Runtime::Builder{}.
    static Builder builder() noexcept {
        return Builder{};
    }

    // ── Lifecycle ─────────────────────────────────────────────────────────────

    /// Destroys the runtime, releasing all loaded plugins and associated memory.
    ~Runtime() noexcept {
        if (handle_ != nullptr) {
            polyplug_runtime_free(handle_);
            handle_ = nullptr;
        }
    }

    /// Move-construct: transfers ownership of the handle.
    Runtime(Runtime&& other) noexcept
        : handle_(other.handle_)
    {
        other.handle_ = nullptr;
    }

    /// Move-assign: destroys current runtime then takes ownership.
    Runtime& operator=(Runtime&& other) noexcept {
        if (this != &other) {
            if (handle_ != nullptr) {
                polyplug_runtime_free(handle_);
            }
            handle_       = other.handle_;
            other.handle_ = nullptr;
        }
        return *this;
    }

    // Prevent copying — a runtime handle represents unique ownership.
    Runtime(const Runtime&)            = delete;
    Runtime& operator=(const Runtime&) = delete;

    // ── API ───────────────────────────────────────────────────────────────────

    /// Look up a plugin by contract_id and minimum encoded version.
    ///
    /// Returns a packed u64 handle (UINT64_MAX == not found).
    uint64_t find(uint64_t contract_id, uint32_t min_version) const noexcept {
        return polyplug_rt_find_by_contract(handle_, contract_id, min_version);
    }

    /// Returns the raw opaque runtime handle.
    ///
    /// Needed for loader registration functions that operate on the
    /// underlying C handle. The handle is valid for the lifetime of this Runtime.
    RuntimeHandle handle() const noexcept {
        return handle_;
    }

private:
    /// Private constructor — use builder().build() to construct.
    explicit Runtime(RuntimeHandle h) noexcept
        : handle_(h)
    {}

    RuntimeHandle handle_;
};

}  // namespace polyplug
