/**
 * @file node.ts
 * @description Node.js implementation of the {@link FfiBackend} seam, backed by
 * the `koffi` C-FFI module.
 *
 * This is the ONLY Node file that imports `koffi` or touches `process` for FFI.
 * It mirrors `./deno.ts` method-for-method, but where Deno exposes a strongly
 * typed FFI surface that already speaks the seam's canonical token vocabulary,
 * koffi speaks C type strings and registered type handles — so this backend
 * carries a translation layer (see {@link toKoffiType}) that maps each canonical
 * token (`"pointer"`, `"u32"`, `"u64"`, `"void"`, `{ struct: [...] }`, …) onto
 * the koffi C type it corresponds to.
 *
 * Representation notes (koffi specifics that shape this file):
 * - A {@link PolyPtr} here is either `null` (the null pointer), a raw `bigint`
 *   address (from {@link NodeBackend.pointerOf} / {@link NodeBackend.pointerCreate}),
 *   or a koffi EXTERNAL pointer object returned by a native call that yields
 *   `void *`. koffi accepts a `bigint` where a `void *` arg is expected, but
 *   `koffi.decode` / `koffi.view` require an external — {@link toExternal} wraps a
 *   `bigint` into one.
 * - `koffi.decode(external, offset, type)` reads a scalar at an address and
 *   `koffi.view(external, len)` returns a LIVE (write-through) ArrayBuffer over
 *   native memory; together they back {@link FfiPointerView}.
 * - `koffi.register(fn, koffi.pointer(proto))` turns a JS function into a C
 *   callback pointer; {@link NodeBackend.makeCallback}'s `close()` calls
 *   `koffi.unregister`. Such a callback is meant to be invoked by NATIVE code;
 *   when the SDK drives one from JS, {@link NodeBackend.prepareFunction} routes to
 *   the JS function directly (re-entering koffi for it would crash).
 * - `koffi.decode(external, proto)` turns a raw function pointer into a callable
 *   JS function; this backs {@link NodeBackend.prepareFunction} for genuine native
 *   function pointers.
 * - By-value structs are registered once via `koffi.struct(...)` and cached by a
 *   structural key so the same shape reuses one koffi type.
 *
 * @module abi/ffi/node
 */

import koffiModule from "koffi";
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

/** A registered or primitive koffi type: an opaque handle or a C type string. */
type KoffiType = unknown;

/** A loaded koffi dynamic library: bound functions plus an unload. */
interface KoffiLib {
    func(
        name: string,
        result: KoffiType,
        params: readonly KoffiType[],
    ): (...args: unknown[]) => unknown;
    unload(): void;
}

/**
 * The exact subset of koffi's surface this backend uses, expressed in the
 * backend's own opaque token types ({@link KoffiType}, {@link PolyPtr}). This is
 * the single boundary that adapts koffi's strongly-typed module to the seam's
 * runtime-agnostic shapes — the same role the cast comment plays in `./deno.ts`.
 * The cast lives ONLY here by design.
 */
interface KoffiApi {
    load(path: string): KoffiLib;
    struct(fields: Record<string, KoffiType>): KoffiType;
    array(type: KoffiType, length: number, hint?: string): KoffiType;
    proto(name: string, result: KoffiType, params: readonly KoffiType[]): KoffiType;
    pointer(type: KoffiType): KoffiType;
    register(fn: (...args: unknown[]) => unknown, type: KoffiType): PolyPtr;
    unregister(callback: PolyPtr): void;
    decode(source: unknown, ...rest: unknown[]): unknown;
    address(value: unknown): bigint;
    view(value: unknown, length: number): ArrayBuffer;
}

const koffi: KoffiApi = koffiModule as unknown as KoffiApi;

/** A by-value struct token in the seam's canonical vocabulary. */
interface StructToken {
    readonly struct: readonly unknown[];
}

function isStructToken(token: unknown): token is StructToken {
    return typeof token === "object" && token !== null && "struct" in token &&
        Array.isArray((token as { struct: unknown }).struct);
}

