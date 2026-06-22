// sdks/js/host/tests/reload_runtime_test.ts
// REAL-runtime hot-reload notification test.
//
// `reload_notification_test.ts` covers the SDK-side ReloadPhase/RuntimeConfig
// types only — it builds local objects and asserts on them, which can never
// catch a broken FFI path. This test drives the actual flow: create a runtime
// through mod.js with an onReload callback, load the native reload fixture
// bundle, trigger a reload through the runtime, and assert the callback fired
// with REAL phase data delivered across the C ABI.
//
// Skip-honestly policy (matches tests/fixtures/deno_host_test.ts): when
// POLYPLUG_LIB is absent the test FAILS LOUDLY with instructions — a runtime
// test that silently passes hides exactly the never-run breakage class it
// exists to catch. The fail is raised from inside the test body (not at import
// time) so the harness reports it as a real test failure and the runner exits
// non-zero, which is the runtime-agnostic equivalent of the old `Deno.exit(1)`.
//
// All native-FFI access goes through the runtime-agnostic seam (`getBackend()`),
// so this body has no `Deno.*`.
//
// Registered into the runtime-agnostic harness; run the whole suite from repo
// root via the Deno entrypoint (or `just test-host-js`):
//   cargo build --release -p polyplug -p polyplug_native
//   bash tests/fixtures/build_all.sh
//   POLYPLUG_LIB=$PWD/target/release/libpolyplug.so \
//   POLYPLUG_NATIVE_LIB=$PWD/target/release/libpolyplug_native.so \
//   deno run --allow-ffi --allow-env --allow-read --allow-write \
//     sdks/js/testing/run_deno.ts

import { openPolyplug, ReloadPhase, runtimeNew } from "../mod.js";
import { registerNativeLoader } from "../../loaders/native/mod.ts";
import { FfiNotFoundError, getBackend } from "@polyplug/abi";
import { assertEquals, test } from "../../testing/harness.ts";

const HERE: string = new URL(".", import.meta.url).pathname;
const FIXTURES_DIR: string = `${HERE}../../../../tests/fixtures`;
const V1_DIR: string = `${FIXTURES_DIR}/reload_plugin_v1`;
// The reload target is the v2 .so INSIDE its bundle dir — the runtime reads
// the sibling manifest.toml during reload (mirrors integration_reload.rs).
const V2_SO: string = `${FIXTURES_DIR}/reload_plugin_v2/libreload_plugin_v2.so`;
// id from tests/fixtures/reload_plugin_v1/manifest.toml.
const V1_BUNDLE_ID = 16808897324254478442n;

function requireLib(): string {
    const lib: string = getBackend().env("POLYPLUG_LIB") ?? getBackend().env("POLYPLUG_SO") ?? "";
    if (!lib) {
        throw new Error(
            "FATAL: POLYPLUG_LIB not set — this runtime test must not silently pass.\n" +
                "Build the core and point the test at it:\n" +
                "  cargo build --release -p polyplug -p polyplug_native\n" +
                "  export POLYPLUG_LIB=$PWD/target/release/libpolyplug.so\n" +
                "  export POLYPLUG_NATIVE_LIB=$PWD/target/release/libpolyplug_native.so",
        );
    }
    return lib;
}

function requireFixture(path: string): void {
    try {
        getBackend().statSync(path);
    } catch (error) {
        if (error instanceof FfiNotFoundError) {
            throw new Error(
                `reload fixture missing: ${path} — run \`bash tests/fixtures/build_all.sh\` first`,
            );
        }
        throw error;
    }
}

test("onReload fires with real phase data on a real runtime reload", () => {
    const polyplugLib: string = requireLib();

    requireFixture(`${V1_DIR}/manifest.toml`);
    requireFixture(`${V1_DIR}/libreload_plugin_v1.so`);
    requireFixture(V2_SO);

    const lib = openPolyplug(polyplugLib);
    try {
        const phases: ReloadPhase[] = [];
        const rt = runtimeNew(lib, {
            config: { hotReloadEnabled: true },
            onReload: (phase: ReloadPhase) => {
                phases.push(phase);
            },
        });
        try {
            registerNativeLoader(rt);
            rt.loadBundle(V1_DIR);
            assertEquals(phases.length, 0, "no reload phases before the reload");

            rt.reloadBundle(V2_SO);

            assertEquals(
                phases.length >= 2,
                true,
                `reload must deliver at least Preparing + Reloaded, got ${phases.length}: ${
                    phases.map((p) => p.toString()).join(", ")
                }`,
            );
            const preparing = phases[0];
            assertEquals(
                preparing.isPreparing(),
                true,
                `first phase must be Preparing, got: ${preparing.toString()}`,
            );
            assertEquals(
                preparing.bundleId,
                V1_BUNDLE_ID,
                "Preparing phase must carry the real bundle id from the manifest",
            );
            assertEquals(
                phases.some((p) => p.isReloaded()),
                true,
                `a Reloaded phase must follow, got: ${phases.map((p) => p.toString()).join(", ")}`,
            );
        } finally {
            rt[Symbol.dispose]();
        }
    } finally {
        lib.close();
    }
});
