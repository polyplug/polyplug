// sdks/cpp/host/test_reload_runtime.cpp
// REAL-runtime hot-reload notification test (mirrors
// sdks/lua/host/tests/test_reload_runtime.lua and
// sdks/js/host/tests/reload_runtime_test.ts).
//
// tests/integration/cpp/hot_reload_test.cpp covers the SDK-side ReloadPhase
// type only — it builds local structs and asserts on them, which can never
// catch a broken FFI path. This test drives the actual flow: create a runtime
// through the C++ host SDK Builder with an on_reload callback (routed through
// detail::on_reload_trampoline for the `void(*)(void*, const ReloadPhase*)`
// ABI signature), register the native loader, load the native reload fixture
// bundle, trigger a reload through the runtime, and assert the callback fired
// with REAL phase data delivered across the C ABI.
//
// Skip-honestly policy (matches the lua/js reload runtime tests): the core
// and native-loader cdylibs are LINK-time dependencies — when they are missing
// the build fails loudly via the Makefile guard with instructions — and the
// fixture checks below fail loudly at runtime. A runtime test that silently
// passes hides exactly the never-run breakage class it exists to catch.
//
// Run from repo root:
//   cargo build --release -p polyplug -p polyplug_native
//   bash tests/fixtures/build_all.sh
//   make -C sdks/cpp test

#include <cstdint>
#include <cstdio>
#include <filesystem>
#include <iostream>
#include <string>
#include <vector>

#include "polyplug/runtime.hpp"
#include "polyplug/id.hpp"
#include "polyplug_loaders_native.hpp"

// Fixtures root — injected by the Makefile as an absolute path so the test
// binary works regardless of the invoking working directory.
#ifndef POLYPLUG_FIXTURES_DIR
#define POLYPLUG_FIXTURES_DIR "../../tests/fixtures"
#endif

// Platform-specific cdylib naming (matches tests/fixtures/build_all.sh):
// `<name>.dll` on Windows (no `lib` prefix), `lib<name>.dylib` on macOS,
// `lib<name>.so` on Linux.
#if defined(_WIN32)
static const char* const kV1LibName = "reload_plugin_v1.dll";
static const char* const kV2LibName = "reload_plugin_v2.dll";
#elif defined(__APPLE__)
static const char* const kV1LibName = "libreload_plugin_v1.dylib";
static const char* const kV2LibName = "libreload_plugin_v2.dylib";
#else
static const char* const kV1LibName = "libreload_plugin_v1.so";
static const char* const kV2LibName = "libreload_plugin_v2.so";
#endif

// Owned copy of one delivered ReloadPhase: the pointee (and the StringViews
// inside it) is valid only for the duration of the callback, so every field
// is copied before the callback returns.
struct CapturedPhase {
    ReloadPhaseType type;
    uint64_t bundle_id;
    std::string bundle_name;
    std::string reason;
};

static int tests_passed = 0;
static int tests_failed = 0;

static void check(bool ok, const std::string& message) {
    if (ok) {
        std::cout << "  PASS: " << message << std::endl;
        ++tests_passed;
    } else {
        std::cout << "  FAIL: " << message << std::endl;
        ++tests_failed;
    }
}

static std::string view_to_string(const StringView& view) {
    if (view.ptr == nullptr || view.len == 0) {
        return std::string{};
    }
    return std::string(reinterpret_cast<const char*>(view.ptr), view.len);
}

static void require_fixture(const std::filesystem::path& path) {
    std::error_code ec{};
    if (!std::filesystem::exists(path, ec)) {
        std::cerr << "FATAL: reload fixture missing: " << path.string()
                  << " — run `bash tests/fixtures/build_all.sh` first."
                  << std::endl;
        std::exit(1);
    }
}

int main() {
    const std::filesystem::path fixtures_dir{POLYPLUG_FIXTURES_DIR};
    const std::filesystem::path v1_dir = fixtures_dir / "reload_plugin_v1";
    // The reload target is the v2 cdylib INSIDE its bundle dir — the runtime
    // reads the sibling manifest.toml during reload (mirrors
    // integration_reload.rs).
    const std::filesystem::path v2_so = fixtures_dir / "reload_plugin_v2" / kV2LibName;

    require_fixture(v1_dir / "manifest.toml");
    require_fixture(v1_dir / kV1LibName);
    require_fixture(v2_so);

    // Name from tests/fixtures/reload_plugin_v1/manifest.toml; the bundle id
    // is FNV-1a 64 of the name (TRUST_MODEL §2) — computed via the SDK helper
    // in polyplug/id.hpp, never hand-rolled.
    constexpr uint64_t kV1BundleId = polyplug::bundle_id("reload_plugin_v1");

    std::cout << "=== on_reload fires with real phase data on a real runtime reload ==="
              << std::endl;

    std::vector<CapturedPhase> phases{};

    RuntimeConfig cfg{};
    cfg.hot_reload_enabled = true;
    polyplug::Runtime rt = polyplug::Runtime::builder()
        .config(cfg)
        .on_reload([&phases](const ReloadPhase& phase) {
            phases.push_back(CapturedPhase{
                phase.phase_type,
                phase.bundle_id,
                view_to_string(phase.bundle_name),
                view_to_string(phase.reason),
            });
        })
        .build();

    polyplug::loaders::register_native(rt);

    rt.load_bundle(v1_dir.string());
    check(phases.empty(), "no reload phases before the reload");

    rt.reload_bundle(v2_so.string());

    check(phases.size() >= 2,
        "reload must deliver at least Preparing + Reloaded (got "
        + std::to_string(phases.size()) + ")");

    if (!phases.empty()) {
        const CapturedPhase& first = phases.front();
        check(first.type == ReloadPhaseType::Preparing,
            "first phase must be Preparing (got type "
            + std::to_string(static_cast<uint32_t>(first.type)) + ")");
        check(first.bundle_id == kV1BundleId,
            "Preparing phase must carry the real bundle id from the manifest (got "
            + std::to_string(first.bundle_id) + ", want "
            + std::to_string(kV1BundleId) + ")");
        check(first.bundle_name == "reload_plugin_v1",
            "Preparing phase must carry the real bundle name (got \""
            + first.bundle_name + "\")");
        check(first.reason.empty(),
            "non-Failed phase must carry the null-view reason as empty (got \""
            + first.reason + "\")");
    }

    bool saw_reloaded = false;
    for (const CapturedPhase& phase : phases) {
        if (phase.type == ReloadPhaseType::Reloaded) {
            saw_reloaded = true;
        }
    }
    check(saw_reloaded, "a Reloaded phase must follow");

    std::cout << std::endl
              << "Results: " << tests_passed << " passed, "
              << tests_failed << " failed" << std::endl;
    return tests_failed > 0 ? 1 : 0;
}
