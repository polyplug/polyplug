// THIS FILE IS PART OF polyplug — header-only C++ binding.
// Compile-time identity helpers (FNV-1a 64-bit) for bundle and contract IDs.
//
// These mirror the canonical scheme in crates/polyplug_utils exactly:
//   - fnv1a_64(bytes)                    : FNV-1a 64-bit, offset 0xcbf29ce484222325,
//                                          prime 0x100000001b3
//   - bundle_id(name)                    : fnv1a_64(name)
//   - guest_contract_id(name, major)     : fnv1a_64("guest_contract:" + name + "@" + major)
//   - host_contract_id(name, major)      : fnv1a_64("host_contract:"  + name + "@" + major)
//
// All helpers are `constexpr` so IDs can be computed at compile time. This is
// the idiomatic C++ form — no codegen is required and the values are verified
// against golden constants by the static_asserts at the end of this file.

#pragma once

#include <cstdint>
#include <string_view>

namespace polyplug {

/// FNV-1a 64-bit offset basis (matches polyplug_utils::FNV_OFFSET).
inline constexpr uint64_t kFnvOffset = 0xcbf29ce484222325ULL;

/// FNV-1a 64-bit prime (matches polyplug_utils::FNV_PRIME).
inline constexpr uint64_t kFnvPrime = 0x00000100000001b3ULL;

/// Fold one byte into a running FNV-1a 64-bit hash state.
constexpr uint64_t fnv1a_64_step(uint64_t hash, unsigned char byte) noexcept {
    hash ^= static_cast<uint64_t>(byte);
    hash *= kFnvPrime;
    return hash;
}

/// Fold every byte of `data` into a running FNV-1a 64-bit hash state.
constexpr uint64_t fnv1a_64_continue(uint64_t hash, std::string_view data) noexcept {
    for (char c : data) {
        hash = fnv1a_64_step(hash, static_cast<unsigned char>(c));
    }
    return hash;
}

/// Compute the FNV-1a 64-bit hash of `data`.
///
/// Same input always produces the same value. Matches
/// `polyplug_utils::fnv1a_64` byte-for-byte.
constexpr uint64_t fnv1a_64(std::string_view data) noexcept {
    return fnv1a_64_continue(kFnvOffset, data);
}

/// Fold a non-negative decimal integer into a running hash, most significant
/// digit first (i.e. the same byte sequence its ASCII rendering would produce).
constexpr uint64_t fnv1a_64_fold_u32(uint64_t hash, uint32_t value) noexcept {
    // Render digits high-to-low without allocating. The maximum width of a
    // uint32_t is 10 decimal digits, so a fixed-size buffer is sufficient.
    char digits[10] = {};
    uint32_t count = 0U;
    if (value == 0U) {
        return fnv1a_64_step(hash, static_cast<unsigned char>('0'));
    }
    uint32_t remaining = value;
    while (remaining != 0U) {
        digits[count] = static_cast<char>('0' + (remaining % 10U));
        remaining /= 10U;
        ++count;
    }
    for (uint32_t i = count; i > 0U; --i) {
        hash = fnv1a_64_step(hash, static_cast<unsigned char>(digits[i - 1U]));
    }
    return hash;
}

/// Compute a bundle ID from its name.
///
/// `bundle_id(name) == fnv1a_64(name)`.
constexpr uint64_t bundle_id(std::string_view name) noexcept {
    return fnv1a_64(name);
}

/// Compute a contract ID using the given canonical prefix.
///
/// `contract_id(prefix, name, major) == fnv1a_64(prefix + name + "@" + major)`.
constexpr uint64_t contract_id(std::string_view prefix,
                               std::string_view name,
                               uint32_t major_version) noexcept {
    uint64_t hash = kFnvOffset;
    hash = fnv1a_64_continue(hash, prefix);
    hash = fnv1a_64_continue(hash, name);
    hash = fnv1a_64_step(hash, static_cast<unsigned char>('@'));
    hash = fnv1a_64_fold_u32(hash, major_version);
    return hash;
}

/// Compute a guest contract ID from name and major version.
///
/// Uses the `"guest_contract:"` prefix so guest IDs never collide with host IDs.
/// Matches `polyplug_utils::guest_contract_id`.
constexpr uint64_t guest_contract_id(std::string_view name, uint32_t major_version) noexcept {
    return contract_id("guest_contract:", name, major_version);
}

/// Compute a host contract ID from name and major version.
///
/// Uses the `"host_contract:"` prefix so host IDs never collide with guest IDs.
/// Matches `polyplug_utils::host_contract_id`.
constexpr uint64_t host_contract_id(std::string_view name, uint32_t major_version) noexcept {
    return contract_id("host_contract:", name, major_version);
}

// ─── Golden-value checks (must match crates/polyplug_utils) ───────────────────

static_assert(fnv1a_64("") == 0xcbf29ce484222325ULL,
    "FNV-1a of empty input must equal the offset basis");
static_assert(fnv1a_64("image.decode@1") == 0xa1ba05dd7da18569ULL,
    "FNV-1a golden value mismatch for \"image.decode@1\"");
static_assert(guest_contract_id("test.add", 1) == 0x40244df59fcbecb6ULL,
    "guest_contract_id golden value mismatch for (\"test.add\", 1)");
static_assert(guest_contract_id("logger", 1) == fnv1a_64("guest_contract:logger@1"),
    "guest_contract_id must equal fnv1a_64 of its canonical string form");
static_assert(host_contract_id("logger", 1) == fnv1a_64("host_contract:logger@1"),
    "host_contract_id must equal fnv1a_64 of its canonical string form");
static_assert(host_contract_id("logger", 1) != guest_contract_id("logger", 1),
    "host and guest contract IDs must never collide");
static_assert(bundle_id("my-bundle") == 0xfe6226876e3a35b2ULL,
    "bundle_id golden value mismatch for \"my-bundle\"");

}  // namespace polyplug