// Canonical (Deno-style) scalar token → koffi C type string. Pointers map to
// `"void *"`; sized integers/floats map to their fixed-width C names; `usize` /
// `isize` map to koffi's pointer-sized integer aliases.
const SCALAR_TO_KOFFI: Readonly<Record<string, string>> = Object.freeze({
    void: "void",
    pointer: "void *",
    bool: "bool",
    u8: "uint8_t",
    u16: "uint16_t",
    u32: "uint32_t",
    u64: "uint64_t",
    i8: "int8_t",
    i16: "int16_t",
    i32: "int32_t",
    i64: "int64_t",
    f32: "float",
    f64: "double",
    usize: "size_t",
    isize: "ssize_t",
});

// Cache of registered koffi struct types, keyed by the structural signature of
// the canonical struct token. Stateless (type metadata only — no runtime or
// plugin state), so Rule 12 does not apply; it merely dedups koffi.struct calls.
const _structCache: Map<string, KoffiType> = new Map();

/** A JS function registered as a C callback, looked up by its callback address. */
interface JsCallbackEntry {
    readonly fn: (...args: unknown[]) => unknown;
}

// Live SDK-owned callbacks, keyed by their koffi callback address. This is FFI
// plumbing — the JS side of pointers the SDK itself created via makeCallback, so
// the backend can dispatch a JS-driven call to the function directly rather than
// re-entering koffi (which crashes for a registered callback). It holds no
// polyplug runtime or plugin state (Rule 12); entries are added by makeCallback
// and removed by the returned handle's close().
const _jsCallbacks: Map<bigint, JsCallbackEntry> = new Map();

function structKey(token: StructToken): string {
    return JSON.stringify(token.struct);
}

/**
 * Translate one canonical seam token into the koffi C type it denotes.
 *
 * Scalars resolve through {@link SCALAR_TO_KOFFI}; a `{ struct: [...] }` token is
 * registered (once, memoized) as an anonymous koffi struct whose fields are the
 * recursively translated member tokens.
 */
function toKoffiType(token: unknown): KoffiType {
    if (typeof token === "string") {
        const mapped: string | undefined = SCALAR_TO_KOFFI[token];
        if (mapped === undefined) {
            throw new Error(`polyplug node backend: unsupported FFI token "${token}"`);
        }
        return mapped;
    }
    if (isStructToken(token)) {
        const key: string = structKey(token);
        const cached: KoffiType | undefined = _structCache.get(key);
        if (cached !== undefined) {
            return cached;
        }
        const fields: Record<string, KoffiType> = {};
        token.struct.forEach((member: unknown, index: number) => {
            fields[`f${index}`] = toKoffiType(member);
        });
        const registered: KoffiType = koffi.struct(fields);
        _structCache.set(key, registered);
        return registered;
    }
    throw new Error(`polyplug node backend: unsupported FFI token ${String(token)}`);
}

/** Translate a seam function definition into the koffi (result, params) pair. */
function toKoffiSignature(
    definition: FfiSymbolDef,
): { result: KoffiType; parameters: KoffiType[] } {
    return {
        result: toKoffiType(definition.result),
        parameters: definition.parameters.map(toKoffiType),
    };
}

/** A koffi prototype handle for a function signature, memoized per signature. */
const _protoCache: Map<string, KoffiType> = new Map();
let _protoSeq = 0;

function protoFor(definition: FfiSymbolDef): KoffiType {
    const key: string = JSON.stringify({
        result: definition.result,
        parameters: definition.parameters,
    });
    const cached: KoffiType | undefined = _protoCache.get(key);
    if (cached !== undefined) {
        return cached;
    }
    const sig: { result: KoffiType; parameters: KoffiType[] } = toKoffiSignature(definition);
    // koffi.proto needs a unique name per distinct signature.
    const proto: KoffiType = koffi.proto(`polyplug_fn_${_protoSeq++}`, sig.result, sig.parameters);
    _protoCache.set(key, proto);
    return proto;
}

/**
 * Resolve a {@link PolyPtr} to its raw `bigint` address.
 *
 * A `PolyPtr` produced by this backend is either `null` (the null pointer), a
 * raw `bigint` address (from {@link NodeBackend.pointerOf} /
 * {@link NodeBackend.pointerCreate}), or a koffi external pointer object returned
 * by a native call that yields `void *`. `koffi.address` reads the address out
 * of an external; a bigint is already the address.
 */
