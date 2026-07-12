// C++ host SDK staged in-process registration ownership test.

#include <cstdint>
#include <cstdio>
#include <memory>
#include <stdexcept>
#include <string_view>

#include "polyplug/runtime.hpp"

namespace {

HostApi g_host{};
uint64_t g_next_bundle_id = 41;
unsigned int g_begin_calls = 0;
unsigned int g_commit_calls = 0;
unsigned int g_contract_calls = 0;
bool g_reject_begin = false;
bool g_reject_contract = false;
bool g_reject_commit = false;
bool g_reject_unload = false;
unsigned int g_destroyed_a = 0;
unsigned int g_destroyed_b = 0;

class CountingResident final : public polyplug::detail::InProcessResident {
public:
    explicit CountingResident(unsigned int& destroyed) noexcept : destroyed_(destroyed) {}
    ~CountingResident() override { ++destroyed_; }

private:
    unsigned int& destroyed_;
};

class TestBundle {
public:
    TestBundle(unsigned int& destroyed, size_t contract_count)
        : contract_count_(contract_count), resident_(std::make_unique<CountingResident>(destroyed)) {}

    std::string_view in_process_manifest() const noexcept {
        return "[manifest]\nname = \"cpp.in_process\"\nid = 1\nversion = \"1.0.0\"\nloader = \"cpp\"\nfile = \"in-process\"\n";
    }
    SupportedLanguage in_process_language() const noexcept { return SupportedLanguage::Cpp; }
    AbiError register_guest_contracts(const HostApi*) noexcept {
        g_contract_calls += static_cast<unsigned int>(contract_count_);
        return AbiError{
            static_cast<uint32_t>(g_reject_contract ? AbiErrorCode::Generic : AbiErrorCode::Ok),
            StringView{nullptr, 0},
        };
    }
    polyplug::detail::InProcessResident* in_process_resident() const noexcept { return resident_.get(); }
    std::unique_ptr<polyplug::detail::InProcessResident> take_in_process_resident() noexcept {
        return std::move(resident_);
    }

private:
    size_t contract_count_;
    std::unique_ptr<polyplug::detail::InProcessResident> resident_;
};

extern "C" const HostApi* polyplug_runtime_create(const RuntimeConfig*) { return &g_host; }
extern "C" void polyplug_runtime_destroy(const HostApi*) {}
extern "C" void polyplug_begin_in_process_bundle(
    const HostApi*, const uint8_t*, size_t, uint32_t, uint64_t* out_bundle_id, AbiError* out_error) {
    ++g_begin_calls;
    *out_bundle_id = g_reject_begin ? 0 : g_next_bundle_id++;
    *out_error = AbiError{
        static_cast<uint32_t>(g_reject_begin ? AbiErrorCode::Generic : AbiErrorCode::Ok),
        StringView{nullptr, 0},
    };
}
extern "C" void polyplug_commit_in_process_bundle(const HostApi*, uint64_t, AbiError* out_error) {
    ++g_commit_calls;
    *out_error = AbiError{
        static_cast<uint32_t>(g_reject_commit ? AbiErrorCode::Generic : AbiErrorCode::Ok),
        StringView{nullptr, 0},
    };
}
extern "C" void polyplug_abort_in_process_bundle(const HostApi*, uint64_t) {}

void unload_bundle(const HostApi*, uint64_t, AbiError* out_error) {
    *out_error = AbiError{
        static_cast<uint32_t>(g_reject_unload ? AbiErrorCode::Generic : AbiErrorCode::Ok),
        StringView{nullptr, 0},
    };
}
size_t get_error_len(const HostApi*) { return 0; }
size_t get_last_error(const HostApi*, uint8_t*, size_t) { return 0; }

int failures = 0;
void check(bool condition, const char* description) {
    if (!condition) {
        std::fprintf(stderr, "FAIL: %s\n", description);
        ++failures;
    }
}

}  // namespace

int main() {
    g_host.unload_bundle = unload_bundle;
    g_host.get_error_len = get_error_len;
    g_host.get_last_error = get_last_error;

    polyplug::Runtime first_runtime = polyplug::Runtime::builder().build();
    polyplug::Runtime second_runtime = polyplug::Runtime::builder().build();
    TestBundle first_bundle{g_destroyed_a, 2};
    const uint64_t first_id = first_runtime.register_in_process_bundle(first_bundle);
    check(first_id == 41, "begin returns a runtime-local bundle ID");
    check(g_contract_calls == 2 && g_commit_calls == 1, "all contracts stage before one commit");
    check(g_destroyed_a == 0, "successful registration transfers resident ownership");

    TestBundle rejected_bundle{g_destroyed_a, 2};
    g_reject_contract = true;
    try {
        first_runtime.register_in_process_bundle(rejected_bundle);
        check(false, "failed contract registration must throw");
    } catch (const std::runtime_error&) {
    }
    g_reject_contract = false;
    check(g_destroyed_a == 0, "failed transaction retains caller resident");

    g_reject_unload = true;
    try {
        first_runtime.unload_bundle(first_id);
        check(false, "failed logical unload must throw");
    } catch (const std::runtime_error&) {
    }
    g_reject_unload = false;
    check(g_destroyed_a == 0, "failed unload retains the resident");
    first_runtime.unload_bundle(first_id);
    check(g_destroyed_a == 1, "successful unload releases the resident");

    TestBundle second_bundle{g_destroyed_b, 1};
    const uint64_t second_id = second_runtime.register_in_process_bundle(second_bundle);
    second_runtime.unload_bundle(second_id);
    check(g_destroyed_b == 1, "each runtime releases only its own resident");
    if (failures == 0) {
        std::puts("OK: C++ staged in-process residents are transactional and Runtime-local");
        return 0;
    }
    std::fprintf(stderr, "%d check(s) failed\n", failures);
    return 1;
}
