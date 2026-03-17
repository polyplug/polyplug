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
