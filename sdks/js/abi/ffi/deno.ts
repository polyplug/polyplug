/**
 * @file deno.ts
 * @description Deno implementation of the {@link FfiBackend} seam.
 *
 * A faithful 1:1 delegation to the real `Deno.*` FFI APIs — the only file in the
 * SDK that touches `Deno.*`. Every method maps directly onto the Deno primitive
 * it documents in `./backend.ts`, with no behavioural change from the original
 * inline call sites.
 *
 * @module abi/ffi/deno
 */

import {
    type FfiBackend,
    type FfiCallback,
    type FfiFunction,
    type FfiLibrary,
    type FfiPointerView,
    type FfiSymbolDef,
    type FfiSymbolTable,
    FfiNotFoundError,
    type PolyPtr,
} from "./backend.ts";

/**
 * The Deno-backed {@link FfiBackend}. Stateless; a single shared instance is
 * exported as {@link denoBackend}.
 */
// This is the one boundary that adapts Deno's strongly-typed FFI surface to the
// seam's runtime-agnostic opaque types (PolyPtr / FfiSymbolDef). The casts below
// live ONLY here by design: the seam keeps the contract opaque, the backend
// re-asserts the concrete Deno types its APIs require.
class DenoBackend implements FfiBackend {
    openLibrary<T extends FfiSymbolTable>(path: string, symbols: T): FfiLibrary<T> {
        return Deno.dlopen(
            path,
            symbols as unknown as Deno.ForeignLibraryInterface,
        ) as unknown as FfiLibrary<T>;
    }

    pointerOf(buffer: ArrayBufferView): PolyPtr {
        return Deno.UnsafePointer.of(buffer);
    }

    pointerCreate(value: bigint): PolyPtr {
        return Deno.UnsafePointer.create(value);
    }

    pointerValue(pointer: PolyPtr): bigint {
        const raw: bigint | null = Deno.UnsafePointer.value(pointer as Deno.PointerValue);
        return raw === null ? 0n : BigInt(raw);
    }

    pointerView(pointer: PolyPtr): FfiPointerView {
        return new Deno.UnsafePointerView(pointer as Deno.PointerObject);
    }

    makeCallback(definition: FfiSymbolDef, fn: (...args: never[]) => unknown): FfiCallback {
        return new Deno.UnsafeCallback(
            definition as unknown as Deno.UnsafeCallbackDefinition,
            fn as Deno.UnsafeCallbackFunction,
        );
    }

    prepareFunction(pointer: PolyPtr, definition: FfiSymbolDef): FfiFunction {
        const fnDef: Deno.ForeignFunction = definition as unknown as Deno.ForeignFunction;
        return new Deno.UnsafeFnPointer(
            pointer as unknown as Deno.PointerObject<Deno.ForeignFunction>,
            fnDef,
        );
    }

    callFunction(pointer: PolyPtr, definition: FfiSymbolDef, args: readonly unknown[]): unknown {
        return this.prepareFunction(pointer, definition).call(...args);
    }

    platform(): string {
        return Deno.build.os;
    }

    arch(): string {
        return Deno.build.arch;
    }

    env(name: string): string | undefined {
        return Deno.env.get(name);
    }

    statSync(path: string | URL): void {
        try {
            Deno.statSync(path);
        } catch (error) {
            if (error instanceof Deno.errors.NotFound) {
                throw new FfiNotFoundError(`path not found: ${String(path)}`, { cause: error });
            }
            throw error;
        }
    }
}

/** The shared Deno backend instance. */
export const denoBackend: FfiBackend = new DenoBackend();
