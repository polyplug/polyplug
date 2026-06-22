/**
 * @file index.ts
 * @description Entry point for the polyplug FFI seam: runtime detection plus a
 * lazily-initialized singleton accessor.
 *
 * The SDK calls {@link getBackend} to obtain the one {@link FfiBackend} for the
 * current JS runtime. {@link detectBackend} chooses the implementation: Deno and
 * Node are supported; Bun arrives in a later increment behind this same
 * interface.
 *
 * Backend loading is split so each runtime only pulls in code it can resolve.
 * `./deno.ts` is statically imported — it touches `Deno.*` only inside methods,
 * so its module graph loads cleanly under every runtime. `./node.ts` imports the
 * npm-only `koffi` package at module scope, which Deno cannot resolve, so it is
 * loaded ONLY on the Node branch via a synchronous `require` (Node permits
 * `require()` of an ESM module with no top-level await). This keeps
 * {@link getBackend} synchronous — the SDK resolves it at module-init time.
 *
 * @module abi/ffi
 */

import { createRequire } from "node:module";

import { type FfiBackend } from "./backend.ts";
import { denoBackend } from "./deno.ts";

export {
    type FfiBackend,
    type FfiCallback,
    type FfiFunction,
    type FfiLibrary,
    FfiNotFoundError,
    type FfiPointerView,
    type FfiSymbolDef,
    type FfiSymbols,
    type FfiSymbolTable,
    type PolyPtr,
    type PolyPtrTyped,
} from "./backend.ts";

// Bun defines a `Bun` global; Node defines `process.versions.node`. Bun also
// defines `process`, so Bun MUST be checked before Node.
function isBun(): boolean {
    return typeof (globalThis as { Bun?: unknown }).Bun !== "undefined";
}

function isNode(): boolean {
    const proc: { versions?: { node?: string } } | undefined =
        (globalThis as { process?: { versions?: { node?: string } } }).process;
    return typeof proc?.versions?.node === "string";
}

/**
 * Detect the FFI backend for the current JS runtime.
 *
 * Returns the Deno backend under Deno and the koffi-backed Node backend under
 * Node. Bun is not yet supported and throws a clear error, as does any other
 * runtime.
 * @throws Error when the runtime is Bun (not yet supported) or unrecognised.
 */
export function detectBackend(): FfiBackend {
    if (typeof Deno !== "undefined") {
        return denoBackend;
    }
    if (isBun()) {
        throw new Error(
            "polyplug: Bun FFI backend is coming in a later increment",
        );
    }
    if (isNode()) {
        // Load the koffi-backed Node backend synchronously, only under Node, so
        // Deno never has to resolve koffi (see file header). `require` of this
        // ESM module is permitted because node.ts has no top-level await.
        const require: (id: string) => unknown = createRequire(import.meta.url);
        const mod: { nodeBackend: FfiBackend } = require("./node.ts") as {
            nodeBackend: FfiBackend;
        };
        return mod.nodeBackend;
    }
    throw new Error(
        "polyplug: unsupported JS runtime (expected Deno or Node; Bun support is coming in a later increment)",
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
