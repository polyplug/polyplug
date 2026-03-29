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

let _hostVtableLo = 0;
let _hostVtableHi = 0;

export function storeHostVtable(lo, hi) {
    _hostVtableLo = lo;
    _hostVtableHi = hi;
}

export function getHostVtable() {
    return { lo: _hostVtableLo, hi: _hostVtableHi };
}

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
 * Plugin interface — one per contract implemented by a plugin.
 * 
 * @typedef {Object} PluginInterface
 * @property {bigint} contract_id - Contract identifier (FNV-1a hash)
 * @property {number} contract_version - Version encoded as (major << 16) | (minor << 8) | patch
 * @property {number} function_count - Number of functions in the vtable
 * @property {number} dispatch_type - Dispatch mechanism type (0 = Native, 1 = VM)
 * @property {Object} dispatch - Union of dispatch mechanisms
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

// Re-export StringViewHelper from ABI for backward compatibility
export { StringViewHelper } from '../abi/polyplug_abi.ts';

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
    getExtension,
    readBytes,
    writeBytes,
    allocString,
    toStr
};

/**
 * Read bytes from host memory.
 * 
 * @param {bigint} ptr - Pointer to memory (as BigInt)
 * @param {number} len - Number of bytes to read
 * @returns {Uint8Array} Bytes read from host memory
 * 
 * @example
 * const bytes = readBytes(0x1234n, 10);
 */
export function readBytes(ptr, len) {
    if (len === 0) {
        return new Uint8Array(0);
    }
    // QuickJS: use bulk readMemory for performance (single FFI call)
    if (globalThis.polyplug.readMemory) {
        const buffer = globalThis.polyplug.readMemory(ptr, len);
        return new Uint8Array(buffer);
    }
    // Fallback: byte-by-byte read (for runtimes without readMemory)
    const bytes = new Uint8Array(len);
    for (let i = 0; i < len; i++) {
        bytes[i] = globalThis.polyplug.readByte(ptr + BigInt(i));
    }
    return bytes;
}

/**
 * Write bytes to host memory.
 * 
 * @param {bigint} ptr - Pointer to memory (as BigInt)
 * @param {Uint8Array} data - Bytes to write
 * @returns {void}
 * 
 * @example
 * writeBytes(0x1234n, new TextEncoder().encode("hello"));
 */
export function writeBytes(ptr, data) {
    for (let i = 0; i < data.length; i++) {
        globalThis.polyplug.writeByte(ptr + BigInt(i), data[i]);
    }
}

/**
 * Allocate a string in host memory.
 * 
 * @param {string} str - JavaScript string to allocate
 * @returns {{ ptr: bigint, len: number }} Pointer and length of allocated string
 * 
 * @example
 * const { ptr, len } = allocString("hello");
 */
export function allocString(str) {
    const encoder = new TextEncoder();
    const bytes = encoder.encode(str);
    const ptrArr = globalThis.polyplug.alloc(bytes.length);
    const ptr = (BigInt(ptrArr[1]) << 32n) + BigInt(ptrArr[0]);
    writeBytes(ptr, bytes);
    return { ptr, len: bytes.length };
}

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
    if (!sv || sv.len === 0) {
        return '';
    }
    // QuickJS: ptr is split into ptr_lo/ptr_hi
    // Deno: ptr is a bigint
    let ptr;
    if (typeof sv.ptr === 'bigint') {
        // Deno FFI - only use if Deno is available
        ptr = sv.ptr;
        if (typeof Deno !== 'undefined' && Deno.UnsafePointerView) {
            const ptrNum = Number(ptr);
            return new Deno.UnsafePointerView(ptrNum).getUtf8String(sv.len);
        }
        // Fallback to byte-by-byte read if Deno FFI not available
    } else {
        // QuickJS: reconstruct 64-bit pointer from hi/lo split
        ptr = (BigInt(sv.ptr_hi) << 32n) + BigInt(sv.ptr_lo);
    }
    // Read bytes and decode as UTF-8
    const bytes = readBytes(ptr, sv.len);
    const decoder = new TextDecoder();
    return decoder.decode(bytes);
}


