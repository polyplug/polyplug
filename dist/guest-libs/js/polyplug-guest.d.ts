/**
 * @file polyplug-guest.d.ts
 * @description TypeScript type definitions for polyplug JavaScript guest plugins.
 *
 * This file provides type definitions for the polyplug-guest.js module.
 * Import types from this file when writing TypeScript plugins.
 */

/** ABI version constant. All plugins must return this from polyplug_abi_version(). */
export const POLYPLUG_ABI_VERSION: number;

/** ABI_OK - Success error code (0). */
export const ABI_OK: number;

/** ABI_ERROR_GENERIC - Generic error code (1). */
export const ABI_ERROR_GENERIC: number;

/** ABI_ERROR_PANIC - Plugin panicked (3). */
export const ABI_ERROR_PANIC: number;

/** Extension ID for the trace extension (fnv1a_32("trace") = 0xC4EB9AEE). */
export const EXT_TRACE_ID: number;

/**
 * A read-only view into a UTF-8 string in the host's address space.
 *
 * For QuickJS (js-quickjs): Pointer is split into lo/hi 32-bit halves
 * For Deno (js-deno): Pointer is a single bigint
 */
export interface StringView {
    /** Low 32 bits of pointer (QuickJS) OR full pointer as bigint (Deno) */
    readonly ptr_lo: number | bigint;
    /** High 32 bits of pointer (QuickJS only, 0 for Deno) */
    readonly ptr_hi?: number;
    /** Length in bytes */
    readonly len: number;
}

/**
 * A byte buffer owned by the host allocator.
 */
export interface Buffer {
    /** Low 32 bits of pointer (QuickJS) OR full pointer as bigint (Deno) */
    readonly ptr_lo: number | bigint;
    /** High 32 bits of pointer (QuickJS only, 0 for Deno) */
    readonly ptr_hi?: number;
    /** Used length in bytes */
    readonly len: number;
    /** Allocated capacity in bytes */
    readonly cap: number;
}

/**
 * ABI error structure. code === 0 means success.
 */
export interface AbiError {
    /** Error code (0 = success, non-zero = error) */
    readonly code: number;
    /** Error message (may be null/empty for success) */
    readonly message: StringView;
}

/**
 * Opaque handle to a loaded plugin.
 */
export interface PluginHandle {
    /** Plugin index */
    readonly index: number;
    /** Generation counter for validity checking */
    readonly generation: number;
}

/**
 * Plugin virtual method table.
 */
export interface PluginVTable {
    /** Contract identifier (FNV-1a hash) */
    readonly contract_id: bigint | number;
    /** Version encoded as (major << 16) | (minor << 8) | patch */
    readonly contract_version: number;
    /** Number of functions in the vtable */
    readonly function_count: number;
    /** Array of function implementations */
    readonly functions: Array<(...args: any[]) => any> | null;
}

/**
 * Plugin descriptor for registration.
 */
export interface PluginDescriptor {
    /** Plugin instance name */
    readonly name: StringView;
    /** Contract name (e.g., "pipeline.Decoder@1") */
    readonly contract_name: StringView;
    /** Major version */
    readonly version_major: number;
    /** Minor version */
    readonly version_minor: number;
    /** Patch version */
    readonly version_patch: number;
}

/**
 * Host registrar for plugin registration.
 */
export interface PluginRegistrar {
    /** Function to register a plugin */
    readonly register_plugin: (
        registrar: PluginRegistrar,
        descriptor: PluginDescriptor,
        vtable: PluginVTable
    ) => AbiError;
    /** Host vtable pointer */
    readonly host: unknown;
}

/**
 * Context passed to polyplug_init.
 */
export interface PluginContext {
    /** Path to the bundle directory */
    readonly bundle_path: StringView;
}

/**
 * Initialization function signature for polyplug plugins.
 *
 * This function is called by the host runtime when the plugin is loaded.
 * Implement this function and export it as `polyplug_init`.
 *
 * @param registrar - Host registrar for plugin registration
 * @param context - Plugin context with bundle path
 * @returns AbiError - Registration result (ABI_OK on success)
 *
 * @example
 * export function polyplug_init(
 *   registrar: PluginRegistrar,
 *   context: PluginContext
 * ): AbiError {
 *   // Register your plugin implementation
 *   return registrar.register_plugin(
 *     registrar,
 *     descriptor,
 *     vtable
 *   );
 * }
 */
export type InitFn = (
    registrar: PluginRegistrar,
    context: PluginContext
) => AbiError;

/**
 * Error thrown when a declared dependency cannot be resolved at init time.
 */
export class DependencyNotFoundError extends Error {
    constructor(contractName: string);
    readonly contractName: string;
}

/**
 * Helper utilities for StringView operations.
 */
export class StringViewHelper {
    /**
     * Create a StringView from a JavaScript string.
     *
     * @param str - The JavaScript string
     * @returns StringView pointing to encoded bytes
     *
     * @example
     * const sv = StringViewHelper.fromString("hello");
     */
    static fromString(str: string): StringView;

    /**
     * Convert a StringView to a JavaScript string.
     *
     * @param sv - The StringView to convert
     * @returns JavaScript string
     *
     * @example
     * const str = StringViewHelper.toString(sv);
     */
    static toString(sv: StringView): string;
}

/**
 * Get extension by ID.
 *
 * @param extensionId - Extension identifier
 * @returns Extension vtable pointer or null
 *
 * @example
 * const traceVtable = getExtension(EXT_TRACE_ID);
 */
export function getExtension(extensionId: number): unknown;

// Default exports
export default {
    POLYPLUG_ABI_VERSION: number,
    ABI_OK: number,
    ABI_ERROR_GENERIC: number,
    ABI_ERROR_PANIC: number,
    EXT_TRACE_ID: number,
    DependencyNotFoundError: typeof DependencyNotFoundError,
    StringViewHelper: typeof StringViewHelper,
    getExtension: typeof getExtension
};
