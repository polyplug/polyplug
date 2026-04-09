// THIS FILE IS PART OF polyplug — header-only C++ binding.
// Host-side exception types for the polyplug plugin runtime.
//
// Provides PluginError (wraps AbiError) and throw_if_error() helper.

#pragma once

#include "../../abi/polyplug/abi.hpp"

#include <exception>
#include <stdexcept>
#include <string>

static_assert(POLYPLUG_ABI_VERSION == 1,
    "polyplug header version mismatch — recompile against updated headers");

namespace polyplug {

/// Exception wrapper around an AbiError.
///
/// Every ABI call that returns a non-OK AbiError can be surfaced as a
/// PluginError by calling throw_if_error(). The original AbiError is preserved
/// so callers that need to re-cross the ABI boundary can call to_abi_error().
class PluginError : public std::exception {
public:
    /// Construct from an AbiError. Copies the message string so the caller
    /// may free the AbiError message buffer immediately after construction.
    explicit PluginError(AbiError err)
        : err_(err)
        , message_{}
    {
        if (err_.message.ptr != nullptr && err_.message.len > 0) {
            message_.assign(
                reinterpret_cast<const char*>(err_.message.ptr),
                err_.message.len);
        } else {
            message_ = "(polyplug error code " + std::to_string(err_.code) + ")";
        }
    }

    /// Returns the UTF-8 error message as a null-terminated C string.
    const char* what() const noexcept override {
        return message_.c_str();
    }

    /// Returns the original AbiError value so generated wrappers can
    /// re-encode the error when returning across the C ABI boundary.
    AbiError to_abi_error() const noexcept {
        return err_;
    }

private:
    AbiError    err_;
    std::string message_;
};

/// Throws PluginError if err.code != AbiErrorCode::Ok.
/// Intended for host-side code that wants C++ exceptions rather than manual
/// AbiError checks after every ABI call.
inline void throw_if_error(AbiError err) {
    if (err.code != static_cast<uint32_t>(AbiErrorCode::Ok)) {
        throw PluginError(err);
    }
}

/// Exception thrown by generated host callers when an ABI call returns a non-zero code.
/// This is the exception type used by generated code (distinct from the hand-written PluginError).
class PolyplugException : public std::runtime_error {
public:
    explicit PolyplugException(uint32_t code, const std::string& message)
        : std::runtime_error(message), code_(code) {}

    uint32_t code() const noexcept { return code_; }

private:
    uint32_t code_;
};

/// Throw a PolyplugException if the AbiError indicates failure.
/// Used by generated host caller code after every interface dispatch.
inline void check_abi_error(AbiError err) {
    if (err.code != static_cast<uint32_t>(AbiErrorCode::Ok)) {
        const char* msg = (err.message.ptr != nullptr)
            ? reinterpret_cast<const char*>(err.message.ptr)
            : "unknown error";
        throw PolyplugException{err.code, std::string(msg, err.message.len)};
    }
}

}  // namespace polyplug