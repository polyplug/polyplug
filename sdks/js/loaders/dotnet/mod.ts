import type { Runtime } from "../../host/polyplug/mod.js";

let _lib: Deno.DynamicLibrary<typeof DOTNET_SYMBOLS> | null = null;

const DOTNET_SYMBOLS = {
    polyplug_dotnet_loader_create: {
        parameters: ["pointer"] as const,
        result: "pointer" as const,
    },
} satisfies Deno.ForeignLibraryInterface;

function getLib(): Deno.DynamicLibrary<typeof DOTNET_SYMBOLS> {
    if (!_lib) {
        const libPath = Deno.env.get("POLYPLUG_DOTNET_LIB") ?? "libpolyplug_dotnet.so";
        _lib = Deno.dlopen(libPath, DOTNET_SYMBOLS);
    }
    return _lib;
}

/**
 * Register the .NET loader with a Runtime.
 * Opens the loader cdylib, creates the loader, then registers it through the
 * Runtime's HostApi.register_loader path.
 */
export function registerDotnetLoader(rt: Runtime, minFramework: string = "10.0"): void {
    const lib = getLib();
    // PolyplugDotnetConfig is { const uint8_t* min_framework_ptr; size_t min_framework_len; }.
    const encoded = new TextEncoder().encode(minFramework);
    const cfgBuf = new Uint8Array(16);
    const view = new DataView(cfgBuf.buffer);
    const strPtr = Deno.UnsafePointer.of(encoded);
    view.setBigUint64(0, BigInt(Deno.UnsafePointer.value(strPtr)), true);
    view.setBigUint64(8, BigInt(encoded.length), true);
    const cfgPtr = Deno.UnsafePointer.of(cfgBuf);
    const loaderPtr = lib.symbols.polyplug_dotnet_loader_create(cfgPtr);
    if (loaderPtr === null) {
        throw new Error("polyplug: dotnet loader create failed");
    }
    rt.registerLoader(loaderPtr);
}