function toAddress(pointer: PolyPtr): bigint {
    if (pointer === null) {
        return 0n;
    }
    if (typeof pointer === "bigint") {
        return pointer;
    }
    if (typeof pointer === "number") {
        return BigInt(pointer);
    }
    if (typeof pointer === "object") {
        return koffi.address(pointer);
    }
    throw new Error(
        `polyplug node backend: cannot resolve pointer of type ${typeof pointer}`,
    );
}

/**
 * Resolve a {@link PolyPtr} to a koffi EXTERNAL pointer object — the form
 * `koffi.decode` and a decoded function require (koffi rejects a raw `bigint`
 * source). A koffi object is already external; a `bigint` address is wrapped by
 * writing it into an 8-byte buffer and decoding that buffer as `void *`, which
 * yields the external pointing AT that address.
 */
function toExternal(pointer: PolyPtr): unknown {
    if (typeof pointer === "object" && pointer !== null) {
        return pointer;
    }
    const addr: bigint = toAddress(pointer);
    const holder: Buffer = Buffer.alloc(8);
    holder.writeBigUInt64LE(addr);
    return koffi.decode(holder, "void *");
}

/**
 * Marshal one JS argument into the form koffi wants for its declared parameter.
 *
 * The canonical `paramToken` drives the conversion (koffi is positional and
 * type-strict, unlike Deno's looser by-value-from-buffer acceptance):
 * - by-value STRUCT param + an `ArrayBufferView` arg → decode the view's bytes
 *   into the koffi struct object koffi requires for a by-value struct argument
 *   (Deno passes the raw buffer here; koffi does not). A non-view arg (an
 *   already-built struct object) passes through.
 * - any other param + `null` → `0n` (koffi's null pointer / zero scalar).
 * - any other param + an `ArrayBufferView` → a Node `Buffer` aliasing the same
 *   memory, so koffi reads/writes the caller's bytes in place (output pointers).
 * - everything else (bigint addresses, koffi externals, numbers) passes through;
 *   koffi accepts a bigint where a `void *` is expected.
 */
function marshalArg(arg: unknown, paramToken: unknown): unknown {
    if (isStructToken(paramToken)) {
        if (ArrayBuffer.isView(arg)) {
            const buf: Buffer = Buffer.from(arg.buffer, arg.byteOffset, arg.byteLength);
            return koffi.decode(buf, toKoffiType(paramToken));
        }
        return arg;
    }
    if (arg === null) {
        return 0n;
    }
    if (ArrayBuffer.isView(arg)) {
        return Buffer.from(arg.buffer, arg.byteOffset, arg.byteLength);
    }
    return arg;
}

function marshalArgs(args: readonly unknown[], params: readonly unknown[]): unknown[] {
    return args.map((arg: unknown, index: number) => marshalArg(arg, params[index]));
}

class NodePointerView implements FfiPointerView {
    readonly #address: bigint;
    readonly #external: unknown;

    constructor(pointer: PolyPtr) {
        this.#address = toAddress(pointer);
        this.#external = toExternal(pointer);
    }

