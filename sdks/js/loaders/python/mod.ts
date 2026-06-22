import type { Runtime } from "@polyplug/host";
import { type FfiLibrary, type FfiSymbolTable, getBackend } from "@polyplug/abi";

let _lib: FfiLibrary<typeof PYTHON_SYMBOLS> | null = null;

const PYTHON_SYMBOLS = {
    polyplug_python_loader_create: {
        parameters: ["pointer"] as const,
        result: "pointer" as const,
    },
} satisfies FfiSymbolTable;

function getLib(): FfiLibrary<typeof PYTHON_SYMBOLS> {
    if (!_lib) {
        const libPath = getBackend().env("POLYPLUG_PYTHON_LIB") ?? "libpolyplug_python.so";
        _lib = getBackend().openLibrary(libPath, PYTHON_SYMBOLS);
    }
    return _lib;
}

/**
 * Register the Python loader with a Runtime under the "python" runtime name.
 * The PolyplugPythonConfig is a { min_version_ptr, min_version_len } struct.
 */
export function registerPythonLoader(rt: Runtime, minVersion: string = "3.11"): void {
    const ffi = getBackend();
    const lib = getLib();
    const encoded = new TextEncoder().encode(minVersion);
    const versionPtr = ffi.pointerOf(encoded);
    const cfgBuf = new Uint8Array(16);
    const view = new DataView(cfgBuf.buffer);
    view.setBigUint64(0, ffi.pointerValue(versionPtr), true);
    view.setBigUint64(8, BigInt(encoded.length), true);
    const cfgPtr = ffi.pointerOf(cfgBuf);
    const loaderPtr = lib.symbols.polyplug_python_loader_create(cfgPtr);
    if (loaderPtr === null) {
        throw new Error("polyplug: python loader create failed");
    }
    rt.registerLoader(loaderPtr);
}
