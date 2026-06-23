/**
 * @file bun.ts
 * @description Bun implementation of the {@link FfiBackend} seam, backed by the
 * built-in `bun:ffi` module.
 *
 * This is the ONLY Bun file that imports `bun:ffi` or touches `process` for FFI.
 * It mirrors `./node.ts` method-for-method, but where koffi speaks C type
 * strings and registered handles, `bun:ffi` speaks its own `FFIType` string
 * tokens — so this backend carries a translation layer (see {@link toBunType})
 * that maps each canonical seam token (`"pointer"`, `"u32"`, `"u64"`, `"void"`,
 * `{ struct: [...] }`, …) onto the `bun:ffi` `FFIType` it corresponds to.
 *
 * Representation notes (`bun:ffi` specifics that shape this file):
 * - A {@link PolyPtr} here is either `null` (the null pointer) or a JS `number`
 *   address — `bun:ffi` represents native pointers as numbers, NOT bigints
 *   (52 addressable bits fit in a double), so {@link BunBackend.pointerOf} /
 *   {@link BunBackend.pointerCreate} yield a `number` and {@link BunBackend.pointerValue}
 *   widens it back to the seam's `bigint` contract.
 * - `dlopen(path, symbols)` opens a library and binds its symbols; each symbol's
 *   signature is `{ args: [...], returns: ... }` (this backend translates the
 *   seam's `{ parameters, result }` into that shape).
 * - `read.u32(ptr, off)` / `read.u64(ptr, off)` read a scalar at an address and
 *   `toArrayBuffer(ptr, off, len)` returns a LIVE (write-through) ArrayBuffer
 *   over native memory; together they back {@link FfiPointerView}.
 * - `new JSCallback(fn, { args, returns })` turns a JS function into a C callback
 *   pointer exposed as `.ptr`; {@link BunBackend.makeCallback}'s `close()` calls
 *   `JSCallback.close()`.
 * - `new CFunction({ ptr, args, returns })` turns a raw native function pointer
 *   into a callable JS function; this backs {@link BunBackend.prepareFunction}.
 *   Unlike koffi, `bun:ffi` invokes an SDK-owned `JSCallback` pointer correctly
 *   through `CFunction` even when the SDK itself drives the call from JS
 *   (empirically verified), so this backend needs NO callback short-circuit map.
 * - `bun:ffi` has NO by-value struct support: a `{ struct: [...] }` token is
 *   rejected by `dlopen`/`CFunction`. The seam's only by-value struct is the
 *   single-pointer `HostContractInstance { data: void* }`; a one-eightbyte
 *   INTEGER-class struct is passed IDENTICALLY to a bare `void*` under the SysV
 *   AMD64 and AArch64 calling conventions, so this backend maps a single-pointer
 *   struct token to `"ptr"` and marshals such an argument by reading its one
 *   pointer field out of the caller's buffer (see {@link marshalArg}). A
 *   multi-field by-value struct is unsupported and rejected explicitly.
 *
 * @module abi/ffi/bun
 */

import { CFunction, dlopen, JSCallback, ptr, read, toArrayBuffer } from "bun:ffi";
import { statSync } from "node:fs";

import {
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
} from "./backend.ts";

/** A `bun:ffi` `FFIType` token: one of the string names accepted by `dlopen`. */
type BunType = string;

/**
 * The exact subset of `bun:ffi`'s surface this backend uses, expressed in the
 * backend's own opaque token types ({@link BunType}, {@link PolyPtr}). This is the
 * single boundary that adapts `bun:ffi`'s typed module to the seam's
 * runtime-agnostic shapes — the same role the cast comment plays in `./deno.ts`
 * and the `KoffiApi` interface plays in `./node.ts`. The casts live ONLY here by
 * design.
 */
