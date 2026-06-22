import type { Runtime } from "@polyplug/host";
import { type FfiLibrary, type FfiSymbolTable, getBackend } from "@polyplug/abi";

let _lib: FfiLibrary<typeof NATIVE_SYMBOLS> | null = null;

const NATIVE_SYMBOLS = {
    polyplug_native_loader_create: {
        parameters: ["pointer"] as const,
        result: "pointer" as const,
    },
} satisfies FfiSymbolTable;

function getLib(): FfiLibrary<typeof NATIVE_SYMBOLS> {
    if (!_lib) {
        const libPath = getBackend().env("POLYPLUG_NATIVE_LIB") ?? "libpolyplug_native.so";
        _lib = getBackend().openLibrary(libPath, NATIVE_SYMBOLS);
    }
    return _lib;
}

/**
 * Register the native loader with a Runtime.
 * Opens the loader cdylib, creates the loader, then registers it through the
 * Runtime's HostApi.register_loader path.
 */
export function registerNativeLoader(rt: Runtime): void {
    const lib = getLib();
    const cfgBuf = new Uint8Array([0]);
    const cfgPtr = getBackend().pointerOf(cfgBuf);
    const loaderPtr = lib.symbols.polyplug_native_loader_create(cfgPtr);
    if (loaderPtr === null) {
        throw new Error("polyplug: native loader create failed");
    }
    rt.registerLoader(loaderPtr);
}
