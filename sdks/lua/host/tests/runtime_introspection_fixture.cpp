#include "../../../cpp/abi/polyplug/abi.hpp"

#include <algorithm>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <vector>
namespace {

std::size_t free_count = 0;
std::size_t free_sizes[16] = {};
std::size_t free_alignments[16] = {};
std::vector<uint8_t*> owned_strings{};
uint64_t bundle_ids[] = {11, 22, 33, 44};
GuestContractHandle handles[] = {{3, 7}, {5, 11}};
GuestContractHandle* find_all_handles = nullptr;
GuestContractHandle resolved_handles[2] = {};
std::size_t resolve_count = 0;
GuestContractHandle empty_handle = {};
GuestContractInterface resolved_interface = {};
void free_array(const HostApi*, uint8_t* pointer, std::size_t size, std::size_t alignment) {
    free_sizes[free_count] = size;
    free_alignments[free_count++] = alignment;
    if (pointer == reinterpret_cast<uint8_t*>(find_all_handles)) {
        std::memset(find_all_handles, 0xA5, size);
        delete[] find_all_handles;
        find_all_handles = nullptr;
        return;
    }
    const auto owned = std::find(owned_strings.begin(), owned_strings.end(), pointer);
    if (owned != owned_strings.end()) {
        delete[] pointer;
        owned_strings.erase(owned);
    }
}

Array owned_string(const char* value) {
    const std::size_t len = std::char_traits<char>::length(value);
    auto* bytes = new uint8_t[len];
    std::memcpy(bytes, value, len);
    owned_strings.push_back(bytes);
    return {bytes, len, alignof(uint8_t)};
}

void list_bundles(const HostApi*, Array* out) {
    if (out != nullptr) {
        *out = {bundle_ids, 4, alignof(uint64_t)};
    }
}

bool get_bundle_descriptor(const HostApi*, uint64_t bundle_id, BundleDescriptorView* out) {
    const std::size_t index = static_cast<std::size_t>(bundle_id / 11 - 1);
    static constexpr const char* names[] = {"internal", "path", "code", "bytes"};
    const Array name = owned_string(names[index]);
    *out = {
        bundle_id,
        name.items,
        name.len,
        name.align,
        {static_cast<uint32_t>(index + 1), static_cast<uint32_t>(index + 2), static_cast<uint32_t>(index + 3)},
        SupportedLanguage::Lua,
        static_cast<BundleSourceKind>(index),
    };
    return true;
}

void list_registered_guest_contracts(const HostApi*, Array* out) {
    if (out != nullptr) {
        *out = {handles, 2, alignof(GuestContractHandle)};
    }
}

bool get_registered_contract_descriptor(
    const HostApi*, GuestContractHandle handle, RegisteredContractDescriptorView* out
) {
    const std::size_t index = handle.index == 3 ? 0 : 1;
    static constexpr const char* names[] = {"provider-1", "provider-2"};
    static constexpr const char* contracts[] = {"example.contract.1", "example.contract.2"};
    const Array name = owned_string(names[index]);
    const Array contract_name = owned_string(contracts[index]);
    *out = {
        handle,
        bundle_ids[index],
        100 + index,
        {
            name.items,
            name.len,
            name.align,
            contract_name.items,
            contract_name.len,
            contract_name.align,
            {2, static_cast<uint32_t>(index), 9},
        },
    };
    return true;
}

void empty_registered_guest_contracts(const HostApi*, Array* out) {
    if (out != nullptr) {
        *out = {&empty_handle, 0, alignof(GuestContractHandle)};
    }
}

void find_all_guest_contracts(const HostApi*, uint64_t, uint32_t, Array* out) {
    delete[] find_all_handles;
    find_all_handles = new GuestContractHandle[2]{{17, 23}, {29, 31}};
    if (out != nullptr) {
        *out = {find_all_handles, 2, alignof(GuestContractHandle)};
    }
}

const GuestContractInterface* resolve_guest_contract(const HostApi*, GuestContractHandle handle) {
    resolved_handles[resolve_count++] = handle;
    return &resolved_interface;
}

RuntimeIntrospection full_introspection = {
    get_bundle_descriptor,
    list_registered_guest_contracts,
    get_registered_contract_descriptor,
};
RuntimeIntrospection empty_introspection = {
    get_bundle_descriptor,
    empty_registered_guest_contracts,
    get_registered_contract_descriptor,
};
HostApi host = {};

}

extern "C" const HostApi* polyplug_lua_test_runtime_introspection_host() {
    host.free = free_array;
    host.find_all_guest_contracts = find_all_guest_contracts;
    host.resolve_guest_contract = resolve_guest_contract;
    host.list_bundles = list_bundles;
    host.reserved = &full_introspection;
    return &host;
}

extern "C" void polyplug_lua_test_runtime_introspection_mode(uint32_t mode) {
    host.reserved = mode == 0 ? &full_introspection : mode == 1 ? &empty_introspection : nullptr;
}

extern "C" void polyplug_lua_test_runtime_introspection_reset() {
    delete[] find_all_handles;
    find_all_handles = nullptr;
    for (uint8_t* pointer : owned_strings) {
        delete[] pointer;
    }
    owned_strings.clear();
    free_count = 0;
    resolve_count = 0;
    for (std::size_t index = 0; index < sizeof(free_sizes) / sizeof(free_sizes[0]); ++index) {
        free_sizes[index] = 0;
        free_alignments[index] = 0;
    }
}

extern "C" std::size_t polyplug_lua_test_runtime_introspection_free_count() {
    return free_count;
}

extern "C" std::size_t polyplug_lua_test_runtime_introspection_free_size(std::size_t index) {
    return free_sizes[index];
}

extern "C" std::size_t polyplug_lua_test_runtime_introspection_free_alignment(std::size_t index) {
    return free_alignments[index];
}

extern "C" std::size_t polyplug_lua_test_runtime_introspection_resolve_count() {
    return resolve_count;
}

extern "C" uint32_t polyplug_lua_test_runtime_introspection_resolved_index(std::size_t index) {
    return resolved_handles[index].index;
}

extern "C" uint32_t polyplug_lua_test_runtime_introspection_resolved_generation(std::size_t index) {
    return resolved_handles[index].generation;
}
