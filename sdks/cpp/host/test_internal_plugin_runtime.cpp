// C++ host SDK internal-plugin registration ownership test.

#include <cstdint>
#include <cstdio>
#include <memory>
#include <limits>
#include <stdexcept>
#include <string_view>
#include <vector>

#include "polyplug/runtime.hpp"

namespace {

HostApi g_host{};
uint64_t g_next_bundle_id = 41;
unsigned int g_begin_calls = 0;
unsigned int g_commit_calls = 0;
unsigned int g_abort_calls = 0;
unsigned int g_contract_calls = 0;
bool g_reject_begin = false;
bool g_reject_contract = false;
bool g_reject_commit = false;
bool g_reject_unload = false;
bool g_destroy_succeeds = true;
unsigned int g_destroyed_a = 0;
unsigned int g_destroyed_b = 0;

class CountingResident final : public polyplug::detail::InternalPluginResident {
public:
    explicit CountingResident(unsigned int& destroyed) noexcept : destroyed_(destroyed) {}
    ~CountingResident() override { ++destroyed_; }

private:
    unsigned int& destroyed_;
};

class TestInternalPlugin {
public:
    TestInternalPlugin(unsigned int& destroyed, size_t provider_count)
        : provider_count_(provider_count), resident_(std::make_unique<CountingResident>(destroyed)) {}

    std::string_view internal_plugin_manifest() const noexcept {
        return "[manifest]\nname = \"cpp.internal_plugin\"\nid = 1\nversion = \"1.0.0\"\nloader = \"cpp\"\nfile = \"internal-plugin\"\n";
    }
    SupportedLanguage internal_plugin_language() const noexcept { return SupportedLanguage::Cpp; }
    size_t internal_plugin_provider_count() const noexcept { return provider_count_; }
    AbiError register_guest_contracts(const HostApi*) noexcept {
        g_contract_calls += static_cast<unsigned int>(provider_count_);
        return AbiError{
            static_cast<uint32_t>(g_reject_contract ? AbiErrorCode::Generic : AbiErrorCode::Ok),
            StringView{nullptr, 0},
        };
    }
    polyplug::detail::InternalPluginResident* internal_plugin_resident() const noexcept {
        return resident_.get();
    }
    std::unique_ptr<polyplug::detail::InternalPluginResident>
    take_internal_plugin_resident() noexcept {
        return std::move(resident_);
    }

private:
    size_t provider_count_;
    std::unique_ptr<polyplug::detail::InternalPluginResident> resident_;
};

extern "C" const HostApi* polyplug_runtime_create(const RuntimeConfig*) { return &g_host; }
extern "C" bool polyplug_runtime_destroy(const HostApi*) { return g_destroy_succeeds; }
extern "C" void polyplug_begin_internal_plugin(
    const HostApi*, const uint8_t*, size_t, uint32_t, uint64_t* out_bundle_id, AbiError* out_error) {
    ++g_begin_calls;
    *out_bundle_id = g_reject_begin ? 0 : g_next_bundle_id++;
    *out_error = AbiError{
        static_cast<uint32_t>(g_reject_begin ? AbiErrorCode::Generic : AbiErrorCode::Ok),
        StringView{nullptr, 0},
    };
}
extern "C" void polyplug_commit_internal_plugin_with_handles(
    const HostApi*,
    uint64_t,
    GuestContractHandle* out_handles,
    size_t handle_capacity,
    size_t* out_handle_count,
    AbiError* out_error) {
    ++g_commit_calls;
    for (size_t index = 0; index < handle_capacity; ++index) {
        out_handles[index] = GuestContractHandle{static_cast<uint32_t>(index), 1U};
    }
    *out_handle_count = handle_capacity;
    *out_error = AbiError{
        static_cast<uint32_t>(g_reject_commit ? AbiErrorCode::Generic : AbiErrorCode::Ok),
        StringView{nullptr, 0},
    };
}
extern "C" void polyplug_abort_internal_plugin(const HostApi*, uint64_t) { ++g_abort_calls; }

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
    TestInternalPlugin first_internal_plugin{g_destroyed_a, 2};
    const polyplug::InternalPluginCommit first_commit =
        first_runtime.register_internal_plugin_with_handles(first_internal_plugin);
    check(first_commit.bundle_id == 41, "begin returns a runtime-local bundle ID");
    check(
        first_commit.handles.size() == 2U,
        "committing providers returns their exact handles");
    check(g_contract_calls == 2 && g_commit_calls == 1, "all providers stage before one commit");
    check(g_destroyed_a == 0, "successful registration transfers resident ownership");

