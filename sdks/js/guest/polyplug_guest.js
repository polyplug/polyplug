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
 * ABI error codes — returned by all ABI functions.
 * @enum {number}
 */
export const AbiErrorCode = {
    Ok: 0,
    Generic: 1,
    BufferTooSmall: 2,
    Panic: 3,
    NotFound: 4,
    StaleHandle: 5,
    FunctionNotAvailable: 6,
    DuplicateProvider: 7,
    InvalidPointer: 8,
    HostContractNotFound: 100,
    HostContractVersionMismatch: 101,
    HostContractCallFailed: 102,
};

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
 * @typedef {Object} GuestContractHandle
 * @property {number} index - Plugin index
 * @property {number} generation - Generation counter for validity checking
 */

/**
 * Plugin interface — one per contract implemented by a plugin.
 * 
 * @typedef {Object} GuestContractInterface
 * @property {bigint} contract_id - Contract identifier (FNV-1a hash)
 * @property {number} contract_version - Version encoded as (major << 16) | (minor << 8) | patch
 * @property {number} function_count - Number of functions in the interface
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
 * Host interface for plugin registration.
 *
 * @typedef {Object} HostInterface
 * @property {Function} register_contract - Function to register a guest contract
 * @property {*} host - Host interface pointer
 */

/**
 * Context passed to polyplug_init.
 * 
 * @typedef {Object} BundleInitContext
 * @property {StringView} bundle_path - Path to the bundle directory
 */

/**
 * Initialization function signature for polyplug plugins.
 * 
 * @callback InitFn
 * @param {HostInterface} host - Host interface for plugin registration
 * @param {BundleInitContext} context - Plugin context with bundle path
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
 * Helper class for working with StringView.
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
            ptr_lo: bytes,
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
        return '';
    }
}

/**
 * Get extension by ID.
 * 
 * @param {number} extensionId - Extension identifier
 * @returns {*} Extension interface pointer or null
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
    AbiErrorCode,
    EXT_TRACE_ID,
    DependencyNotFoundError,
    StringViewHelper,
    getExtension,
    readBytes,
    writeBytes,
    allocString,
    freeBytes,
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
        const buffer = globalThis.polyplug.readMemory(Number(ptr), len);
        return new Uint8Array(buffer);
    }
    // Fallback: byte-by-byte read (for runtimes without readMemory)
    const bytes = new Uint8Array(len);
    const ptrNum = Number(ptr);
    for (let i = 0; i < len; i++) {
        bytes[i] = globalThis.polyplug.readByte(ptrNum + i);
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
    const ptrNum = Number(ptr);
    for (let i = 0; i < data.length; i++) {
        globalThis.polyplug.writeByte(ptrNum + i, data[i]);
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
function _encodeUtf8(str) {
    const out = [];
    for (let i = 0; i < str.length; i++) {
        let code = str.charCodeAt(i);
        if (code >= 0xD800 && code <= 0xDBFF) {
            const low = str.charCodeAt(++i);
            code = 0x10000 + ((code - 0xD800) << 10) + (low - 0xDC00);
        }
        if (code < 0x80) {
            out.push(code);
        } else if (code < 0x800) {
            out.push(0xC0 | (code >> 6), 0x80 | (code & 0x3F));
        } else if (code < 0x10000) {
            out.push(0xE0 | (code >> 12), 0x80 | ((code >> 6) & 0x3F), 0x80 | (code & 0x3F));
        } else {
            out.push(0xF0 | (code >> 18), 0x80 | ((code >> 12) & 0x3F), 0x80 | ((code >> 6) & 0x3F), 0x80 | (code & 0x3F));
        }
    }
    return new Uint8Array(out);
}

function _decodeUtf8(bytes) {
    let str = '';
    let i = 0;
    while (i < bytes.length) {
        const b = bytes[i];
        if (b < 0x80) {
            str += String.fromCharCode(b);
            i++;
        } else if (b < 0xE0) {
            str += String.fromCharCode(((b & 0x1F) << 6) | (bytes[i + 1] & 0x3F));
            i += 2;
        } else if (b < 0xF0) {
            str += String.fromCharCode(((b & 0x0F) << 12) | ((bytes[i + 1] & 0x3F) << 6) | (bytes[i + 2] & 0x3F));
            i += 3;
        } else {
            const cp = ((b & 0x07) << 18) | ((bytes[i + 1] & 0x3F) << 12) | ((bytes[i + 2] & 0x3F) << 6) | (bytes[i + 3] & 0x3F);
            str += String.fromCharCode(0xD800 + ((cp - 0x10000) >> 10), 0xDC00 + ((cp - 0x10000) & 0x3FF));
            i += 4;
        }
    }
    return str;
}

export function allocString(str) {
    const bytes = (typeof TextEncoder !== 'undefined')
        ? new TextEncoder().encode(str)
        : _encodeUtf8(str);
    const ptrArr = globalThis.polyplug.alloc(bytes.length);
    const ptr = (BigInt(ptrArr[1]) << 32n) + BigInt(ptrArr[0]);
    writeBytes(ptr, bytes);
    return { ptr, len: bytes.length };
}

/**
 * Free a host-allocated region previously obtained via {@link allocString} or
 * `polyplug.alloc`.
 *
 * The host allocator requires the original allocation size and alignment to
 * free the exact region — passing the wrong size leaks memory (the host free is
 * a no-op on size 0). `alloc`/`allocString` use alignment 1, which is the
 * default here.
 *
 * @param {bigint} ptr - Pointer returned by allocString/alloc (as BigInt).
 * @param {number} size - Original allocation size in bytes.
 * @param {number} [align=1] - Original allocation alignment.
 * @returns {void}
 */
export function freeBytes(ptr, size, align = 1) {
    if (!ptr || size === 0) {
        return;
    }
    const ptrBig = BigInt(ptr);
    const lo = Number(ptrBig & 0xFFFFFFFFn);
    const hi = Number((ptrBig >> 32n) & 0xFFFFFFFFn);
    globalThis.polyplug.free(lo, hi, size, align);
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
    // Read bytes and decode as UTF-8 (without TextDecoder for QuickJS compatibility)
    const bytes = readBytes(ptr, sv.len);
    if (typeof TextDecoder !== 'undefined') {
        return new TextDecoder().decode(bytes);
    }
    return _decodeUtf8(bytes);
}


