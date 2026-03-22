let _lib: Deno.DynamicLibrary<typeof LUA_SYMBOLS> | null = null;

const LUA_SYMBOLS = {
    polyplug_lua_loader_create: {
        parameters: ["pointer"] as const,
        result: "pointer" as const,
    },
} satisfies Deno.ForeignLibraryInterface;

function getLib(): Deno.DynamicLibrary<typeof LUA_SYMBOLS> {
    if (!_lib) {
        const libPath = Deno.env.get("POLYPLUG_LUA_LIB") ?? "libpolyplug_lua.so";
        _lib = Deno.dlopen(libPath, LUA_SYMBOLS);
    }
    return _lib;
}

export function registerLuaLoader(
    rt: Deno.PointerValue,
    registerFn: (rt: Deno.PointerValue, loader: Deno.PointerValue) => number
): void {
    const lib = getLib();
    const cfgBuf = new Uint8Array([0]);
    const cfgPtr = Deno.UnsafePointer.of(cfgBuf);
    const loaderPtr = lib.symbols.polyplug_lua_loader_create(cfgPtr);
    if (loaderPtr === null) {
        throw new Error("polyplug: lua loader create failed");
    }
    const err = registerFn(rt, loaderPtr);
    if (err !== 0) {
        throw new Error(`polyplug: lua loader register failed: ${err}`);
    }
}