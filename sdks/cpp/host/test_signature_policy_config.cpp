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
#include <vector>

#include "polyplug/runtime.hpp"

// ─── Captured config + create/destroy stubs (replace the native exports) ──────
namespace {
std::optional<RuntimeConfig> g_captured_config{};
// Snapshot of the trusted-key bytes taken DURING create (the runtime copies the
// keys during create; the host buffer may be freed once create returns, so the
// captured pointer must not be dereferenced afterward).
std::vector<Ed25519PublicKey> g_captured_keys{};
HostApi g_fake_host{};
} // namespace

extern "C" {
const HostApi* polyplug_runtime_create(const RuntimeConfig* config) {
    g_captured_keys.clear();
    if (config != nullptr) {
        g_captured_config = *config;
        if (config->trusted_keys != nullptr && config->trusted_keys_len > 0) {
            const auto* keys =
                static_cast<const Ed25519PublicKey*>(config->trusted_keys);
            g_captured_keys.assign(keys, keys + config->trusted_keys_len);
        }
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
    // Layout floor: signature_policy fills the former tail padding at offset 44;
    // key pinning then added trusted_keys (a 24-byte Array) at offset 48, growing
    // the struct from 48 to 72 bytes.
    check(sizeof(RuntimeConfig) == 72, "RuntimeConfig is 72 bytes");
    check(offsetof(RuntimeConfig, signature_policy) == 44,
          "signature_policy at offset 44");
    check(offsetof(RuntimeConfig, trusted_keys) == 48,
          "trusted_keys at offset 48");
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

    // trusted_keys({k1, k2}) must reach the marshaled config as a 2-element
    // Array (key pinning). The buffer is transient — alive only across create —
    // so the bytes are verified from the snapshot taken during the create stub.
    {
        g_captured_config.reset();
        std::array<uint8_t, 32> k1{};
        std::array<uint8_t, 32> k2{};
        k1.fill(0x11);
        k2.fill(0x22);
        polyplug::Runtime rt = polyplug::Runtime::builder()
            .trusted_keys({k1, k2})
            .build();
        check(g_captured_config.has_value(),
              "trusted_keys() marshaled a RuntimeConfig");
        check(g_captured_config.has_value()
                  && g_captured_config->trusted_keys != nullptr,
              "trusted_keys() -> config.trusted_keys != nullptr");
        check(g_captured_config.has_value()
                  && g_captured_config->trusted_keys_len == 2,
              "trusted_keys({k1, k2}) -> config.trusted_keys_len == 2");
        check(g_captured_config.has_value()
                  && g_captured_config->trusted_keys__align == alignof(Ed25519PublicKey),
              "trusted_keys() -> config.trusted_keys__align == alignof(Ed25519PublicKey)");
        // The bytes the runtime sees during create must match the input. The
        // snapshot is taken inside the create stub (the host buffer is freed
        // when build() returns), proving the keys are valid for the create call.
        check(g_captured_keys.size() == 2
                  && g_captured_keys[0].bytes[0] == 0x11
                  && g_captured_keys[1].bytes[0] == 0x22,
              "trusted_keys() -> bytes are valid during create");
    }

    // No trusted_keys() -> the fields stay zero (TOFU preserved). The
    // signature_policy-only path must not populate trusted_keys.
    {
        g_captured_config.reset();
        polyplug::Runtime rt = polyplug::Runtime::builder()
            .signature_policy(SignaturePolicy::Required)
            .build();
        check(g_captured_config.has_value()
                  && g_captured_config->trusted_keys == nullptr
                  && g_captured_config->trusted_keys_len == 0,
              "no trusted_keys() -> trusted_keys fields stay zero (TOFU)");
    }

    if (g_failures == 0) {
        std::puts("OK: cpp signature_policy setter writes RuntimeConfig.signature_policy");
        return 0;
    }
    std::fprintf(stderr, "%d check(s) failed\n", g_failures);
    return 1;
}
