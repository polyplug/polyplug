import type { Runtime } from "@polyplug/host";
import { type FfiLibrary, type FfiSymbolTable, getBackend } from "@polyplug/abi";

let _lib: FfiLibrary<typeof LUA_SYMBOLS> | null = null;

const LUA_SYMBOLS = {
    polyplug_lua_loader_create: {
        parameters: [] as const,
        result: "pointer" as const,
    },
} satisfies FfiSymbolTable;

function getLib(): FfiLibrary<typeof LUA_SYMBOLS> {
    if (!_lib) {
        const libPath = getBackend().env("POLYPLUG_LUA_LIB") ?? "libpolyplug_lua.so";
        _lib = getBackend().openLibrary(libPath, LUA_SYMBOLS);
    }
    return _lib;
}

/**
 * Register the Lua loader with a Runtime under the "lua" runtime name.
 */
export function registerLuaLoader(rt: Runtime): void {
    const lib = getLib();
    const loaderPtr = lib.symbols.polyplug_lua_loader_create();
    if (loaderPtr === null) {
        throw new Error("polyplug: lua loader create failed");
    }
    rt.registerLoader(loaderPtr);
}
