/**
 * @file polyplug-guest.js
 * @description Guest library for polyplug JavaScript/TypeScript plugins.
 * 
 * This module provides ABI types and helpers for writing polyplug guest plugins
 * in JavaScript. Plugins run inside the QuickJS VM embedded by the polyplug JS
 * loader; all host access goes through the injected `globalThis.polyplug` bridge.
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
    ReentrantCall: 9,
    HostContractNotFound: 100,
    HostContractVersionMismatch: 101,
    HostContractCallFailed: 102,
};

/**
 * Log severity levels for {@link log}, mirroring the ABI LogLevel enum
 * (`LogLevel` in abi.ts). Lower values are more severe.
 * @enum {number}
 */
export const LogLevel = {
    Error: 1,
    Warn: 2,
    Info: 3,
    Debug: 4,
    Trace: 5,
};

/**
 * A read-only view into a UTF-8 string in the host's address space.
 *
 * @typedef {Object} StringView
 * @property {number} ptr_lo - Low 32 bits of the pointer
 * @property {number} ptr_hi - High 32 bits of the pointer
 * @property {number} len - Length in bytes
 */

/**
 * A byte buffer owned by the host allocator.
 *
 * @typedef {Object} Buffer
 * @property {number} ptr_lo - Low 32 bits of the pointer
 * @property {number} ptr_hi - High 32 bits of the pointer
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
 * @typedef {Object} HostApi
 * @property {Function} register_guest_contract - Function to register a guest contract
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
 * @param {HostApi} host - Host interface for plugin registration
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
 * Send a log record to the host's logging funnel (RuntimeConfig log callback,
 * or the host's stderr default).
 *
 * `level` is one of {@link LogLevel} (unknown values are clamped to Error by
 * the loader), `scope` is a short stable tag chosen by the guest — the
 * suggested convention is `"guest.<plugin-name>"` — and `message` is delivered
 * verbatim.
 *
 * The `polyplug.log` bridge is injected into the VM by the polyplug JS loader;
 * outside a polyplug VM (e.g. plain unit tests of plugin code) it is absent
 * and this helper degrades to a no-op.
 *
 * @param {number} level - One of {@link LogLevel}.
 * @param {string} scope - Short stable tag, e.g. "guest.my-plugin".
 * @param {string} message - Log message, delivered verbatim.
 * @returns {void}
 *
 * @example
 * log(LogLevel.Info, "guest.decoder", "frame decoded");
 */
export function log(level, scope, message) {
    const bridge = globalThis.polyplug;
    if (!bridge || typeof bridge.log !== 'function') {
        return;
    }
    bridge.log(level, scope, message);
}

