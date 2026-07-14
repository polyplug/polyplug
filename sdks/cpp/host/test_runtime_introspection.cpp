// C++ host SDK metadata-introspection snapshot test.

#include <algorithm>
#include <array>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <memory>
#include <stdexcept>
#include <string_view>
#include <utility>
#include <vector>

#include "polyplug/runtime.hpp"

namespace {

enum class IntrospectionMode {
    Populated,
    NullAndEmpty,
    CallbackFalse,
    Empty,
    OlderRuntime,
};

struct NativeAllocation {
    std::unique_ptr<uint8_t[]> storage;
    bool is_string;
};

HostApi g_host{};
RuntimeIntrospection g_introspection{};
IntrospectionMode g_mode = IntrospectionMode::Populated;
std::vector<NativeAllocation> g_allocations{};
std::vector<std::pair<size_t, size_t>> g_free_calls{};
size_t g_poisoned_string_frees = 0;
size_t g_false_bundle_callbacks = 0;
size_t g_false_contract_callbacks = 0;
int failures = 0;

void check(bool condition, const char* description) {
    if (!condition) {
        std::fprintf(stderr, "FAIL: %s\n", description);
        ++failures;
    }
}


Version version() {
    return Version{1, 2, 3};
}

template <typename T>
Array allocate_array(const T* values, size_t len, size_t alignment, bool is_string = false) {
    const size_t bytes = len * sizeof(T);
    auto storage = std::make_unique<uint8_t[]>(bytes == 0 ? 1 : bytes);
    if (bytes != 0) {
        std::memcpy(storage.get(), values, bytes);
    }
    uint8_t* items = storage.get();
    g_allocations.push_back(NativeAllocation{std::move(storage), is_string});
    return Array{items, len, alignment};
}

Array owned_string(const char* value) {
    return allocate_array(
        reinterpret_cast<const uint8_t*>(value), std::strlen(value), alignof(uint8_t), true);
}

void list_bundles(const HostApi*, Array* out) {
    if (out == nullptr) {
        return;
    }
    if (g_mode == IntrospectionMode::Populated) {
        constexpr std::array<uint64_t, 4> ids{10, 20, 30, 40};
        *out = allocate_array(ids.data(), ids.size(), alignof(uint64_t));
        return;
    }
    if (g_mode == IntrospectionMode::NullAndEmpty) {
        constexpr std::array<uint64_t, 2> ids{50, 60};
        *out = allocate_array(ids.data(), ids.size(), alignof(uint64_t));
        return;
    }
    if (g_mode == IntrospectionMode::CallbackFalse) {
        constexpr std::array<uint64_t, 1> ids{70};
        *out = allocate_array(ids.data(), ids.size(), alignof(uint64_t));
        return;
    }
    *out = allocate_array<uint64_t>(nullptr, 0, alignof(uint64_t));
}

bool get_bundle_descriptor(const HostApi*, uint64_t bundle_id, BundleDescriptorView* out) {
    if (out == nullptr) {
        return false;
    }
    if (g_mode == IntrospectionMode::CallbackFalse) {
        ++g_false_bundle_callbacks;
        static char borrowed_name[] = "borrowed bundle";
        *out = BundleDescriptorView{
            bundle_id,
            borrowed_name,
            std::strlen(borrowed_name),
            alignof(uint8_t),
            version(),
            SupportedLanguage::Cpp,
            BundleSourceKind::Internal,
        };
        return false;
    }
    if (g_mode == IntrospectionMode::NullAndEmpty) {
        if (bundle_id == 50) {
            *out = BundleDescriptorView{
                bundle_id,
                nullptr,
                0,
                alignof(uint8_t),
                version(),
                SupportedLanguage::Cpp,
                BundleSourceKind::Internal,
            };
            return true;
        }
        if (bundle_id == 60) {
            const Array name = owned_string("");
            *out = BundleDescriptorView{
                bundle_id,
                name.items,
                name.len,
                name.align,
                version(),
                SupportedLanguage::Cpp,
                BundleSourceKind::Path,
            };
            return true;
        }
        return false;
    }
    if (g_mode != IntrospectionMode::Populated) {
        return false;
    }
    const char* name = nullptr;
    BundleSourceKind source = BundleSourceKind::Internal;
    switch (bundle_id) {
    case 10:
        name = "internal";
        source = BundleSourceKind::Internal;
        break;
    case 20:
        name = "path";
        source = BundleSourceKind::Path;
        break;
    case 30:
        name = "code";
        source = BundleSourceKind::Code;
        break;
    case 40:
        name = "bytes";
        source = BundleSourceKind::Bytes;
        break;
    default:
        return false;
    }
    const Array owned_name = owned_string(name);
    *out = BundleDescriptorView{
        bundle_id,
        owned_name.items,
        owned_name.len,
        owned_name.align,
        version(),
        SupportedLanguage::Cpp,
        source,
    };
    return true;
}

void list_registered_guest_contracts(const HostApi*, Array* out) {
    if (out == nullptr) {
        return;
    }
    if (g_mode == IntrospectionMode::Populated) {
        constexpr std::array<GuestContractHandle, 2> handles{{{1, 7}, {2, 8}}};
        *out = allocate_array(handles.data(), handles.size(), alignof(GuestContractHandle));
        return;
    }
    if (g_mode == IntrospectionMode::NullAndEmpty) {
        constexpr std::array<GuestContractHandle, 1> handles{{{3, 9}}};
        *out = allocate_array(handles.data(), handles.size(), alignof(GuestContractHandle));
        return;
    }
    if (g_mode == IntrospectionMode::CallbackFalse) {
        constexpr std::array<GuestContractHandle, 1> handles{{{4, 10}}};
        *out = allocate_array(handles.data(), handles.size(), alignof(GuestContractHandle));
        return;
    }
    *out = allocate_array<GuestContractHandle>(nullptr, 0, alignof(GuestContractHandle));
}

bool get_registered_contract_descriptor(
    const HostApi*, GuestContractHandle handle, RegisteredContractDescriptorView* out) {
    if (out == nullptr) {
        return false;
    }
    if (g_mode == IntrospectionMode::CallbackFalse) {
        ++g_false_contract_callbacks;
        static char borrowed_plugin_name[] = "borrowed plugin";
        static char borrowed_contract_name[] = "borrowed.contract";
        *out = RegisteredContractDescriptorView{
            handle,
            70,
            701,
            OwnedPluginDescriptorView{
                borrowed_plugin_name,
                std::strlen(borrowed_plugin_name),
                alignof(uint8_t),
                borrowed_contract_name,
                std::strlen(borrowed_contract_name),
                alignof(uint8_t),
                version(),
            },
        };
        return false;
    }
    if (g_mode == IntrospectionMode::NullAndEmpty) {
        if (handle.index != 3) {
            return false;
        }
        const Array empty_contract_name = owned_string("");
        *out = RegisteredContractDescriptorView{
            handle,
            50,
            501,
            OwnedPluginDescriptorView{
                nullptr,
                0,
                alignof(uint8_t),
                empty_contract_name.items,
                empty_contract_name.len,
                empty_contract_name.align,
                version(),
            },
        };
        return true;
    }
    if (g_mode != IntrospectionMode::Populated) {
        return false;
    }
    const char* plugin_name = nullptr;
    const char* contract_name = nullptr;
    uint64_t bundle_id = 0;
    uint64_t contract_id = 0;
    switch (handle.index) {
    case 1:
        plugin_name = "alpha";
        contract_name = "example.alpha";
        bundle_id = 10;
        contract_id = 101;
        break;
    case 2:
        plugin_name = "beta";
        contract_name = "example.beta";
        bundle_id = 20;
        contract_id = 102;
        break;
    default:
        return false;
    }
    const Array owned_plugin_name = owned_string(plugin_name);
    const Array owned_contract_name = owned_string(contract_name);
    *out = RegisteredContractDescriptorView{
        handle,
        bundle_id,
        contract_id,
        OwnedPluginDescriptorView{
            owned_plugin_name.items,
            owned_plugin_name.len,
            owned_plugin_name.align,
            owned_contract_name.items,
            owned_contract_name.len,
            owned_contract_name.align,
            version(),
        },
    };
    return true;
}

void free_array(const HostApi*, uint8_t* pointer, size_t size, size_t alignment) {
    g_free_calls.emplace_back(size, alignment);
    const auto found = std::find_if(
        g_allocations.begin(), g_allocations.end(),
        [pointer](const NativeAllocation& allocation) { return allocation.storage.get() == pointer; });
    if (found == g_allocations.end()) {
        check(false, "SDK must free each native temporary array exactly once");
        return;
    }
    std::memset(pointer, 0xA5, size == 0 ? 1 : size);
    if (found->is_string) {
        ++g_poisoned_string_frees;
    }
    g_allocations.erase(found);
}

extern "C" const HostApi* polyplug_runtime_create(const RuntimeConfig*) {
    return &g_host;
}

extern "C" bool polyplug_runtime_destroy(const HostApi*) {
    return true;
}

void assert_free_calls(std::initializer_list<std::pair<size_t, size_t>> expected) {
    check(g_free_calls.size() == expected.size(), "every temporary ABI array is freed once");
    const size_t count = std::min(g_free_calls.size(), expected.size());
    auto expected_it = expected.begin();
    for (size_t index = 0; index < count; ++index, ++expected_it) {
        check(g_free_calls[index] == *expected_it, "free uses the exact ABI size and alignment");
    }
    check(g_allocations.empty(), "no native temporary array survives the snapshot call");
}

void reset(IntrospectionMode mode) {
    g_mode = mode;
    g_free_calls.clear();
    g_allocations.clear();
    g_poisoned_string_frees = 0;
    g_false_bundle_callbacks = 0;
    g_false_contract_callbacks = 0;
    g_host = HostApi{};
    g_host.list_bundles = list_bundles;
    g_host.free = free_array;
    g_introspection = RuntimeIntrospection{
        get_bundle_descriptor,
        list_registered_guest_contracts,
        get_registered_contract_descriptor,
    };
    g_host.reserved = mode == IntrospectionMode::OlderRuntime ? nullptr : &g_introspection;
}

}  // namespace

