import type { Runtime } from "@polyplug/host";
import { type FfiLibrary, type FfiSymbolTable, getBackend } from "@polyplug/abi";

let _lib: FfiLibrary<typeof JS_SYMBOLS> | null = null;

const JS_SYMBOLS = {
    polyplug_js_loader_create: {
        parameters: ["pointer"] as const,
        result: "pointer" as const,
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
 * Register the JavaScript (QuickJS) loader with a Runtime under the
 * "js-quickjs" runtime name.
 */
export function registerJsLoader(rt: Runtime): void {
    const lib = getLib();
    const cfgBuf = new Uint8Array([0]);
    const cfgPtr = getBackend().pointerOf(cfgBuf);
    const loaderPtr = lib.symbols.polyplug_js_loader_create(cfgPtr);
    if (loaderPtr === null) {
        throw new Error("polyplug: js loader create failed");
    }
    rt.registerLoader(loaderPtr);
}