// Module exports
export default {
    POLYPLUG_ABI_VERSION,
    AbiErrorCode,
    LogLevel,
    DependencyNotFoundError,
    log,
    readBytes,
    writeBytes,
    allocString,
    allocStringArena,
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

// Validating UTF-8 decoder for QuickJS, where TextDecoder is absent. Mirrors
// the strictness of Rust's core::str::from_utf8 / the fatal TextDecoder: it
// rejects invalid lead bytes, truncated sequences, bad continuation bytes,
// overlong forms, surrogates, and code points above U+10FFFF, throwing a
// TypeError so a readable-but-invalid view never decodes to mojibake.
function _decodeUtf8(bytes) {
    let str = '';
    let i = 0;
    const n = bytes.length;
    while (i < n) {
        const b = bytes[i];
        if (b < 0x80) {
            str += String.fromCharCode(b);
            i++;
            continue;
        }
        let extra;
        let minCp;
        let cp;
        if (b >= 0xC0 && b < 0xE0) {
            extra = 1; minCp = 0x80; cp = b & 0x1F;
        } else if (b >= 0xE0 && b < 0xF0) {
            extra = 2; minCp = 0x800; cp = b & 0x0F;
        } else if (b >= 0xF0 && b < 0xF8) {
            extra = 3; minCp = 0x10000; cp = b & 0x07;
        } else {
            throw new TypeError('polyplug.toStr: StringView contains invalid UTF-8 (invalid lead byte)');
        }
        if (i + extra >= n) {
            throw new TypeError('polyplug.toStr: StringView contains invalid UTF-8 (truncated sequence)');
        }
        for (let k = 1; k <= extra; k++) {
            const cc = bytes[i + k];
            if ((cc & 0xC0) !== 0x80) {
                throw new TypeError('polyplug.toStr: StringView contains invalid UTF-8 (bad continuation byte)');
            }
            cp = (cp << 6) | (cc & 0x3F);
        }
        if (cp < minCp || cp > 0x10FFFF || (cp >= 0xD800 && cp <= 0xDFFF)) {
            throw new TypeError('polyplug.toStr: StringView contains invalid UTF-8 (overlong, surrogate, or out-of-range code point)');
        }
        if (cp < 0x10000) {
            str += String.fromCharCode(cp);
        } else {
            str += String.fromCharCode(0xD800 + ((cp - 0x10000) >> 10), 0xDC00 + ((cp - 0x10000) & 0x3FF));
        }
        i += extra + 1;
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
 * Allocate a return-value string from the current call arena.
 *
 * Use this for strings RETURNED from a contract function: the bytes are served
 * from the host's per-call {@link CallArena} and stay valid until the next call
 * on the same caller, so the guest never frees them. When no arena is active
 * (`polyplug.arenaAlloc` falls back to `polyplug.alloc`), this behaves like
 * {@link allocString}. For data that must outlive the call, use
 * {@link allocString} and free it explicitly with {@link freeBytes}.
 *
 * @param {string} str - JavaScript string to allocate.
 * @returns {{ ptr: bigint, len: number }} Pointer and length of the bytes.
 */
export function allocStringArena(str) {
    const bytes = (typeof TextEncoder !== 'undefined')
        ? new TextEncoder().encode(str)
        : _encodeUtf8(str);
    const ptrArr = globalThis.polyplug.arenaAlloc(bytes.length);
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
    // Reconstruct the 64-bit pointer from the QuickJS hi/lo split.
    const ptr = (BigInt(sv.ptr_hi) << 32n) + BigInt(sv.ptr_lo);
    if (ptr === 0n) {
        return '';
    }
    // Read bytes and decode as UTF-8 (TextDecoder is absent in QuickJS).
    // Both paths reject invalid UTF-8 (the fatal decoder / the manual scan throw
    // a TypeError) so a readable-but-invalid view surfaces an error rather than
    // silently yielding U+FFFD or mojibake.
    const bytes = readBytes(ptr, sv.len);
    if (typeof TextDecoder !== 'undefined') {
        return new TextDecoder('utf-8', { fatal: true }).decode(bytes);
    }
    return _decodeUtf8(bytes);
}

/**
 * Check if a StringView (or string) starts with the given prefix.
 *
 * @param {StringView|string} sv - StringView from polyplug ABI, or a plain JS string
 * @param {string} prefix - The prefix to check
 * @returns {boolean} True if the string starts with prefix
 *
 * @example
 * const ok = startsWith(stringView, 'hello');
 */
export function startsWith(sv, prefix) {
    const s = typeof sv === 'string' ? sv : toStr(sv);
    return s.startsWith(prefix);
}

/**
 * Check if a StringView (or string) ends with the given suffix.
 *
 * @param {StringView|string} sv - StringView from polyplug ABI, or a plain JS string
 * @param {string} suffix - The suffix to check
 * @returns {boolean} True if the string ends with suffix
 *
 * @example
 * const ok = endsWith(stringView, 'world');
 */
export function endsWith(sv, suffix) {
    const s = typeof sv === 'string' ? sv : toStr(sv);
    return s.endsWith(suffix);
}

/**
 * Strip a prefix from a StringView (or string).
 * Returns the original string unchanged if the prefix is not present.
 *
 * @param {StringView|string} sv - StringView from polyplug ABI, or a plain JS string
 * @param {string} prefix - The prefix to remove
 * @returns {string} The string with prefix removed, or the original string
 *
 * @example
 * const stripped = stripPrefix(stringView, 'hello_');
 */
export function stripPrefix(sv, prefix) {
    const s = typeof sv === 'string' ? sv : toStr(sv);
    if (s.startsWith(prefix)) {
        return s.slice(prefix.length);
    }
    return s;
}

/**
 * Split a StringView (or string) by a literal delimiter, keeping empty segments.
 *
 * @param {StringView|string} sv - StringView from polyplug ABI, or a plain JS string
 * @param {string} delimiter - The literal delimiter to split by
 * @returns {string[]} [] for a null/empty input, [s] for an empty delimiter,
 *                     otherwise the segments around every occurrence (empties kept)
 *
 * @example
 * const parts = split(stringView, ',');
 */
export function split(sv, delimiter) {
    const s = typeof sv === 'string' ? sv : toStr(sv);
    if (s.length === 0) {
        return [];
    }
    if (delimiter.length === 0) {
        return [s];
    }
    return s.split(delimiter);
}

