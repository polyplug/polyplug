/**
 * @file index.ts
 * @description Entry point for the polyplug FFI seam: runtime detection plus a
 * lazily-initialized singleton accessor.
 *
 * The SDK calls {@link getBackend} to obtain the one {@link FfiBackend} for the
 * current JS runtime. {@link detectBackend} chooses the implementation: Deno,
 * Node, and Bun are all supported behind this same interface.
 *
 * Backend loading is split so each runtime only pulls in code it can resolve.
 * `./deno.ts` is statically imported — it touches `Deno.*` only inside methods,
 * so its module graph loads cleanly under every runtime. `./node.ts` imports the
 * npm-only `koffi` package at module scope, which Deno cannot resolve, and
 * `./bun.ts` imports the Bun-only `bun:ffi` builtin, which neither Deno, Node,
 * nor `tsc` can resolve — so each is loaded ONLY on its own runtime branch via a
 * synchronous `require` (a static top-level import of either would poison the
 * module graph for every other runtime, and break the npm `tsc` build). This
 * keeps {@link getBackend} synchronous — the SDK resolves it at module-init time.
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
 * Returns the Deno backend under Deno, the `bun:ffi`-backed Bun backend under
 * Bun, and the koffi-backed Node backend under Node. Any other runtime throws a
 * clear error.
 * @throws Error when the runtime is unrecognised (not Deno, Bun, or Node).
 */
export function detectBackend(): FfiBackend {
    if (typeof Deno !== "undefined") {
        return denoBackend;
    }
    if (isBun()) {
        // Load the bun:ffi-backed Bun backend synchronously, only under Bun, so
        // neither Deno, Node, nor tsc has to resolve the bun:ffi builtin (see
        // file header). Bun's createRequire supports a synchronous require of a
        // .ts module, and bun.ts has no top-level await.
        const require: (id: string) => unknown = createRequire(import.meta.url);
        const mod: { bunBackend: FfiBackend } = require("./bun.ts") as {
            bunBackend: FfiBackend;
        };
        return mod.bunBackend;
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
        "polyplug: unsupported JS runtime (expected Deno, Bun, or Node)",
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
