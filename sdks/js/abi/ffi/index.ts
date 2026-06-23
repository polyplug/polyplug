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
 * The require specifier must resolve from BOTH the source tree (running `.ts`
 * directly under Node/Bun) AND the published `@polyplug/abi` package (where `tsc`
 * has transpiled `node.ts`/`bun.ts` into `node.js`/`bun.js` under `dist/`).
 * Because this lazy `require` is invisible to `tsc`'s static analysis, its
 * specifier is NOT rewritten by `rewriteRelativeImportExtensions`, so a literal
 * `"./node.ts"` 404s in `dist/` and a literal `"./node.js"` 404s in source.
 * {@link requireSibling} resolves whichever extension exists, trying `.js` (the
 * published shape) before `.ts` (the source shape).
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
 * Synchronously `require` a sibling backend module by its extensionless base
 * name, resolving whichever concrete file exists for the current layout.
 *
 * In the published `@polyplug/abi` package the backend is `dist/ffi/<base>.js`;
 * in the source tree it is `ffi/<base>.ts`. This helper tries `.js` first (the
 * published shape) and falls back to `.ts` (the source shape), so the SAME
 * `index` works in both. The `.ts` fallback only succeeds under Node/Bun's
 * native TypeScript loaders — it never runs under the transpiled package, where
 * the `.js` resolution always wins.
 * @throws Error when neither `<base>.js` nor `<base>.ts` can be resolved.
 */
function requireSibling(base: string): unknown {
    const require: (id: string) => unknown = createRequire(import.meta.url);
    let firstError: unknown;
    for (const ext of [".js", ".ts"] as const) {
        try {
            return require(`./${base}${ext}`);
        } catch (error) {
            if (firstError === undefined) {
                firstError = error;
            }
        }
    }
    throw new Error(
        `polyplug: could not load FFI backend "./${base}" as .js or .ts`,
        { cause: firstError },
    );
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
        // .ts module, and bun.ts has no top-level await. requireSibling resolves
        // the source (.ts) or published (.js) layout transparently.
        const mod: { bunBackend: FfiBackend } = requireSibling("bun") as {
            bunBackend: FfiBackend;
        };
        return mod.bunBackend;
    }
    if (isNode()) {
        // Load the koffi-backed Node backend synchronously, only under Node, so
        // Deno never has to resolve koffi (see file header). `require` of this
        // ESM module is permitted because node.ts has no top-level await.
        // requireSibling resolves the source (.ts) or published (.js) layout.
        const mod: { nodeBackend: FfiBackend } = requireSibling("node") as {
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