    getArrayBuffer(byteLength: number, offset = 0): ArrayBuffer {
        // koffi.view returns a LIVE ArrayBuffer aliasing the native memory at the
        // pointer (zero-copy, write-through), mirroring the effective semantics
        // of Deno's UnsafePointerView.getArrayBuffer that the SDK relies on:
        // writing through the returned buffer reaches native memory (e.g. a host
        // contract's create_instance writes the instance id back through it).
        const base: unknown = offset === 0
            ? this.#external
            : toExternal(this.#address + BigInt(offset));
        return koffi.view(base, byteLength);
    }

    getUint32(offset = 0): number {
        return koffi.decode(this.#external, offset, "uint32_t") as number;
    }

    getBigUint64(offset = 0): bigint {
        // koffi may return a u64 as a JS `number` when it fits the safe-integer
        // range; the seam contract (mirroring Deno) is always a `bigint`.
        return BigInt(koffi.decode(this.#external, offset, "uint64_t") as number | bigint);
    }
}

/**
 * The koffi-backed {@link FfiBackend}. Stateless; a single shared instance is
 * exported as {@link nodeBackend}. All koffi/`process` access lives here.
 */
class NodeBackend implements FfiBackend {
    openLibrary<T extends FfiSymbolTable>(path: string, symbols: T): FfiLibrary<T> {
        const lib: KoffiLib = koffi.load(path);
        const bound: Record<string, (...args: unknown[]) => unknown> = {};
        for (const name of Object.keys(symbols)) {
            const definition: FfiSymbolDef = symbols[name];
            const sig: { result: KoffiType; parameters: KoffiType[] } = toKoffiSignature(
                definition,
            );
            const native: (...args: unknown[]) => unknown = lib.func(
                name,
                sig.result,
                sig.parameters,
            );
            const params: readonly unknown[] = definition.parameters;
            bound[name] = (...args: unknown[]): unknown => native(...marshalArgs(args, params));
        }
        return {
            symbols: bound as unknown as FfiSymbols<T>,
            close(): void {
                lib.unload();
            },
        };
    }

    pointerOf(buffer: ArrayBufferView): PolyPtr {
        // koffi.address yields the raw address of a Buffer/TypedArray's memory.
        const node: Buffer = Buffer.from(buffer.buffer, buffer.byteOffset, buffer.byteLength);
        return koffi.address(node);
    }

    pointerCreate(value: bigint): PolyPtr {
        return value === 0n ? null : value;
    }

    pointerValue(pointer: PolyPtr): bigint {
        return toAddress(pointer);
    }

    pointerView(pointer: PolyPtr): FfiPointerView {
        return new NodePointerView(pointer);
    }

    makeCallback(definition: FfiSymbolDef, fn: (...args: never[]) => unknown): FfiCallback {
        const proto: KoffiType = protoFor(definition);
        // koffi.register returns an external callback pointer that native code
        // can invoke; it is a valid PolyPtr (an external object). The owner must
        // keep this handle reachable until close() (which calls unregister).
        const registered: PolyPtr = koffi.register(
            fn as (...args: unknown[]) => unknown,
            koffi.pointer(proto),
        );
        // Record the JS function by its callback address. A koffi-registered
        // callback is a trampoline meant to be invoked by NATIVE code; invoking
        // it back from JS through a koffi-decoded function pointer crashes (the
        // two calling paths are not interchangeable in koffi). When the SDK
        // itself drives one of these pointers (e.g. a host driving its own
        // host-contract dispatch), prepareFunction dispatches to the JS function
        // directly instead — see {@link _jsCallbacks}.
        const address: bigint = toAddress(registered);
        _jsCallbacks.set(address, { fn: fn as (...args: unknown[]) => unknown });
        return {
            pointer: registered,
            close(): void {
                _jsCallbacks.delete(address);
                koffi.unregister(registered);
            },
        };
    }

    prepareFunction(pointer: PolyPtr, definition: FfiSymbolDef): FfiFunction {
        const params: readonly unknown[] = definition.parameters;
        // If this pointer is one of our own registered JS callbacks, invoke the
        // JS function directly (calling it through koffi from JS would crash).
        // The args are passed through UNCHANGED: a JS callback authored against
        // the seam expects the seam's own shapes (a by-value struct as the raw
        // ArrayBufferView, a pointer as a bigint/null), which is exactly what the
        // SDK caller supplies — no koffi-direction marshalling applies here.
        const jsEntry: JsCallbackEntry | undefined = _jsCallbacks.get(toAddress(pointer));
        if (jsEntry !== undefined) {
            const targetFn: (...args: unknown[]) => unknown = jsEntry.fn;
            return {
                call(...args: unknown[]): unknown {
                    return targetFn(...args);
                },
            };
        }
        const proto: KoffiType = protoFor(definition);
        // koffi.decode(external, proto) yields a directly-callable JS function
        // bound to the raw native function pointer (the koffi analogue of
        // Deno.UnsafeFnPointer).
        const callable: (...args: unknown[]) => unknown = koffi.decode(
            toExternal(pointer),
            proto,
        ) as (...args: unknown[]) => unknown;
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
        // Normalize Node's process.platform to the same identifiers Deno reports
        // (Deno.build.os): "darwin"/"windows"/"linux".
        const p: string = process.platform;
        if (p === "win32") {
            return "windows";
        }
        return p;
    }

    arch(): string {
        // Normalize Node's process.arch to Deno.build.arch identifiers.
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

/** The shared Node backend instance. */
export const nodeBackend: FfiBackend = new NodeBackend();
