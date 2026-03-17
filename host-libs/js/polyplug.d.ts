/**
 * @file polyplug.d.ts
 * @description TypeScript type definitions for polyplug JavaScript host library.
 *
 * This file provides type definitions for the polyplug.js module.
 * Import types from this file when writing TypeScript hosts.
 */

/** NULL_HANDLE - Sentinel value for invalid plugin handles */
export const NULL_HANDLE: bigint;

/**
 * Compute FNV-1a 64-bit hash.
 * @param data - Data to hash (Uint8Array or string)
 * @returns 64-bit hash as bigint
 */
export function fnv1a64(data: Uint8Array | string): bigint;

/**
 * Compute contract ID using FNV-1a 64-bit hash.
 * @param name - Contract name (e.g., "pipeline.Decoder")
 * @param majorVersion - Major version number
 * @returns 64-bit contract ID as bigint
 */
export function contractId(name: string, majorVersion: number): bigint;

/**
 * Compute bundle ID using FNV-1a 64-bit hash.
 * @param name - Bundle name
 * @returns 64-bit bundle ID as bigint
 */
export function bundleId(name: string): bigint;

/**
 * Convert StringView to JavaScript string.
 * @param sv - StringView from polyplug ABI
 * @returns JavaScript string
 */
export function toStr(sv: { ptr: bigint; len: number }): string;

/**
 * Alias for toStr().
 * @param sv - StringView
 * @returns JavaScript string
 */
export const toString: (sv: { ptr: bigint; len: number }) => string;

/**
 * Create StringView from JavaScript string (owned copy).
 * @param s - JavaScript string
 * @returns StringView with allocated memory
 */
export function strAsView(s: string): { ptr: bigint; len: number };

/**
 * Call a plugin function by vtable index.
 * @param lib - Deno dynamic library instance
 * @param vtablePtr - Pointer to plugin vtable
 * @param funcIdx - Function index (0-based)
 * @param input - Input string
 * @returns Output string from plugin
 */
export function callPluginFn(
    lib: Deno.DynamicLibrary,
    vtablePtr: Deno.PointerValue,
    funcIdx: number,
    input: string
): string;

/** Runtime class for managing polyplug plugin runtime. */
export class Runtime {
    /**
     * @param lib - Dynamic library instance
     * @param ptr - Runtime pointer
     */
    constructor(lib: Deno.DynamicLibrary, ptr: Deno.PointerValue);

    /** Register native loader (built-in, no-op) */
    registerNativeLoader(): void;

    /** Dispose runtime */
    [Symbol.dispose](): void;

    /** Get runtime pointer */
    ptr(): Deno.PointerValue;

    /** Get last error message */
    lastError(): string;

    /**
     * Load a plugin bundle.
     * @param path - Path to bundle directory
     */
    loadBundle(path: string): void;

    /**
     * Reload a plugin bundle.
     * @param path - Path to bundle directory
     */
    reloadBundle(path: string): void;

    /**
     * Find plugin by contract ID.
     * @param contractId - Contract identifier
     * @param minVersion - Minimum version (default: 0)
     * @returns Plugin handle (bigint, NULL_HANDLE if not found)
     */
    findByContract(contractId: bigint, minVersion?: number): bigint;

    /**
     * Find plugin by bundle ID and contract ID.
     * @param bundleId - Bundle identifier
     * @param contractId - Contract identifier
     * @param minVersion - Minimum version (default: 0)
     * @returns Plugin handle (bigint, NULL_HANDLE if not found)
     */
    findByBundle(bundleId: bigint, contractId: bigint, minVersion?: number): bigint;

    /**
     * Find all plugins by contract ID.
     * @param contractId - Contract identifier
     * @param minVersion - Minimum version (default: 0)
     * @param cap - Buffer capacity (default: 64)
     * @returns Array of plugin handles
     */
    findAllByContract(contractId: bigint, minVersion?: number, cap?: number): bigint[];

    /**
     * Resolve plugin handle to guard.
     * @param packedHandle - Packed plugin handle
     * @returns Plugin guard
     */
    resolvePlugin(packedHandle: bigint): Guard;
}

/** Plugin guard - holds a lock on a resolved plugin. */
export class Guard {
    /**
     * @param lib - Dynamic library instance
     * @param ptr - Guard pointer
     */
    constructor(lib: Deno.DynamicLibrary, ptr: Deno.PointerValue);

    /** Register native loader (built-in, no-op) */
    registerNativeLoader(): void;

    /** Release guard */
    [Symbol.dispose](): void;

    /** Get vtable pointer */
    vtable(): Deno.PointerValue;
}

/**
 * Open polyplug library.
 * @param soPath - Path to libpolyplug.so
 * @returns Deno dynamic library instance
 */
export function openPolyplug(soPath: string): Deno.DynamicLibrary;

/**
 * Create new runtime instance.
 * @param lib - Dynamic library
 * @returns Runtime instance
 */
export function runtimeNew(lib: Deno.DynamicLibrary): Runtime;
