import { Runtime, InProcessBundle } from "../polyplug/mod.js";
import {
    HOST_API_GET_ERROR_LEN_OFFSET,
    HOST_API_REGISTER_IN_PROCESS_BUNDLE_OFFSET,
    HOST_API_SIZE,
    HOST_API_UNLOAD_BUNDLE_OFFSET,
} from "../../abi/abi.ts";
import { getBackend } from "@polyplug/abi";
import { assertEquals, assertStrictEquals, test } from "../../testing/harness.ts";

const ABI_ERROR_SIZE = 24;

function writeError(pointer: unknown, code: number): void {
    const bytes = getBackend().pointerView(pointer).getArrayBuffer(ABI_ERROR_SIZE);
    new DataView(bytes).setUint32(0, code, true);
}

test("runtime retains an in-process resident through failed unload", () => {
    const backend = getBackend();
    const hostTable = new Uint8Array(HOST_API_SIZE);
    const hostView = new DataView(hostTable.buffer);
    let allowUnload = false;
    let allowRegistration = false;
    let releases = 0;

    const register = backend.makeCallback(
        { parameters: ["pointer", "pointer", "pointer", "pointer"], result: "void" },
        (_host, _registration, outBundleId, outError) => {
            if (!allowRegistration) {
                writeError(outError, 1);
                return;
            }
            const bundleId = backend.pointerView(outBundleId).getArrayBuffer(8);
            new DataView(bundleId).setBigUint64(0, 71n, true);
            writeError(outError, 0);
        },
    );
    const unload = backend.makeCallback(
        { parameters: ["pointer", "u64", "pointer"], result: "void" },
        (_host, _bundleId, outError) => writeError(outError, allowUnload ? 0 : 1),
    );
    const errorLen = backend.makeCallback(
        { parameters: ["pointer"], result: "usize" },
        () => 0n,
    );
    hostView.setBigUint64(HOST_API_REGISTER_IN_PROCESS_BUNDLE_OFFSET, backend.pointerValue(register.pointer), true);
    hostView.setBigUint64(HOST_API_UNLOAD_BUNDLE_OFFSET, backend.pointerValue(unload.pointer), true);
    hostView.setBigUint64(HOST_API_GET_ERROR_LEN_OFFSET, backend.pointerValue(errorLen.pointer), true);

    const fakeLibrary = { symbols: { polyplug_runtime_destroy: () => {} } };
    // Runtime only calls this synthetic library's destroy field in this ABI unit test.
    const runtime = new Runtime(fakeLibrary as never, backend.pointerOf(hostTable));
    const bundle = new InProcessBundle(new Uint8Array(64), {
        release(): void {
            releases += 1;
        },
    });

    try {
        let registrationFailed = false;
        try {
            runtime.registerInProcessBundle(bundle);
        } catch {
            registrationFailed = true;
        }
        assertEquals(registrationFailed, true, "rejected registration must surface the core error");
        assertStrictEquals(bundle._inProcessRegistration().byteLength, 64);
        assertEquals(releases, 0, "rejected registration must not transfer the resident");

        allowRegistration = true;
        const bundleId = runtime.registerInProcessBundle(bundle);
        assertEquals(bundleId, 71n);

        let failed = false;
        try {
            runtime.unloadBundle(bundleId);
        } catch {
            failed = true;
        }
        assertEquals(failed, true, "failed logical unload must surface the core error");
        assertEquals(releases, 0, "failed logical unload must retain the resident");

        allowUnload = true;
        runtime.unloadBundle(bundleId);
        assertEquals(releases, 1, "successful logical unload must release the resident");
    } finally {
        runtime.destroy();
        register.close();
        unload.close();
        errorLen.close();
    }
});