    TestInternalPlugin rejected_internal_plugin{g_destroyed_a, 2};
    g_reject_contract = true;
    try {
        first_runtime.register_internal_plugin_with_handles(rejected_internal_plugin);
        check(false, "failed contract registration must throw");
    } catch (const std::runtime_error&) {
    }
    g_reject_contract = false;
    check(g_abort_calls == 1, "provider registration failure aborts its transaction once");
    check(g_destroyed_a == 0, "failed transaction retains caller resident");
    const unsigned int commits_before_allocation_failure = g_commit_calls;
    const unsigned int aborts_before_allocation_failure = g_abort_calls;
    TestInternalPlugin allocation_failure{
        g_destroyed_a, std::numeric_limits<size_t>::max()};
    try {
        first_runtime.register_internal_plugin_with_handles(allocation_failure);
        check(false, "impossible provider count must throw before commit");
    } catch (const std::length_error&) {
    }
    check(
        g_abort_calls == aborts_before_allocation_failure + 1U,
        "precommit allocation failure aborts its transaction once");
    check(
        g_commit_calls == commits_before_allocation_failure,
        "precommit allocation failure does not attempt commit");
    check(
        allocation_failure.internal_plugin_resident() != nullptr,
        "precommit allocation failure retains caller resident ownership");

    TestInternalPlugin commit_rejected{g_destroyed_a, 1};
    const unsigned int aborts_before_commit_failure = g_abort_calls;
    const unsigned int commits_before_commit_failure = g_commit_calls;
    g_reject_commit = true;
    try {
        first_runtime.register_internal_plugin_with_handles(commit_rejected);
        check(false, "commit rejection must throw");
    } catch (const std::runtime_error&) {
    }
    g_reject_commit = false;
    check(
        g_abort_calls == aborts_before_commit_failure,
        "commit rejection consumes the transaction without aborting");
    check(
        g_commit_calls == commits_before_commit_failure + 1U,
        "commit rejection attempts commit exactly once");
    check(
        commit_rejected.internal_plugin_resident() != nullptr,
        "commit rejection retains caller resident ownership");

    TestInternalPlugin fresh_registration{g_destroyed_a, 1};
    const polyplug::InternalPluginCommit fresh_commit =
        first_runtime.register_internal_plugin_with_handles(fresh_registration);
    check(
        fresh_commit.handles.size() == 1U,
        "fresh registration succeeds after failed transactions");

    g_reject_unload = true;
    try {
        first_runtime.unload_bundle(first_commit.bundle_id);
        check(false, "failed logical unload must throw");
    } catch (const std::runtime_error&) {
    }
    g_reject_unload = false;
    check(g_destroyed_a == 0, "failed unload retains the resident");
    first_runtime.unload_bundle(first_commit.bundle_id);
    check(g_destroyed_a == 1, "successful unload releases the resident");

    TestInternalPlugin second_internal_plugin{g_destroyed_b, 1};
    const polyplug::InternalPluginCommit second_commit =
        second_runtime.register_internal_plugin_with_handles(second_internal_plugin);
    second_runtime.unload_bundle(second_commit.bundle_id);
    check(g_destroyed_b == 1, "each runtime releases only its own resident");

    polyplug::Runtime retry_runtime = polyplug::Runtime::builder().build();
    g_destroy_succeeds = false;
    check(!retry_runtime.destroy(), "failed destroy must remain retryable");
    check(retry_runtime.host() != nullptr, "failed destroy must retain the HostApi handle");
    g_destroy_succeeds = true;
    check(retry_runtime.destroy(), "owner retry must consume the runtime");
    check(retry_runtime.host() == nullptr, "successful destroy must clear the HostApi handle");

    const auto unload_reports_destroyed = [](polyplug::Runtime& runtime) {
        try {
            runtime.unload_bundle(0);
            return false;
        } catch (const std::runtime_error& error) {
            return std::string_view(error.what()) == "Runtime is destroyed";
        }
    };
    polyplug::Runtime explicitly_destroyed = polyplug::Runtime::builder().build();
    check(explicitly_destroyed.destroy(), "explicit destroy must consume its runtime");
    check(
        unload_reports_destroyed(explicitly_destroyed),
        "explicitly destroyed runtime unload reports Runtime is destroyed");

    polyplug::Runtime source_runtime = polyplug::Runtime::builder().build();
    polyplug::Runtime moved_runtime = std::move(source_runtime);
    check(
        unload_reports_destroyed(source_runtime),
        "moved-from runtime unload reports Runtime is destroyed");
    if (failures == 0) {
        std::puts("OK: C++ internal-plugin residents are transactional and Runtime-local");
        return 0;
    }
    std::fprintf(stderr, "%d check(s) failed\n", failures);
    return 1;
}
