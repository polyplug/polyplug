// C++ host SDK in-process registration ownership test.
// Uses a deterministic HostApi table to exercise the Runtime transaction around
// canonical InProcessBundleRegistration without requiring a dynamically loaded plugin.

#include <cstdint>
#include <cstdio>
#include <memory>
#include <stdexcept>

#include "polyplug/runtime.hpp"

namespace {

HostApi g_host{};
uint64_t g_next_bundle_id = 41;
unsigned int g_registration_calls = 0;
bool g_reject_registration = false;
bool g_reject_unload = false;
unsigned int g_destroyed_a = 0;
unsigned int g_destroyed_b = 0;
unsigned int g_seen_contract_count = 0;

class CountingResident final : public polyplug::detail::InProcessResident {
public:
    explicit CountingResident(unsigned int& destroyed) noexcept : destroyed_(destroyed) {}

    ~CountingResident() override {
        ++destroyed_;
    }

private:
    unsigned int& destroyed_;
};

class TestBundle {
public:
    TestBundle(unsigned int& destroyed, size_t contract_count)
        : resident_(std::make_unique<CountingResident>(destroyed)) {
        registration_.metadata = InProcessBundleMetadata{
            StringView{reinterpret_cast<const uint8_t*>("cpp.in_process"), 14},
            Version{1, 0, 0},
            SupportedLanguage::Cpp,
        };
        registration_.contracts = contracts_;
        registration_.contract_count = contract_count;
    }

    const InProcessBundleRegistration& in_process_registration() const noexcept {
        return registration_;
    }

    polyplug::detail::InProcessResident* in_process_resident() const noexcept {
        return resident_.get();
    }

    std::unique_ptr<polyplug::detail::InProcessResident> take_in_process_resident() noexcept {
        return std::move(resident_);
    }

private:
    InProcessContractRegistration contracts_[2]{};
    InProcessBundleRegistration registration_{};
    std::unique_ptr<polyplug::detail::InProcessResident> resident_{};
};

extern "C" const HostApi* polyplug_runtime_create(const RuntimeConfig*) {
    return &g_host;
}

extern "C" void polyplug_runtime_destroy(const HostApi*) {}

void register_in_process_bundle(
    const HostApi*,
    const InProcessBundleRegistration* registration,
    uint64_t* out_bundle_id,
    AbiError* out_error) {
    ++g_registration_calls;
    g_seen_contract_count = registration == nullptr ? 0U : static_cast<unsigned int>(registration->contract_count);
    if (g_reject_registration) {
        *out_bundle_id = 0;
        *out_error = AbiError{static_cast<uint32_t>(AbiErrorCode::Generic), StringView{nullptr, 0}};
        return;
    }
    *out_bundle_id = g_next_bundle_id++;
    *out_error = AbiError{static_cast<uint32_t>(AbiErrorCode::Ok), StringView{nullptr, 0}};
}

void unload_bundle(const HostApi*, uint64_t, AbiError* out_error) {
    const AbiErrorCode code = g_reject_unload ? AbiErrorCode::Generic : AbiErrorCode::Ok;
    *out_error = AbiError{static_cast<uint32_t>(code), StringView{nullptr, 0}};
}

size_t get_error_len(const HostApi*) {
    return 0;
}

size_t get_last_error(const HostApi*, uint8_t*, size_t) {
    return 0;
}

int failures = 0;

void check(bool condition, const char* description) {
    if (!condition) {
        std::fprintf(stderr, "FAIL: %s\n", description);
        ++failures;
    }
}

}  // namespace

int main() {
    g_host.register_in_process_bundle = register_in_process_bundle;
    g_host.unload_bundle = unload_bundle;
    g_host.get_error_len = get_error_len;
    g_host.get_last_error = get_last_error;

    polyplug::Runtime first_runtime = polyplug::Runtime::builder().build();
    polyplug::Runtime second_runtime = polyplug::Runtime::builder().build();

    TestBundle first_bundle{g_destroyed_a, 2};
    const uint64_t first_id = first_runtime.register_in_process_bundle(first_bundle);
    check(first_id == 41, "first registration returns the host-assigned bundle id");
    check(g_seen_contract_count == 2, "complete multi-contract registration reaches HostApi once");
    check(g_destroyed_a == 0, "successful registration transfers resident ownership to Runtime");

    try {
        first_runtime.register_in_process_bundle(first_bundle);
        check(false, "a transferred bundle must not register a second time");
    } catch (const std::runtime_error&) {
    }
    check(g_registration_calls == 1, "transferred bundle is rejected before the canonical ABI call");
    check(g_destroyed_a == 0, "rejected transferred bundle leaves the Runtime resident intact");

    TestBundle duplicate_bundle{g_destroyed_a, 2};
    g_reject_registration = true;
    try {
        first_runtime.register_in_process_bundle(duplicate_bundle);
        check(false, "duplicate registration must throw");
    } catch (const std::runtime_error&) {
    }
    g_reject_registration = false;
    check(g_registration_calls == 2, "duplicate registration attempts exactly one canonical ABI call");
    check(g_destroyed_a == 0, "failed duplicate registration retains the caller resident");

    g_reject_unload = true;
    try {
        first_runtime.unload_bundle(first_id);
        check(false, "failed logical unload must throw");
    } catch (const std::runtime_error&) {
    }
    g_reject_unload = false;
    check(g_destroyed_a == 0, "failed unload retains the Runtime resident");

    first_runtime.unload_bundle(first_id);
    check(g_destroyed_a == 1, "successful logical unload releases exactly one resident");

    const uint64_t replacement_id = first_runtime.register_in_process_bundle(duplicate_bundle);
    check(replacement_id == 42, "bundle can register again after successful unload");
    check(g_destroyed_a == 1, "re-registration installs a fresh resident");

    TestBundle second_bundle{g_destroyed_b, 2};
    const uint64_t second_id = second_runtime.register_in_process_bundle(second_bundle);
    check(second_id == 43, "second Runtime receives an independent registration");
    first_runtime.unload_bundle(replacement_id);
    check(g_destroyed_a == 2, "first Runtime unload does not retain its replacement resident");
    check(g_destroyed_b == 0, "first Runtime unload cannot release the second Runtime resident");
    second_runtime.unload_bundle(second_id);
    check(g_destroyed_b == 1, "second Runtime releases only its own resident");

    if (failures == 0) {
        std::puts("OK: C++ in-process residents are transactional and Runtime-local");
        return 0;
    }
    std::fprintf(stderr, "%d check(s) failed\n", failures);
    return 1;
}