interface BunFfiApi {
    dlopen(
        path: string,
        symbols: Record<string, { args: readonly BunType[]; returns: BunType }>,
    ): {
        symbols: Record<string, (...args: unknown[]) => unknown>;
        close(): void;
    };
    JSCallback: new (
        fn: (...args: unknown[]) => unknown,
        definition: { args: readonly BunType[]; returns: BunType },
    ) => { readonly ptr: number | null; close(): void };
    CFunction: new (
        definition: { ptr: number | null; args: readonly BunType[]; returns: BunType },
    ) => ((...args: unknown[]) => unknown) & { close(): void };
    ptr(view: ArrayBufferView): number;
    read: {
        ptr(source: number, offset: number): number;
        u32(source: number, offset: number): number;
        u64(source: number, offset: number): bigint;
    };
    toArrayBuffer(source: number, byteOffset: number, byteLength: number): ArrayBuffer;
}

const bun: BunFfiApi = {
    dlopen: dlopen as unknown as BunFfiApi["dlopen"],
    JSCallback: JSCallback as unknown as BunFfiApi["JSCallback"],
    CFunction: CFunction as unknown as BunFfiApi["CFunction"],
    ptr: ptr as unknown as BunFfiApi["ptr"],
    read: read as unknown as BunFfiApi["read"],
    toArrayBuffer: toArrayBuffer as unknown as BunFfiApi["toArrayBuffer"],
};

/** A by-value struct token in the seam's canonical vocabulary. */
interface StructToken {
    readonly struct: readonly unknown[];
}

function isStructToken(token: unknown): token is StructToken {
    return typeof token === "object" && token !== null && "struct" in token &&
        Array.isArray((token as { struct: unknown }).struct);
}

// Canonical (Deno-style) scalar token → `bun:ffi` FFIType name. `"pointer"` maps
// to `bun:ffi`'s `"ptr"`; sized integers/floats keep their canonical names
// (which `bun:ffi` accepts verbatim); `usize` / `isize` are pointer-sized
// integers, accepted by `bun:ffi` directly.
const SCALAR_TO_BUN: Readonly<Record<string, BunType>> = Object.freeze({
    void: "void",
    pointer: "ptr",
    bool: "bool",
    u8: "u8",
    u16: "u16",
    u32: "u32",
    u64: "u64",
    i8: "i8",
    i16: "i16",
    i32: "i32",
    i64: "i64",
    f32: "f32",
    f64: "f64",
    usize: "usize",
    isize: "isize",
});

/**
 * Translate one canonical seam token into the `bun:ffi` FFIType it denotes.
 *
 * Scalars resolve through {@link SCALAR_TO_BUN}. A `{ struct: [...] }` token has
 * no `bun:ffi` equivalent; the seam's only by-value struct is a single pointer
 * field, which is ABI-identical to a bare `void*`, so a one-field pointer struct
 * resolves to `"ptr"`. A multi-field struct is unsupported.
 */
function toBunType(token: unknown): BunType {
    if (typeof token === "string") {
        const mapped: BunType | undefined = SCALAR_TO_BUN[token];
        if (mapped === undefined) {
            throw new Error(`polyplug bun backend: unsupported FFI token "${token}"`);
        }
        return mapped;
    }
    if (isStructToken(token)) {
        if (token.struct.length !== 1 || toBunType(token.struct[0]) !== "ptr") {
            throw new Error(
                "polyplug bun backend: only a single-pointer by-value struct is " +
                    "supported (bun:ffi has no by-value struct types); " +
                    `got ${JSON.stringify(token.struct)}`,
            );
        }
        return "ptr";
    }
    throw new Error(`polyplug bun backend: unsupported FFI token ${String(token)}`);
}

/** Translate a seam function definition into the `bun:ffi` (args, returns) pair. */
function toBunSignature(
    definition: FfiSymbolDef,
): { args: BunType[]; returns: BunType } {
    return {
        args: definition.parameters.map(toBunType),
        returns: toBunType(definition.result),
    };
}

/**
 * Marshal one JS argument into the form `bun:ffi` wants for its declared
 * parameter, driven by the canonical `paramToken`:
 * - a single-pointer by-value STRUCT param + an `ArrayBufferView` arg → read the
 *   one pointer field out of the buffer and pass it as a `ptr` value. The
 *   translated param is `"ptr"` (a real pointer arg), so passing the buffer
 *   itself would hand over the buffer's ADDRESS; the by-value semantics require
 *   the field's CONTENTS instead.
 * - everything else passes through: `bun:ffi` accepts `null` for a `ptr` param,
 *   takes an `ArrayBufferView` as a pointer to its memory (output pointers), and
 *   accepts a `number` address verbatim.
 */
