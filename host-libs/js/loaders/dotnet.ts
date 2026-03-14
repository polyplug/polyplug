let _lib: Deno.DynamicLibrary<typeof DOTNET_SYMBOLS> | null = null;

const DOTNET_SYMBOLS = {
    polyplug_dotnet_loader_create: {
        parameters: ["pointer"] as const,
        result: "pointer" as const,
    },
} satisfies Deno.ForeignLibraryInterface;

function getLib() {
    if (!_lib) {
        const libPath = Deno.env.get("POLYPLUG_DOTNET_LIB")
            ?? "libpolyplug_dotnet.so";
        _lib = Deno.dlopen(libPath, DOTNET_SYMBOLS);
    }
    return _lib;
}

export function registerDotnetLoader(
    rt: Deno.PointerValue,
    registerFn: (rt: Deno.PointerValue, loader: Deno.PointerValue) => number,
    minFramework: string = "10.0"
): void {
    const lib = getLib();
    const encoded = new TextEncoder().encode(minFramework);
    const cfgBuf = new Uint8Array(16); // ptr (8) + len (8)
    const ptr = Deno.UnsafePointer.of(encoded);
    const view = new DataView(cfgBuf.buffer);
    view.setBigUint64(0, BigInt(Deno.UnsafePointer.value(ptr)), true);
    view.setBigUint64(8, BigInt(encoded.length), true);
    const cfgPtr = Deno.UnsafePointer.of(cfgBuf);
    const loaderPtr = lib.symbols.polyplug_dotnet_loader_create(cfgPtr);
    if (loaderPtr === null) {
        throw new Error("polyplug: dotnet loader create failed");
    }
    const err = registerFn(rt, loaderPtr);
    if (err !== 0) {
        throw new Error(`polyplug: dotnet loader register failed: ${err}`);
    }
}
