/**
 * @file index.ts
 * @description Entry point for the polyplug FFI seam: runtime detection plus a
 * lazily-initialized singleton accessor.
 *
 * The SDK calls {@link getBackend} to obtain the one {@link FfiBackend} for the
 * current JS runtime. {@link detectBackend} chooses the implementation:
 * currently only Deno is supported; Node and Bun backends arrive in a later
 * increment behind this same interface.
 *
 * @module abi/ffi
 */

import { type FfiBackend } from "./backend.ts";
import { denoBackend } from "./deno.ts";

export {
    type FfiBackend,
    type FfiCallback,
    type FfiFunction,
    type FfiLibrary,
    type FfiPointerView,
    type FfiSymbolDef,
    type FfiSymbols,
    type FfiSymbolTable,
    FfiNotFoundError,
    type PolyPtr,
    type PolyPtrTyped,
} from "./backend.ts";

/**
 * Detect the FFI backend for the current JS runtime.
 *
 * Returns the Deno backend when running under Deno. Node and Bun are not yet
 * supported and throw a clear error.
 * @throws Error when the runtime is neither Deno (nor a later-added backend).
 */
export function detectBackend(): FfiBackend {
    if (typeof Deno !== "undefined") {
        return denoBackend;
    }
    throw new Error(
        "polyplug: unsupported JS runtime (Node and Bun support are coming in a later increment)",
    );
}

let _backend: FfiBackend | null = null;

/**
 * The lazily-initialized FFI backend singleton for the current runtime.
 *
 * Resolves {@link detectBackend} once on first use and caches it. This is the
 * accessor the SDK imports for all FFI work.
 */
export function getBackend(): FfiBackend {
    if (_backend === null) {
        _backend = detectBackend();
    }
    return _backend;
}
