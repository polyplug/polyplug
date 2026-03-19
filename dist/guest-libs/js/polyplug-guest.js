/**
 * @file polyplug-guest.js
 * @description Guest library for polyplug JavaScript/TypeScript plugins.
 * 
 * This module provides ABI types and helpers for writing polyplug guest plugins
 * in JavaScript. It works with both QuickJS (js-quickjs) and Deno (js-deno) runtimes.
 * 
 * @module polyplug-guest
 */

/**
 * ABI version constant. All plugins must return this from polyplug_abi_version().
 * @type {number}
 */
export const POLYPLUG_ABI_VERSION = 1;

/**
 * ABI_OK - Success error code (0).
 * @type {number}
 */
export const ABI_OK = 0;

/**
 * ABI_ERROR_GENERIC - Generic error code (1).
 * @type {number}
 */
export const ABI_ERROR_GENERIC = 1;

/**
 * ABI_ERROR_PANIC - Plugin panicked (3).
 * @type {number}
 */
export const ABI_ERROR_PANIC = 3;

/**
 * Extension ID for the trace extension.
 * Value: fnv1a_32("trace") = 0xC4EB9AEE
 * @type {number}
 */
export const EXT_TRACE_ID = 0xC4EB9AEE;

/**
 * A read-only view into a UTF-8 string in the host's address space.
 * 
 * @typedef {Object} StringView
 * @property {number} ptr_lo - Low 32 bits of the pointer (QuickJS) OR ptr: bigint (Deno)
 * @property {number} ptr_hi - High 32 bits of the pointer (QuickJS only)
 * @property {number} len - Length in bytes
 */

/**
 * A byte buffer owned by the host allocator.
 * 
 * @typedef {Object} Buffer
 * @property {number} ptr_lo - Low 32 bits of the pointer (QuickJS) OR ptr: bigint (Deno)
 * @property {number} ptr_hi - High 32 bits of the pointer (QuickJS only)
 * @property {number} len - Used length in bytes
 * @property {number} cap - Allocated capacity in bytes
 */

/**
 * ABI error structure. code === 0 means success.
 * 
 * @typedef {Object} AbiError
 * @property {number} code - Error code (0 = success)
 * @property {StringView} message - Error message (may be null)
 */

/**
 * Opaque handle to a loaded plugin.
 * 
 * @typedef {Object} PluginHandle
 * @property {number} index - Plugin index
 * @property {number} generation - Generation counter for validity checking
 */

/**
 * Plugin virtual method table.
 * 
 * @typedef {Object} PluginVTable
 * @property {bigint|number} contract_id - Contract identifier (FNV-1a hash)
 * @property {number} contract_version - Version encoded as (major << 16) | (minor << 8) | patch
 * @property {number} function_count - Number of functions in the vtable
 * @property {Array<Function>} functions - Array of function implementations
 */

/**
 * Plugin descriptor for registration.
 * 
 * @typedef {Object} PluginDescriptor
 * @property {StringView} name - Plugin instance name
 * @property {StringView} contract_name - Contract name (e.g., "pipeline.Decoder@1")
 * @property {number} version_major - Major version
 * @property {number} version_minor - Minor version
 * @property {number} version_patch - Patch version
 */

/**
 * Host registrar for plugin registration.
 * 
 * @typedef {Object} PluginRegistrar
 * @property {Function} register_plugin - Function to register a plugin
 * @property {*} host - Host vtable pointer
 */

/**
 * Context passed to polyplug_init.
 * 
 * @typedef {Object} PluginContext
 * @property {StringView} bundle_path - Path to the bundle directory
 */

/**
 * Initialization function signature for polyplug plugins.
 * 
 * @callback InitFn
 * @param {PluginRegistrar} registrar - Host registrar for plugin registration
 * @param {PluginContext} context - Plugin context with bundle path
 * @returns {AbiError} Registration result
 */

/**
 * Error thrown when a declared dependency cannot be resolved at init time.
 */
export class DependencyNotFoundError extends Error {
    /**
     * @param {string} contractName - Name of the missing contract
     */
    constructor(contractName) {
        super(`dependency not found: ${contractName}`);
        this.name = 'DependencyNotFoundError';
        this.contractName = contractName;
    }
}

/**
 * Helper utilities for StringView operations.
 */
export class StringViewHelper {
    /**
     * Create a StringView from a JavaScript string.
     * 
     * @param {string} str - The JavaScript string
     * @returns {StringView} StringView pointing to encoded bytes
     * 
     * @example
     * const sv = StringViewHelper.fromString("hello");
     */
    static fromString(str) {
        const encoder = new TextEncoder();
        const bytes = encoder.encode(str);
        return {
            ptr_lo: bytes,  // Host will handle memory
            ptr_hi: 0,
            len: bytes.length
        };
    }

    /**
     * Convert a StringView to a JavaScript string.
     * 
     * @param {StringView} sv - The StringView to convert
     * @returns {string} JavaScript string
     * 
     * @example
     * const str = StringViewHelper.toString(sv);
     */
    static toString(sv) {
        if (!sv || sv.len === 0) return '';
        // Host provides memory accessor - actual implementation depends on runtime
        // This is a placeholder - the generated code will provide actual implementation
        return '';
    }
}

/**
 * Get extension by ID.
 * 
 * @param {number} extensionId - Extension identifier
 * @returns {*} Extension vtable pointer or null
 * 
 * @example
 * const traceVtable = polyplug.getExtension(EXT_TRACE_ID);
 */
export function getExtension(extensionId) {
    // Implementation provided by host runtime
    // This is a placeholder for the interface
    return null;
}

// Module exports
export default {
    POLYPLUG_ABI_VERSION,
    ABI_OK,
    ABI_ERROR_GENERIC,
    ABI_ERROR_PANIC,
    EXT_TRACE_ID,
    DependencyNotFoundError,
    StringViewHelper,
    getExtension
};

/**
 * Convert a StringView to a JavaScript string.
 * 
 * @param {StringView} sv - StringView from polyplug ABI
 * @returns {string} JavaScript string (UTF-8 decoded)
 * 
 * @example
 * const s = toStr(stringView);
 */
export function toStr(sv) {
    if (!sv || !sv.ptr || sv.len === 0) {
        return '';
    }
    // QuickJS: ptr is split into ptr_lo/ptr_hi
    // Deno: ptr is a bigint
    if (typeof sv.ptr === 'bigint') {
        // Deno FFI
        const ptr = Number(sv.ptr);
        return new Deno.UnsafePointerView(ptr).getUtf8String(sv.len);
    } else {
        // QuickJS
        const ptr = (sv.ptr_hi << 16) + sv.ptr_lo;
        // Host runtime provides memory view
        return globalThis.__polyplug_read_string(ptr, sv.len) || '';
    }
}


