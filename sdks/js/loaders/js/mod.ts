import type { Runtime } from "@polyplug/host";

let _lib: Deno.DynamicLibrary<typeof JS_SYMBOLS> | null = null;

const JS_SYMBOLS = {
    polyplug_js_loader_create: {
        parameters: ["pointer"] as const,
        result: "pointer" as const,
    },
} satisfies Deno.ForeignLibraryInterface;

function getLib(): Deno.DynamicLibrary<typeof JS_SYMBOLS> {
    if (!_lib) {
        const libPath = Deno.env.get("POLYPLUG_JS_LIB") ?? "libpolyplug_js.so";
        _lib = Deno.dlopen(libPath, JS_SYMBOLS);
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
    const cfgPtr = Deno.UnsafePointer.of(cfgBuf);
    const loaderPtr = lib.symbols.polyplug_js_loader_create(cfgPtr);
    if (loaderPtr === null) {
        throw new Error("polyplug: js loader create failed");
    }
    rt.registerLoader(loaderPtr);
}
