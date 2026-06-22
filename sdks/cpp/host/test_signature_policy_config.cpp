// sdks/cpp/host/test_signature_policy_config.cpp
// Asserts the C++ host SDK Builder writes RuntimeConfig.signature_policy through
// the signature_policy() setter, WITHOUT the native library.
//
// runtime.hpp declares polyplug_runtime_create / polyplug_runtime_destroy as
// extern "C" but the Builder only calls them — it does not define them. This
// test defines local stubs that CAPTURE the RuntimeConfig the Builder marshals,
// drives the Builder through signature_policy(), and asserts the captured field.
// This exercises the real setter→config path; full runtime-load coverage lives
// in host/test_reload_runtime.cpp.
//
// Run from repo root:
//   make -C sdks/cpp test

#include <cstdint>
#include <cstdio>
#include <optional>

#include "polyplug/runtime.hpp"

// ─── Captured config + create/destroy stubs (replace the native exports) ──────
namespace {
std::optional<RuntimeConfig> g_captured_config{};
HostApi g_fake_host{};
} // namespace

extern "C" {
const HostApi* polyplug_runtime_create(const RuntimeConfig* config) {
    if (config != nullptr) {
        g_captured_config = *config;
    } else {
        g_captured_config.reset();
    }
    // Builder treats a non-null return as success; the fake host is never called.
    return &g_fake_host;
}

void polyplug_runtime_destroy(const HostApi*) {}
}

static int g_failures = 0;

static void check(bool cond, const char* what) {
    if (!cond) {
        std::fprintf(stderr, "FAIL: %s\n", what);
        g_failures += 1;
    }
}

int main() {
    // Layout floor: the field fills the former tail padding; struct stays 48.
    check(sizeof(RuntimeConfig) == 48, "RuntimeConfig stays 48 bytes");
    check(offsetof(RuntimeConfig, signature_policy) == 44,
          "signature_policy at offset 44");
    check(static_cast<uint32_t>(SignaturePolicy::Off) == 0, "SignaturePolicy::Off == 0");
    check(static_cast<uint32_t>(SignaturePolicy::WarnOnly) == 1, "SignaturePolicy::WarnOnly == 1");
    check(static_cast<uint32_t>(SignaturePolicy::Required) == 2, "SignaturePolicy::Required == 2");

    // signature_policy(Required) must reach the marshaled config as 2.
    {
        g_captured_config.reset();
        polyplug::Runtime rt = polyplug::Runtime::builder()
            .signature_policy(SignaturePolicy::Required)
            .build();
        check(g_captured_config.has_value(), "Builder marshaled a RuntimeConfig");
        check(g_captured_config.has_value()
                  && g_captured_config->signature_policy == SignaturePolicy::Required,
              "signature_policy(Required) -> config.signature_policy == Required");
        check(g_captured_config.has_value()
                  && static_cast<uint32_t>(g_captured_config->signature_policy) == 2,
              "signature_policy(Required) -> raw value 2");
    }

    // No signature_policy() and no other options -> default config path (null),
    // preserving Off behavior (no config marshaled).
    {
        g_captured_config.reset();
        polyplug::Runtime rt = polyplug::Runtime::builder().build();
        check(!g_captured_config.has_value(),
              "no options -> null config (default Off preserved)");
    }

    // signature_policy(WarnOnly) reaches the config as 1.
    {
        g_captured_config.reset();
        polyplug::Runtime rt = polyplug::Runtime::builder()
            .signature_policy(SignaturePolicy::WarnOnly)
            .build();
        check(g_captured_config.has_value()
                  && static_cast<uint32_t>(g_captured_config->signature_policy) == 1,
              "signature_policy(WarnOnly) -> raw value 1");
    }

    if (g_failures == 0) {
        std::puts("OK: cpp signature_policy setter writes RuntimeConfig.signature_policy");
        return 0;
    }
    std::fprintf(stderr, "%d check(s) failed\n", g_failures);
    return 1;
}
