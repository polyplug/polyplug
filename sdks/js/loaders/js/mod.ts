import type { Runtime } from "@polyplug/host";
import { type FfiLibrary, type FfiSymbolTable, getBackend } from "@polyplug/abi";

let _lib: FfiLibrary<typeof JS_SYMBOLS> | null = null;

const JS_SYMBOLS = {
    polyplug_js_loader_create: {
        parameters: [] as const,
        result: "pointer" as const,
    },
    polyplug_js_in_process_bridge_create: {
        parameters: ["pointer", "pointer", "pointer", "u64", "u32", "u32", "u32"] as const,
        result: "pointer" as const,
    },
    polyplug_js_in_process_bridge_interface: {
        parameters: ["pointer"] as const,
        result: "pointer" as const,
    },
    polyplug_js_in_process_bridge_free: {
        parameters: ["pointer"] as const,
        result: "void" as const,
    },
} satisfies FfiSymbolTable;

function getLib(): FfiLibrary<typeof JS_SYMBOLS> {
    if (!_lib) {
        const libPath = getBackend().env("POLYPLUG_JS_LIB") ?? "libpolyplug_js.so";
        _lib = getBackend().openLibrary(libPath, JS_SYMBOLS);
    }
    return _lib;
}

/**
 * Returns the explicit Rust bridge library required by JavaScript in-process
 * adapters. Bundle residents retain this object through logical unload.
 */
export function bridgeLibrary(): FfiLibrary<typeof JS_SYMBOLS> {
    return getLib();
}

/**
 * Register the JavaScript (QuickJS) loader with a Runtime under the
 * "js-quickjs" runtime name.
 */
export function registerJsLoader(rt: Runtime): void {
    const lib = getLib();
    const loaderPtr = lib.symbols.polyplug_js_loader_create();
    if (loaderPtr === null) {
        throw new Error("polyplug: js loader create failed");
    }
    rt.registerLoader(loaderPtr);
}
