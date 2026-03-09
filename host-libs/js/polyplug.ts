// THIS FILE IS PART OF THE polyplug HOST LIBRARY FOR JAVASCRIPT/TYPESCRIPT
// It is NOT auto-generated. Do not add the auto-generated header.

/**
 * polyplug host library for JavaScript/TypeScript.
 *
 * Provides TypeScript type declarations and helper utilities for host applications
 * that embed the polyplug runtime via the polyplug C ABI.
 *
 * This file is a type-only library — it declares the shape of the C ABI
 * that the host application must load via its FFI mechanism (e.g., bun:ffi,
 * Deno.dlopen, or a Node.js N-API binding).
 */

/** Opaque handle to a loaded plugin. */
export interface PluginHandle {
    readonly index: number;
    readonly generation: number;
}

/** ABI OK sentinel value. */
export const ABI_OK: number = 0;

/** Error returned from any polyplug ABI function. code === 0 means success. */
export interface AbiError {
    readonly code: number;
    readonly message: {
        readonly ptr: bigint;
        readonly len: number;
    };
}

/**
 * The polyplug C ABI surface exported by `libpolyplug.so` / `libpolyplug.dylib`.
 *
 * Host applications must load these symbols via their FFI mechanism.
 */
export interface PolyplugAbi {
    /**
     * Find the first provider of a contract.
     * `contract_id` is the FNV-1a hash of `"name@major"`.
     * `min_version` is the minimum acceptable version (minor << 16 | patch).
     * Returns a PluginHandle; `handle.index === 0xFFFFFFFF` means not found.
     */
    readonly find_by_contract: (contract_id: bigint, min_version: number) => PluginHandle;

    /**
     * Resolve a PluginHandle to a vtable pointer (as bigint).
     * Returns 0n if the handle is stale or invalid.
     */
    readonly resolve_plugin: (handle: PluginHandle) => bigint;
}

/**
 * Compute the FNV-1a 64-bit hash of a string (matches polyplug::abi::contract_id()).
 * Use this to compute `contract_id` values from `"name@major"` strings.
 *
 * @example
 * const id = contractId('test.add@1'); // matches FNV-1a('test.add@1')
 */
export function contractId(name: string): bigint {
    const FNV_OFFSET_BASIS: bigint = 14695981039346656037n;
    const FNV_PRIME: bigint = 1099511628211n;
    const MASK64: bigint = (1n << 64n) - 1n;
    let hash: bigint = FNV_OFFSET_BASIS;
    const encoder: TextEncoder = new TextEncoder();
    const bytes: Uint8Array = encoder.encode(name);
    for (const byte of bytes) {
        hash = (hash ^ BigInt(byte)) * FNV_PRIME & MASK64;
    }
    return hash;
}