function marshalArg(arg: unknown, paramToken: unknown): unknown {
    if (isStructToken(paramToken) && ArrayBuffer.isView(arg)) {
        return bun.read.ptr(bun.ptr(arg), 0);
    }
    return arg;
}

function marshalArgs(args: readonly unknown[], params: readonly unknown[]): unknown[] {
    return args.map((arg: unknown, index: number) => marshalArg(arg, params[index]));
}

/**
 * Unmarshal one argument `bun:ffi` delivers to a JS callback into the shape the
 * seam's callback handlers expect, driven by the canonical `paramToken`.
 *
 * The inverse of {@link marshalArg}'s struct case: a single-pointer by-value
 * struct param is translated to `"ptr"`, so when native code invokes the
 * callback `bun:ffi` delivers that struct's eight bytes AS a pointer `number`.
 * The seam's handlers (e.g. the host-contract dispatch/destroy callbacks) author
 * a by-value struct argument as the raw bytes (an `ArrayBufferView`, the form
 * Deno hands a callback), so this packs the delivered pointer value back into an
 * 8-byte view carrying those exact bytes. Every other argument passes through.
 */
function unmarshalCallbackArg(arg: unknown, paramToken: unknown): unknown {
    if (isStructToken(paramToken)) {
        const view: Uint8Array = new Uint8Array(8);
        new DataView(view.buffer).setBigUint64(0, BigInt(toNumberAddress(arg as PolyPtr)), true);
        return view;
    }
    return arg;
}

function unmarshalCallbackArgs(args: readonly unknown[], params: readonly unknown[]): unknown[] {
    return args.map((arg: unknown, index: number) => unmarshalCallbackArg(arg, params[index]));
}

/**
 * Resolve a {@link PolyPtr} to the `number` address `bun:ffi` reads from. A
 * `PolyPtr` from this backend is either `null` (→ `0`) or already a `number`.
 */
function toNumberAddress(pointer: PolyPtr): number {
    if (pointer === null) {
        return 0;
    }
    if (typeof pointer === "number") {
        return pointer;
    }
    if (typeof pointer === "bigint") {
        return Number(pointer);
    }
    throw new Error(
        `polyplug bun backend: cannot resolve pointer of type ${typeof pointer}`,
    );
}

class BunPointerView implements FfiPointerView {
    readonly #address: number;

    constructor(pointer: PolyPtr) {
        this.#address = toNumberAddress(pointer);
    }

