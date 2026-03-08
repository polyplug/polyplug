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
            // Build the StringView array that RuntimeConfig points into.
            // The strings are owned by plugin_dir_storage_ for the duration
            // of the polyplug_runtime_init call.
            dir_views_.clear();
            dir_views_.reserve(plugin_dirs_.size());
            for (const std::string& d : plugin_dirs_) {
                StringView sv;
                sv.ptr = reinterpret_cast<const uint8_t*>(d.data());
                sv.len = d.size();
                dir_views_.push_back(sv);
            }

            RuntimeConfig cfg{};
            cfg.plugin_dirs      = dir_views_.empty() ? nullptr : dir_views_.data();
            cfg.plugin_dir_count = dir_views_.size();
            cfg.compatibility    = compatibility_;
            cfg.extensions       = nullptr;
            cfg.extension_count  = 0U;

            RuntimeHandle h = polyplug_runtime_init(&cfg);
            if (h == nullptr) {
                throw std::runtime_error(
                    "polyplug_runtime_init returned null — "
                    "runtime initialisation failed");
            }
            return Runtime(h);
        }

    private:
        std::vector<std::string>  plugin_dirs_{};
        std::vector<StringView>   dir_views_{};
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
            polyplug_runtime_destroy(handle_);
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
                polyplug_runtime_destroy(handle_);
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
    /// Returns invalid_handle() (index == UINT32_MAX) when no matching plugin
    /// is loaded. Never throws.
    PluginHandle find(uint64_t contract_id, uint32_t min_version) const noexcept {
        return polyplug_find_plugin(handle_, contract_id, min_version);
    }

    /// Invoke function function_id on the plugin identified by plugin.
    ///
    /// args and out must match the contract function's expected layout.
    /// Never throws — errors are surfaced via the returned AbiError.
    AbiError call(PluginHandle plugin,
                  uint32_t     fn_id,
                  const void*  args,
                  void*        out) const noexcept
    {
        return polyplug_call_plugin(handle_, plugin, fn_id, args, out);
    }

    /// Retrieve an extension vtable by extension_id.
    ///
    /// Returns nullptr if the extension is not registered.
    const void* get_extension(uint32_t extension_id) const noexcept {
        return polyplug_get_extension(handle_, extension_id);
    }

private:
    /// Private constructor — use builder().build() to construct.
    explicit Runtime(RuntimeHandle h) noexcept
        : handle_(h)
    {}

    RuntimeHandle handle_;
};

}  // namespace polyplug
