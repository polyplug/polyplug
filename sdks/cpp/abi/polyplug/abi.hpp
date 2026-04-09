#pragma once
#include <cstdint>
#include <cstddef>

/// ABI version sentinel.
#define POLYPLUG_ABI_VERSION 1U

/// ABI error codes — returned by all ABI functions.
enum class AbiErrorCode : uint32_t {
    Ok = 0,
    Generic = 1,
    BufferTooSmall = 2,
    Panic = 3,
    NotFound = 4,
    StaleHandle = 5,
    FunctionNotAvailable = 6,
    DuplicateProvider = 7,
    InvalidPointer = 8,
    HostContractNotFound = 100,
    HostContractVersionMismatch = 101,
    HostContractCallFailed = 102,
};

// FNV-1a hash constants
constexpr uint64_t FNV_OFFSET = 0xcbf29ce484222325ULL;
constexpr uint64_t FNV_PRIME = 0x00000100000001B3ULL;

/// Compute FNV-1a 64-bit hash of byte data.
inline uint64_t fnv1a_64(const uint8_t* data, size_t len) {
    uint64_t h = FNV_OFFSET;
    for (size_t i = 0; i < len; ++i) {
        h ^= data[i];
        h *= FNV_PRIME;
    }
    return h;
}

/// Compute FNV-1a 64-bit hash of a string.
inline uint64_t fnv1a_64_str(const char* str) {
    uint64_t h = FNV_OFFSET;
    while (*str) {
        h ^= static_cast<uint8_t>(*str);
        h *= FNV_PRIME;
        ++str;
    }
    return h;
}

/// Compute contract ID for "name@major" using FNV-1a 64-bit.
inline uint64_t contract_id(const char* name, uint32_t major) {
    char buf[256];
    snprintf(buf, sizeof(buf), "%s@%u", name, major);
    return fnv1a_64_str(buf);
}

/// Compute bundle ID from name using FNV-1a 64-bit.
inline uint64_t bundle_id(const char* name) {
    return fnv1a_64_str(name);
}

/// Compute host contract ID from name and major version.
inline uint64_t host_contract_id(const char* name, uint32_t major) {
    char buf[256];
    snprintf(buf, sizeof(buf), "host_contract:%s@%u", name, major);
    return fnv1a_64_str(buf);
}

/// Compute guest contract ID from name and major version.
inline uint64_t guest_contract_id(const char* name, uint32_t major) {
    char buf[256];
    snprintf(buf, sizeof(buf), "guest_contract:%s@%u", name, major);
    return fnv1a_64_str(buf);
}
