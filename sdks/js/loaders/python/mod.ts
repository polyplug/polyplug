import type { Runtime } from "@polyplug/host";

let _lib: Deno.DynamicLibrary<typeof PYTHON_SYMBOLS> | null = null;

const PYTHON_SYMBOLS = {
    polyplug_python_loader_create: {
        parameters: ["pointer"] as const,
        result: "pointer" as const,
    },
} satisfies Deno.ForeignLibraryInterface;

function getLib(): Deno.DynamicLibrary<typeof PYTHON_SYMBOLS> {
    if (!_lib) {
        const libPath = Deno.env.get("POLYPLUG_PYTHON_LIB") ?? "libpolyplug_python.so";
        _lib = Deno.dlopen(libPath, PYTHON_SYMBOLS);
    }
    return _lib;
}

/**
 * Register the Python loader with a Runtime under the "python" runtime name.
 * The PolyplugPythonConfig is a { min_version_ptr, min_version_len } struct.
 */
export function registerPythonLoader(rt: Runtime, minVersion: string = "3.11"): void {
    const lib = getLib();
    const encoded = new TextEncoder().encode(minVersion);
    const versionPtr = Deno.UnsafePointer.of(encoded);
    const cfgBuf = new Uint8Array(16);
    const view = new DataView(cfgBuf.buffer);
    view.setBigUint64(0, BigInt(Deno.UnsafePointer.value(versionPtr)), true);
    view.setBigUint64(8, BigInt(encoded.length), true);
    const cfgPtr = Deno.UnsafePointer.of(cfgBuf);
    const loaderPtr = lib.symbols.polyplug_python_loader_create(cfgPtr);
    if (loaderPtr === null) {
        throw new Error("polyplug: python loader create failed");
    }
    rt.registerLoader(loaderPtr);
}
