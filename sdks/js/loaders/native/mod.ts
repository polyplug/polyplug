import type { Runtime } from "@polyplug/sdk/host";

let _lib: Deno.DynamicLibrary<typeof NATIVE_SYMBOLS> | null = null;

const NATIVE_SYMBOLS = {
    polyplug_native_loader_create: {
        parameters: ["pointer"] as const,
        result: "pointer" as const,
    },
} satisfies Deno.ForeignLibraryInterface;

function getLib(): Deno.DynamicLibrary<typeof NATIVE_SYMBOLS> {
    if (!_lib) {
        const libPath = Deno.env.get("POLYPLUG_NATIVE_LIB") ?? "libpolyplug_native.so";
        _lib = Deno.dlopen(libPath, NATIVE_SYMBOLS);
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
    const cfgPtr = Deno.UnsafePointer.of(cfgBuf);
    const loaderPtr = lib.symbols.polyplug_native_loader_create(cfgPtr);
    if (loaderPtr === null) {
        throw new Error("polyplug: native loader create failed");
    }
    rt.registerLoader(loaderPtr);
}
