/**
 * @file backend.ts
 * @description The single FFI abstraction seam for the polyplug JS/TS SDK.
 *
 * Every native-FFI primitive the SDK needs crosses through the {@link FfiBackend}
 * interface — dynamic-library open, pointer conversions, typed memory reads,
 * JS-to-C callbacks, raw-function-pointer calls, and platform/env queries. A
 * concrete backend (see `./deno.ts`) supplies one runtime's implementation; the
 * SDK never touches a runtime-global (e.g. `Deno.*`) directly.
 *
 * This is the stable contract: the data/ABI layout it carries does NOT change
 * between backends — only the implementation does. Node and Bun backends will
 * implement this same interface in a later increment.
 *
 * @module abi/ffi/backend
 */

/**
 * An opaque native pointer value.
 *
 * Mirrors Deno's `Deno.PointerValue`: a non-null pointer is an opaque object,
 * and `null` represents the null pointer. Consumers MUST treat it as opaque and
 * only move it across {@link FfiBackend} methods — never deref or arithmetic.
 */
export type PolyPtr = unknown | null;

/**
 * A typed native pointer value, parameterized by the foreign-function signature
 * it points at. Mirrors Deno's `Deno.PointerObject<T>`. The type parameter is a
 * compile-time tag only; at runtime it is the same opaque pointer as
 * {@link PolyPtr}.
 */
export type PolyPtrTyped<T extends FfiSymbolDef = FfiSymbolDef> = PolyPtr & {
    readonly __ffiSignature?: T;
};

/**
 * A single foreign-function signature: its parameter types and result type.
 *
 * The element types are the seam's CANONICAL FFI type tokens — the Deno-style
 * vocabulary: `"pointer"`, `"void"`, `"u8".."u64"`, `"i8".."i64"`, `"f32"` /
 * `"f64"`, `"usize"` / `"isize"`, `"bool"`, and by-value structs written as a
 * descriptor `{ struct: [...] }`. Call sites and tests author these tokens
 * directly. The Deno backend consumes them as-is (they are Deno's own FFI
 * tokens); every non-Deno backend (Node, Bun) TRANSLATES each canonical token
 * to its native FFI type. The seam itself passes them through opaquely and does
 * not re-validate them.
 */
export interface FfiSymbolDef {
    readonly parameters: readonly unknown[];
    readonly result: unknown;
}

/**
 * A map of exported symbol name to its foreign-function signature, used to open
 * a dynamic library. Mirrors Deno's `Deno.ForeignLibraryInterface`.
 */
export type FfiSymbolTable = Record<string, FfiSymbolDef>;

/**
 * The callable symbols of an opened dynamic library.
 *
 * Each entry is a function that invokes the named native symbol; arguments and
 * return value follow that symbol's {@link FfiSymbolDef}. Mirrors the
 * `.symbols` object of a `Deno.DynamicLibrary`.
 */
export type FfiSymbols<T extends FfiSymbolTable> = {
    [K in keyof T]: (...args: unknown[]) => unknown;
};

/**
 * An opened dynamic library handle. Mirrors `Deno.DynamicLibrary<T>`: exposes
 * the callable `symbols` and a `close()` that unloads the library.
 */
export interface FfiLibrary<T extends FfiSymbolTable = FfiSymbolTable> {
    readonly symbols: FfiSymbols<T>;
    close(): void;
}

/**
 * A read-only typed view over native memory at a pointer. Mirrors the subset of
 * `Deno.UnsafePointerView` the SDK uses: length-bounded byte copy plus typed
 * scalar reads at a byte offset.
 */
export interface FfiPointerView {
    /** Copy `byteLength` bytes starting at `offset` into a fresh ArrayBuffer. */
    getArrayBuffer(byteLength: number, offset?: number): ArrayBuffer;
    /** Read a little-endian `u32` at `offset`. */
    getUint32(offset?: number): number;
    /** Read a little-endian `u64` at `offset` as a bigint. */
    getBigUint64(offset?: number): bigint;
}

/**
 * A JS function wrapped as a C-callable function pointer. Mirrors
 * `Deno.UnsafeCallback`: `pointer` is the address handed to native code, and
 * `close()` releases it. The owner MUST keep the handle alive (and reachable)
 * for as long as native code may invoke it, then call `close()`.
 */
