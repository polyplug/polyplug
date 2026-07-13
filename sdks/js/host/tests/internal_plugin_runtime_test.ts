import { Runtime, InternalPluginBundle } from "../polyplug/mod.js";
import {
    HOST_API_GET_ERROR_LEN_OFFSET,
    HOST_API_SIZE,
    HOST_API_UNLOAD_BUNDLE_OFFSET,
} from "../../abi/abi.ts";
import { getBackend } from "@polyplug/abi";
import { assertEquals, test } from "../../testing/harness.ts";

const ABI_ERROR_SIZE = 24;
const MANIFEST = 'name = "js.runtime"\nid = 71\nversion = "1.0.0"\nloader = "js-quickjs"\nprovides = []\nfunction_count = {}\nneeds_reinit_on_dep_reload = false\nfile = "plugin.js"\n';

function writeError(pointer: unknown, code: number): void {
    const bytes = getBackend().pointerView(pointer).getArrayBuffer(ABI_ERROR_SIZE);
    new DataView(bytes).setUint32(0, code, true);
}

test("runtime commits internal-plugin registrations and retains residents until unload", () => {
    const backend = getBackend();
    const hostTable = new Uint8Array(HOST_API_SIZE);
    const hostView = new DataView(hostTable.buffer);
    let allowUnload = false;
    let failRegistration = true;
    let failCommit = false;
    let aborts = 0;
    let releases = 0;
    let allowDestroy = false;
    let destroyAttempts = 0;

    const unload = backend.makeCallback(
        { parameters: ["pointer", "u64", "pointer"], result: "void" },
        (_host, _bundleId, outError) => writeError(outError, allowUnload ? 0 : 1),
    );
    const errorLen = backend.makeCallback(
        { parameters: ["pointer"], result: "usize" },
        () => 0n,
    );
    hostView.setBigUint64(HOST_API_UNLOAD_BUNDLE_OFFSET, backend.pointerValue(unload.pointer), true);
    hostView.setBigUint64(HOST_API_GET_ERROR_LEN_OFFSET, backend.pointerValue(errorLen.pointer), true);

    assertEquals(
        hostView.getBigUint64(HOST_API_UNLOAD_BUNDLE_OFFSET, true) !== 0n,
        true,
        "synthetic HostApi must retain its unload callback pointer",
    );
    const fakeLibrary = {
        symbols: {
            polyplug_runtime_destroy: () => {
                destroyAttempts += 1;
                return allowDestroy;
            },
            polyplug_begin_internal_plugin: (
                _host: unknown,
                _manifest: unknown,
                _length: bigint,
                _language: number,
                outBundleId: unknown,
                outError: unknown,
            ) => {
                new DataView(backend.pointerView(outBundleId).getArrayBuffer(8)).setBigUint64(0, 71n, true);
                writeError(outError, 0);
            },
            polyplug_commit_internal_plugin: (_host: unknown, _bundleId: bigint, outError: unknown) => {
                writeError(outError, failCommit ? 1 : 0);
            },
            polyplug_abort_internal_plugin: () => {
                aborts += 1;
            },
        },
        _callbacks: [unload, errorLen],
        _roots: [hostTable],
    };
    const runtime = new Runtime(fakeLibrary as never, backend.pointerOf(hostTable));
    assertEquals(typeof runtime.registerInternalPlugin, "function");
    assertEquals(typeof runtime.registerInternalPluginWithHandles, "function");
    assertEquals(Object.keys(runtime).length, 0);
    const bundle = new InternalPluginBundle(MANIFEST, {
        release(): void {
            releases += 1;
        },
    }, () => {
        if (failRegistration) {
            throw new Error("registration failed");
        }
    });

    try {
        let registrationFailed = false;
        try {
            runtime.registerInternalPlugin(bundle);
        } catch {
            registrationFailed = true;
        }
        assertEquals(registrationFailed, true, "registration failure must surface");
        assertEquals(aborts, 1, "registration failure before commit must abort staging");
        assertEquals(releases, 0, "failed registration must not transfer the resident");

        failRegistration = false;
        failCommit = true;
        let commitFailed = false;
        try {
            runtime.registerInternalPlugin(bundle);
        } catch {
            commitFailed = true;
        }
        assertEquals(commitFailed, true, "commit failure must surface");
        assertEquals(aborts, 1, "commit failure must not abort a transaction core already discarded");
        assertEquals(releases, 0, "failed commit must not transfer the resident");

        failCommit = false;
        const bundleId = runtime.registerInternalPlugin(bundle);
        assertEquals(bundleId, 71n);
        assertEquals(
            hostView.getBigUint64(HOST_API_UNLOAD_BUNDLE_OFFSET, true) !== 0n,
            true,
            "synthetic HostApi callback pointer must remain live through staging",
        );
        assertEquals(
            backend.pointerView(runtime.host()).getBigUint64(HOST_API_UNLOAD_BUNDLE_OFFSET) !== 0n,
            true,
            "runtime must retain the synthetic HostApi pointer through staging",
        );

        let unloadFailed = false;
        try {
            runtime.unloadBundle(bundleId);
        } catch {
            unloadFailed = true;
        }
        assertEquals(unloadFailed, true, "failed logical unload must surface the core error");
        assertEquals(releases, 0, "failed logical unload must retain the resident");

        allowUnload = true;
        runtime.unloadBundle(bundleId);
        assertEquals(releases, 1, "successful logical unload must release the resident");

        assertEquals(runtime.destroy(), false, "wrong-thread rejection must keep destroy retryable");
        assertEquals(destroyAttempts, 1);
        allowDestroy = true;
        assertEquals(runtime.destroy(), true, "owner retry must consume the runtime");
        assertEquals(destroyAttempts, 2);
    } finally {
        allowDestroy = true;
        runtime.destroy();
        unload.close();
        errorLen.close();
    }
});
