let _lib: Deno.DynamicLibrary<typeof PYTHON_SYMBOLS> | null = null;

const PYTHON_SYMBOLS = {
    polyplug_python_loader_create: {
        parameters: ["pointer"] as const,
        result: "pointer" as const,
    },
} satisfies Deno.ForeignLibraryInterface;

function getLib() {
    if (!_lib) {
        const libPath = Deno.env.get("POLYPLUG_PYTHON_LIB")
            ?? "libpolyplug_python.so";
        _lib = Deno.dlopen(libPath, PYTHON_SYMBOLS);
    }
    return _lib;
}

export function registerPythonLoader(
    rt: Deno.PointerValue,
    registerFn: (rt: Deno.PointerValue, loader: Deno.PointerValue) => number,
    minVersion: string = "3.11"
): void {
    const lib = getLib();
    const encoded = new TextEncoder().encode(minVersion);
    const cfgBuf = new Uint8Array(16); // ptr (8) + len (8)
    const ptr = Deno.UnsafePointer.of(encoded);
    const view = new DataView(cfgBuf.buffer);
    view.setBigUint64(0, BigInt(Deno.UnsafePointer.value(ptr)), true);
    view.setBigUint64(8, BigInt(encoded.length), true);
    const cfgPtr = Deno.UnsafePointer.of(cfgBuf);
    const loaderPtr = lib.symbols.polyplug_python_loader_create(cfgPtr);
    if (loaderPtr === null) {
        throw new Error("polyplug: python loader create failed");
    }
    const err = registerFn(rt, loaderPtr);
    if (err !== 0) {
        throw new Error(`polyplug: python loader register failed: ${err}`);
    }
}