export interface FfiCallback {
    readonly pointer: PolyPtr;
    close(): void;
}

/**
 * A reusable, pre-bound callable over a raw native function pointer. Mirrors a
 * `Deno.UnsafeFnPointer` instance: built once for a (pointer, signature) pair
 * and invoked many times, so a hot dispatch slot can cache it instead of
 * re-wrapping the pointer on every call.
 */
export interface FfiFunction {
    call(...args: unknown[]): unknown;
}

/**
 * The FFI abstraction seam. A backend implements every primitive the SDK needs
 * to talk to native polyplug; the SDK depends only on this interface.
 */
export interface FfiBackend {
    /**
     * Open a dynamic library at `path` and bind the given `symbols`.
     * Mirrors `Deno.dlopen`.
     */
    openLibrary<T extends FfiSymbolTable>(path: string, symbols: T): FfiLibrary<T>;

    /**
     * Wrap a JS-owned buffer's memory as a native pointer (valid while the
     * buffer is reachable). Mirrors `Deno.UnsafePointer.of`.
     */
    pointerOf(buffer: ArrayBufferView): PolyPtr;

    /**
     * Build a native pointer from a raw 64-bit address; `0n` yields the null
     * pointer. Mirrors `Deno.UnsafePointer.create`.
     */
    pointerCreate(value: bigint): PolyPtr;

    /**
     * Read a native pointer back as its raw 64-bit address; the null pointer
     * yields `0n`. Mirrors `Deno.UnsafePointer.value` (normalizing its
     * `null`/`0n` result to `0n`).
     */
    pointerValue(pointer: PolyPtr): bigint;

    /**
     * Create a read-only typed view over the memory at `pointer`.
     * Mirrors `new Deno.UnsafePointerView(pointer)`.
     */
    pointerView(pointer: PolyPtr): FfiPointerView;

    /**
     * Wrap a JS function as a C-callable function pointer with the given
     * signature. Mirrors `new Deno.UnsafeCallback(definition, fn)`.
     */
    makeCallback(definition: FfiSymbolDef, fn: (...args: never[]) => unknown): FfiCallback;

    /**
     * Build a reusable callable for a raw native function pointer. Mirrors
     * `new Deno.UnsafeFnPointer(pointer, definition)`. Use this when the same
     * (pointer, signature) is invoked repeatedly (e.g. a cached dispatch slot);
     * for a one-shot call prefer {@link FfiBackend.callFunction}.
     */
    prepareFunction(pointer: PolyPtr, definition: FfiSymbolDef): FfiFunction;

    /**
     * Call a raw native function pointer once with the given signature and
     * arguments. Mirrors `new Deno.UnsafeFnPointer(pointer, definition).call(...args)`.
     */
    callFunction(pointer: PolyPtr, definition: FfiSymbolDef, args: readonly unknown[]): unknown;

    /** Operating-system identifier (e.g. "linux", "darwin", "windows"). Mirrors `Deno.build.os`. */
    platform(): string;

    /** CPU architecture identifier (e.g. "x86_64", "aarch64"). Mirrors `Deno.build.arch`. */
    arch(): string;

    /** Read an environment variable, or `undefined` when unset. Mirrors `Deno.env.get`. */
    env(name: string): string | undefined;

    /**
     * Throw if no readable file exists at `path` (a URL or a filesystem path).
     * The error is a {@link FfiNotFoundError} when the path does not exist, so
     * callers can distinguish "missing" from other I/O failures without
     * referencing a runtime-specific error class. Mirrors `Deno.statSync` +
     * `Deno.errors.NotFound`.
     */
    statSync(path: string | URL): void;
}

/**
 * Error thrown by {@link FfiBackend.statSync} when the target path does not
 * exist. Backends translate their runtime's not-found error into this type so
 * the SDK never has to test for a runtime-specific error class.
 */
export class FfiNotFoundError extends Error {
    constructor(message: string, options?: ErrorOptions) {
        super(message, options);
        this.name = "FfiNotFoundError";
    }
}
