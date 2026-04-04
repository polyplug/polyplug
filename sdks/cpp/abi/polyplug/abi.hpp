#pragma once
#include <cstdint>
#include <cstddef>

#define POLYPLUG_ABI_VERSION 1U
constexpr uint64_t fnv1a_64(&[u8] data) { /* implementation */ }

constexpr uint64_t contract_id(&str name, uint32_t major) { /* implementation */ }

constexpr uint64_t bundle_id(&str name) { /* implementation */ }

constexpr uint64_t host_contract_id(&str name, uint32_t major) { /* implementation */ }

constexpr uint64_t plugin_contract_id(&str name, uint32_t major) { /* implementation */ }

