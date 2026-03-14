let _lib: Deno.DynamicLibrary<typeof JS_DENO_SYMBOLS> | null = null;

const JS_DENO_SYMBOLS = {
    polyplug_js_deno_loader_create: {
        parameters: ["pointer"] as const,
        result: "pointer" as const,
    },
} satisfies Deno.ForeignLibraryInterface;

function getLib() {
    if (!_lib) {
        const libPath = Deno.env.get("POLYPLUG_JS_DENO_LIB")
            ?? "libpolyplug_js_deno.so";
        _lib = Deno.dlopen(libPath, JS_DENO_SYMBOLS);
    }
    return _lib;
}

export function registerJsDenoLoader(
    rt: Deno.PointerValue,
    registerFn: (rt: Deno.PointerValue, loader: Deno.PointerValue) => number
): void {
    const lib = getLib();
    const cfgBuf = new Uint8Array([0]);
    const cfgPtr = Deno.UnsafePointer.of(cfgBuf);
    const loaderPtr = lib.symbols.polyplug_js_deno_loader_create(cfgPtr);
    if (loaderPtr === null) {
        throw new Error("polyplug: js_deno loader create failed");
    }
    const err = registerFn(rt, loaderPtr);
    if (err !== 0) {
        throw new Error(`polyplug: js_deno loader register failed: ${err}`);
    }
}