int main() {
    reset(IntrospectionMode::Populated);
    polyplug::Runtime populated = polyplug::Runtime::builder().build();
    const std::vector<polyplug::LoadedBundleDescriptor> bundles = populated.bundle_descriptors();
    check(bundles.size() == 4, "all four canonical source kinds are listed");
    constexpr std::array<BundleSourceKind, 4> expected_sources{
        BundleSourceKind::Internal,
        BundleSourceKind::Path,
        BundleSourceKind::Code,
        BundleSourceKind::Bytes,
    };
    for (size_t index = 0; index < bundles.size() && index < expected_sources.size(); ++index) {
        check(bundles[index].id == (index + 1) * 10, "bundle identity is copied from its ABI view");
        check(bundles[index].source_kind == expected_sources[index], "bundle origin is metadata-only kind");
        check(bundles[index].runtime == SupportedLanguage::Cpp, "bundle runtime is copied");
    }
    check(bundles.size() < 4 || bundles[2].name == "code", "code origin exposes metadata, not artifact payload");
    check(bundles.size() < 4 || bundles[3].name == "bytes", "bytes origin exposes metadata, not artifact payload");
    const std::vector<polyplug::RegisteredContractDescriptor> contracts =
        populated.registered_contract_descriptors();
    check(contracts.size() == 2, "all registered contracts are listed");
    check(
        contracts.size() < 2
            || (contracts[0].bundle_id == 10 && contracts[0].contract_id == 101
                && contracts[0].plugin_name == "alpha" && contracts[1].bundle_id == 20
                && contracts[1].contract_id == 102 && contracts[1].plugin_name == "beta"),
        "registered contracts retain their owning bundles and copied descriptors");
    assert_free_calls({
        {4 * sizeof(uint64_t), alignof(uint64_t)},
        {std::strlen("internal"), alignof(uint8_t)},
        {std::strlen("path"), alignof(uint8_t)},
        {std::strlen("code"), alignof(uint8_t)},
        {std::strlen("bytes"), alignof(uint8_t)},
        {std::strlen("example.alpha"), alignof(uint8_t)},
        {std::strlen("alpha"), alignof(uint8_t)},
        {std::strlen("example.beta"), alignof(uint8_t)},
        {std::strlen("beta"), alignof(uint8_t)},
        {2 * sizeof(GuestContractHandle), alignof(GuestContractHandle)},
    });
    check(g_poisoned_string_frees == 8, "every owned native string is poisoned before release");
    check(bundles[1].name == "path", "bundle snapshot remains valid after native array free");
    check(contracts[1].contract_name == "example.beta", "contract snapshot remains valid after native array free");

    reset(IntrospectionMode::NullAndEmpty);
    polyplug::Runtime null_and_empty = polyplug::Runtime::builder().build();
    const std::vector<polyplug::LoadedBundleDescriptor> null_and_empty_bundles =
        null_and_empty.bundle_descriptors();
    const std::vector<polyplug::RegisteredContractDescriptor> null_and_empty_contracts =
        null_and_empty.registered_contract_descriptors();
    check(
        null_and_empty_bundles.size() == 2 && null_and_empty_bundles[0].name.empty()
            && null_and_empty_bundles[1].name.empty(),
        "null and allocated-empty bundle names become empty snapshots");
    check(
        null_and_empty_contracts.size() == 1 && null_and_empty_contracts[0].plugin_name.empty()
            && null_and_empty_contracts[0].contract_name.empty(),
        "null and allocated-empty contract names become empty snapshots");
    assert_free_calls({
        {2 * sizeof(uint64_t), alignof(uint64_t)},
        {0, alignof(uint8_t)},
        {0, alignof(uint8_t)},
        {sizeof(GuestContractHandle), alignof(GuestContractHandle)},
    });
    check(g_poisoned_string_frees == 2, "allocated empty strings are poisoned and freed exactly once");

    reset(IntrospectionMode::CallbackFalse);
    polyplug::Runtime callback_false = polyplug::Runtime::builder().build();
    check(
        callback_false.bundle_descriptors().empty(),
        "a false bundle descriptor callback transfers no descriptor ownership");
    check(
        callback_false.registered_contract_descriptors().empty(),
        "a false contract descriptor callback transfers no descriptor ownership");
    check(
        g_false_bundle_callbacks == 1 && g_false_contract_callbacks == 1,
        "false descriptor callbacks are invoked before their outputs are ignored");
    assert_free_calls({
        {sizeof(uint64_t), alignof(uint64_t)},
        {sizeof(GuestContractHandle), alignof(GuestContractHandle)},
    });
    check(g_poisoned_string_frees == 0, "false callbacks do not transfer borrowed strings for freeing");

    reset(IntrospectionMode::Empty);
    polyplug::Runtime empty = polyplug::Runtime::builder().build();
    check(empty.bundle_descriptors().empty(), "current runtime empty bundle result is an empty snapshot");
    check(empty.registered_contract_descriptors().empty(), "current runtime empty contract result is an empty snapshot");
    assert_free_calls({
        {0, alignof(uint64_t)},
        {0, alignof(GuestContractHandle)},
    });

    reset(IntrospectionMode::OlderRuntime);
    polyplug::Runtime older = polyplug::Runtime::builder().build();
    check(older.bundle_descriptors().empty(), "legacy runtime without introspection returns empty bundles");
    check(older.registered_contract_descriptors().empty(), "legacy runtime without introspection returns empty contracts");
    check(g_free_calls.empty(), "legacy runtime does not request unavailable ABI arrays");

    if (failures == 0) {
        std::puts("OK: C++ descriptor snapshots copy metadata and free temporary arrays");
        return 0;
    }
    std::fprintf(stderr, "%d check(s) failed\n", failures);
    return 1;
}