    getArrayBuffer(byteLength: number, offset = 0): ArrayBuffer {
        // toArrayBuffer returns a LIVE ArrayBuffer aliasing the native memory at
        // the pointer (zero-copy, write-through), mirroring the effective
        // semantics of Deno's UnsafePointerView.getArrayBuffer that the SDK
        // relies on: writing through the returned buffer reaches native memory
        // (e.g. a host contract's create_instance writes the instance id back
        // through it).
        return bun.toArrayBuffer(this.#address, offset, byteLength);
    }

    getUint32(offset = 0): number {
        return bun.read.u32(this.#address, offset);
    }

    getBigUint64(offset = 0): bigint {
        return bun.read.u64(this.#address, offset);
    }
}

/**
 * The `bun:ffi`-backed {@link FfiBackend}. Stateless; a single shared instance is
 * exported as {@link bunBackend}. All `bun:ffi`/`process` access lives here.
 */
class BunBackend implements FfiBackend {
    openLibrary<T extends FfiSymbolTable>(path: string, symbols: T): FfiLibrary<T> {
        const defs: Record<string, { args: BunType[]; returns: BunType }> = {};
        const paramsByName: Record<string, readonly unknown[]> = {};
        for (const name of Object.keys(symbols)) {
            const definition: FfiSymbolDef = symbols[name];
            defs[name] = toBunSignature(definition);
            paramsByName[name] = definition.parameters;
        }
        const lib: {
            symbols: Record<string, (...args: unknown[]) => unknown>;
            close(): void;
        } = bun.dlopen(path, defs);
        const bound: Record<string, (...args: unknown[]) => unknown> = {};
        for (const name of Object.keys(symbols)) {
            const native: (...args: unknown[]) => unknown = lib.symbols[name];
            const params: readonly unknown[] = paramsByName[name];
            bound[name] = (...args: unknown[]): unknown => native(...marshalArgs(args, params));
        }
        return {
            symbols: bound as unknown as FfiSymbols<T>,
            close(): void {
                lib.close();
            },
        };
    }

    pointerOf(buffer: ArrayBufferView): PolyPtr {
        // ptr() yields the raw numeric address of a TypedArray's memory.
        return bun.ptr(buffer);
    }

    pointerCreate(value: bigint): PolyPtr {
        return value === 0n ? null : Number(value);
    }

    pointerValue(pointer: PolyPtr): bigint {
        return BigInt(toNumberAddress(pointer));
    }

    pointerView(pointer: PolyPtr): FfiPointerView {
        return new BunPointerView(pointer);
    }

    makeCallback(definition: FfiSymbolDef, fn: (...args: never[]) => unknown): FfiCallback {
        const sig: { args: BunType[]; returns: BunType } = toBunSignature(definition);
        const params: readonly unknown[] = definition.parameters;
        const handler: (...args: unknown[]) => unknown = fn as (...args: unknown[]) => unknown;
        // JSCallback wraps the JS function as a C callback pointer that native
        // code can invoke; `.ptr` is a valid PolyPtr (a numeric address). The
        // owner must keep this handle reachable until close(). The arguments
        // bun:ffi delivers are unmarshalled into the seam's callback shapes
        // (a single-pointer by-value struct arrives as a pointer number under the
        // `ptr` translation; the seam handler expects the raw 8 bytes).
        const callback: { readonly ptr: number | null; close(): void } = new bun.JSCallback(
            (...args: unknown[]): unknown => handler(...unmarshalCallbackArgs(args, params)),
            sig,
        );
        return {
            pointer: callback.ptr,
            close(): void {
                callback.close();
            },
        };
    }

    prepareFunction(pointer: PolyPtr, definition: FfiSymbolDef): FfiFunction {
        const params: readonly unknown[] = definition.parameters;
        const sig: { args: BunType[]; returns: BunType } = toBunSignature(definition);
        // CFunction yields a directly-callable JS function bound to the raw
        // native function pointer (the bun:ffi analogue of Deno.UnsafeFnPointer).
        // It correctly invokes an SDK-owned JSCallback pointer driven from JS, so
        // unlike the koffi backend no JS-callback short-circuit is needed.
        const callable: (...args: unknown[]) => unknown = new bun.CFunction({
            ptr: toNumberAddress(pointer),
            args: sig.args,
            returns: sig.returns,
        });
        return {
            call(...args: unknown[]): unknown {
                return callable(...marshalArgs(args, params));
            },
        };
    }

    callFunction(pointer: PolyPtr, definition: FfiSymbolDef, args: readonly unknown[]): unknown {
        return this.prepareFunction(pointer, definition).call(...args);
    }

    platform(): string {
        // Normalize Bun's process.platform to the same identifiers Deno reports
        // (Deno.build.os): "darwin"/"windows"/"linux".
        const p: string = process.platform;
        if (p === "win32") {
            return "windows";
        }
        return p;
    }

    arch(): string {
        // Normalize Bun's process.arch to Deno.build.arch identifiers.
        const a: string = process.arch;
        if (a === "x64") {
            return "x86_64";
        }
        if (a === "arm64") {
            return "aarch64";
        }
        return a;
    }

    env(name: string): string | undefined {
        return process.env[name];
    }

    statSync(path: string | URL): void {
        try {
            statSync(path);
        } catch (error) {
            if (
                typeof error === "object" && error !== null &&
                (error as { code?: string }).code === "ENOENT"
            ) {
                throw new FfiNotFoundError(`path not found: ${String(path)}`, { cause: error });
            }
            throw error;
        }
    }
}

/** The shared Bun backend instance. */
export const bunBackend: FfiBackend = new BunBackend();
