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

export function registerJsLoader(
    rt: Deno.PointerValue,
    registerFn: (rt: Deno.PointerValue, loader: Deno.PointerValue) => number
): void {
    const lib = getLib();
    const cfgBuf = new Uint8Array([0]);
    const cfgPtr = Deno.UnsafePointer.of(cfgBuf);
    const loaderPtr = lib.symbols.polyplug_js_loader_create(cfgPtr);
    if (loaderPtr === null) {
        throw new Error("polyplug: js loader create failed");
    }
    const err = registerFn(rt, loaderPtr);
    if (err !== 0) {
        throw new Error(`polyplug: js loader register failed: ${err}`);
    }
}