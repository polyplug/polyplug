/**
 * @file polyplug.js
 * @description Host library for polyplug JavaScript/TypeScript hosts.
 * 
 * This module provides ABI types and helpers for hosting polyplug plugins
 * in JavaScript. Works with Deno FFI runtime.
 * 
 * @module polyplug
 */

import { ReloadPhase } from "./polyplug/reload_phase.js";

export { 
  getPlatformIdentifier, 
  getNativeLibraryFilename, 
  loadNativeLibrary, 
  openNativeLibrary 
} from "./native-loader.ts";

/** @type {bigint} */
export const NULL_HANDLE = 0xFFFFFFFFFFFFFFFFn;

/** FNV-1a offset basis for 64-bit hash */
const FNV_OFFSET = 0xcbf29ce484222325n;
/** FNV-1a prime for 64-bit hash */
const FNV_PRIME = 0x00000100000001B3n;
/** 64-bit mask */
const MASK_64 = 0xFFFFFFFFFFFFFFFFn;

const SYMBOLS = {
  polyplug_runtime_create: { parameters: [], result: "pointer" },
  polyplug_runtime_destroy: { parameters: ["pointer"], result: "void" },
  polyplug_runtime_load_bundle: { parameters: ["pointer", "pointer", "usize"], result: "u32" },
  polyplug_runtime_reload_bundle: { parameters: ["pointer", "pointer", "usize"], result: "u32" },
  polyplug_runtime_find_by_contract: { parameters: ["pointer", "u64", "u32"], result: "u64" },
  polyplug_runtime_find_by_bundle: { parameters: ["pointer", "u64", "u64", "u32"], result: "u64" },
  polyplug_runtime_find_all_by_contract: { parameters: ["pointer", "u64", "u32", "pointer", "usize"], result: "usize" },
  polyplug_runtime_resolve_plugin: { parameters: ["pointer", "u64"], result: "pointer" },
  polyplug_runtime_last_error: { parameters: ["pointer", "usize"], result: "usize" },
  polyplug_runtime_error_message_len: { parameters: [], result: "usize" },
  polyplug_runtime_on_reload: { parameters: ["pointer"], result: "void" },
  polyplug_runtime_set_config: { parameters: ["pointer"], result: "u32" },
  polyplug_host_free: { parameters: ["pointer", "usize", "usize"], result: "void" },
};

// Module-level caches for hot path performance
const _funcCache = new Map();
const _DISPATCH_FN_TYPE = new Deno.UnsafeFunctionPrototype({
    parameters: ["pointer", "pointer"],
    result: "u32"
});
const _encoder = new TextEncoder();
const _decoder = new TextDecoder();

// Module-level pending callbacks for hot-reload notification
/** @type {function(ReloadPhase): void | null} */
let _pendingReloadCallback = null;
/** @type {import("./polyplug/runtime_config.js").RuntimeConfig | null} */
let _pendingConfig = null;
/** @type {Deno.UnsafeCallback | null} */
let _ffiReloadCallback = null;

// Callback type for reload notifications
const _RELOAD_CALLBACK_TYPE = {
    parameters: ["u32", "u64", "pointer", "usize", "u32", "pointer", "usize"],
    result: "void"
};

/**
 * Compute FNV-1a 64-bit hash.
 * @param {Uint8Array | string} data - Data to hash
 * @returns {bigint} 64-bit hash
 */
export function fnv1a64(data) {
    const bytes = typeof data === 'string' ? _encoder.encode(data) : data;
    let h = FNV_OFFSET;
    for (const b of bytes) {
        h = (h ^ BigInt(b)) * FNV_PRIME;
        h = h & MASK_64;
    }
    return h;
}

/**
 * Compute contract ID using FNV-1a 64-bit hash.
 * @param {string} name - Contract name (e.g., "pipeline.Decoder")
 * @param {number} majorVersion - Major version number
 * @returns {bigint} 64-bit contract ID
 */
export function contractId(name, majorVersion) {
    return fnv1a64(`${name}@${majorVersion}`);
}

/**
 * Compute bundle ID using FNV-1a 64-bit hash.
 * @param {string} name - Bundle name
 * @returns {bigint} 64-bit bundle ID
 */
export function bundleId(name) {
    return fnv1a64(name);
}

/**
 * Convert StringView to JavaScript string.
 * @param {{ ptr: bigint; len: number }} sv - StringView from polyplug ABI
 * @returns {string} JavaScript string
 */
export function toStr(sv) {
    if (!sv || sv.ptr === 0n || sv.len === 0) return '';
    return new Deno.UnsafePointerView(sv.ptr).getUtf8String(sv.len);
}

/**
 * Alias for toStr().
 * @param {{ ptr: bigint; len: number }} sv - StringView
 * @returns {string} JavaScript string
 */
export const toString = toStr;

/**
 * Register a callback to be invoked during hot-reload operations.
 * Must be called BEFORE creating a Runtime instance.
 * @param {function(ReloadPhase): void} callback - Callback function
 */
export function onReload(callback) {
    _pendingReloadCallback = callback;
}

/**
 * Set runtime configuration for subsequently created runtimes.
 * Must be called BEFORE creating a Runtime instance.
 * @param {import("./polyplug/runtime_config.js").RuntimeConfig} config - Configuration options
 */
export function setConfig(config) {
    _pendingConfig = config;
}

export class Runtime {
  #lib;
  #ptr;

  /**
   * @param {Deno.DynamicLibrary} lib - Dynamic library instance
   * @param {Deno.PointerValue} ptr - Runtime pointer
   */
  constructor(lib, ptr) {
    this.#lib = lib;
    this.#ptr = ptr;
  }

  registerNativeLoader() {
    // Native loader is built-in to the runtime
  }

  [Symbol.dispose]() {
    this.#lib.symbols.polyplug_runtime_destroy(this.#ptr);
  }

  /**
   * Get runtime pointer.
   * @returns {Deno.PointerValue}
   */
  ptr() {
    return this.#ptr;
  }

  /**
   * Get last error message.
   * @returns {string}
   */
  lastError() {
    const len = Number(this.#lib.symbols.polyplug_runtime_error_message_len());
    if (len === 0) return "";
    const buf = new Uint8Array(len);
    const ptr = Deno.UnsafePointer.of(buf);
    this.#lib.symbols.polyplug_runtime_last_error(ptr, BigInt(len));
    return _decoder.decode(buf);
  }

  /**
   * Load a plugin bundle.
   * @param {string} path - Path to bundle directory
   */
  loadBundle(path) {
    const encoded = _encoder.encode(path);
    const ptr = Deno.UnsafePointer.of(encoded);
    const result = this.#lib.symbols.polyplug_runtime_load_bundle(this.#ptr, ptr, BigInt(encoded.length));
    if (result !== 0) throw new Error(`polyplug_runtime_load_bundle failed: ${this.lastError()}`);
  }

  /**
   * Reload a plugin bundle.
   * @param {string} path - Path to bundle directory
   */
  reloadBundle(path) {
    const encoded = _encoder.encode(path);
    const ptr = Deno.UnsafePointer.of(encoded);
    const result = this.#lib.symbols.polyplug_runtime_reload_bundle(this.#ptr, ptr, BigInt(encoded.length));
    if (result !== 0) throw new Error(`polyplug_runtime_reload_bundle failed: ${this.lastError()}`);
  }

  /**
   * Find plugin by contract ID.
   * @param {bigint} contractId - Contract identifier
   * @param {number} [minVersion=0] - Minimum version
   * @returns {bigint} Plugin handle
   */
  findByContract(contractId, minVersion = 0) {
    return this.#lib.symbols.polyplug_runtime_find_by_contract(this.#ptr, contractId, minVersion);
  }

  /**
   * Find plugin by bundle ID and contract ID.
   * @param {bigint} bundleId - Bundle identifier
   * @param {bigint} contractId - Contract identifier
   * @param {number} [minVersion=0] - Minimum version
   * @returns {bigint} Plugin handle
   */
  findByBundle(bundleId, contractId, minVersion = 0) {
    return this.#lib.symbols.polyplug_runtime_find_by_bundle(this.#ptr, bundleId, contractId, minVersion);
  }

  /**
   * Find all plugins by contract ID.
   * @param {bigint} contractId - Contract identifier
   * @param {number} [minVersion=0] - Minimum version
   * @param {number} [cap=64] - Buffer capacity
   * @returns {bigint[]} Array of plugin handles
   */
  findAllByContract(contractId, minVersion = 0, cap = 64) {
    const buf = new BigUint64Array(cap);
    const ptr = Deno.UnsafePointer.of(buf);
    const count = Number(this.#lib.symbols.polyplug_runtime_find_all_by_contract(this.#ptr, contractId, minVersion, ptr, BigInt(cap)));
    return Array.from(buf.slice(0, Math.min(count, cap)));
  }

  /**
   * Resolve plugin handle to guard.
   * Guard stores handle for hot-reload safety - re-resolves vtable on each call.
   * @param {bigint} packedHandle - Packed plugin handle
   * @returns {Guard} Plugin guard
   */
  resolvePlugin(packedHandle) {
    if (packedHandle === NULL_HANDLE) {
      throw new Error("null plugin handle");
    }
    return new Guard(this, packedHandle);
  }
}

/**
 * Guard stores runtime + handle for hot-reload safety.
 * Re-resolves vtable on each call to detect stale handles after hot-reload.
 */
export class Guard {
  #runtime;
  #handle;

  /**
   * @param {Runtime} runtime - Runtime instance
   * @param {bigint} handle - Packed plugin handle
   */
  constructor(runtime, handle) {
    this.#runtime = runtime;
    this.#handle = handle;
  }

  /**
   * Get the packed handle.
   * @returns {bigint}
   */
  handle() {
    return this.#handle;
  }

  /**
   * Internal: resolve vtable for this call (hot-reload safe).
   * @returns {Deno.PointerValue}
   */
  #resolveVtable() {
    const vtablePtr = this.#runtime.#lib.symbols.polyplug_runtime_resolve_plugin(
      this.#runtime.#ptr,
      this.#handle
    );
    if (vtablePtr === null) {
      throw new Error(`polyplug_runtime_resolve_plugin failed: ${this.#runtime.lastError()}`);
    }
    return vtablePtr;
  }

  /**
   * Call a plugin function by index (hot-reload safe).
   * Re-resolves vtable on each call to detect stale handles.
   * @param {number} funcIdx - Function index (0-based)
   * @param {string} input - Input string
   * @returns {string} Output string from plugin
   */
  call(funcIdx, input) {
    const vtablePtr = this.#resolveVtable();
    
    // Read vtable as BigUint64Array for faster access
    const vtableBuf = new Deno.UnsafePointerView(vtablePtr).getArrayBuffer(16);
    const vtable = new BigUint64Array(vtableBuf);
    const funcCount = vtable[0];
    const funcsPtr = vtable[1];
    
    if (funcIdx >= Number(funcCount)) {
      throw new Error(`function index ${funcIdx} out of bounds`);
    }
    
    // Read function pointer from funcs array
    const funcsBuf = new Deno.UnsafePointerView(Deno.UnsafePointer.create(funcsPtr)).getArrayBuffer(Number(funcCount) * 8);
    const funcs = new BigUint64Array(funcsBuf);
    const funcPtr = funcs[funcIdx];
    
    let func = _funcCache.get(funcPtr);
    if (!func) {
      func = new Deno.UnsafeFnPointer(funcPtr, _DISPATCH_FN_TYPE);
      _funcCache.set(funcPtr, func);
    }
    
    const inputData = _encoder.encode(input);
    const inputPtr = Deno.UnsafePointer.of(inputData);
    
    const outputBuf = new Uint8Array(16);
    const outputPtr = Deno.UnsafePointer.of(outputBuf);
    
    const errCode = func.call(inputPtr, outputPtr);
    
    if (errCode === 0) {
      const outputView = new Deno.UnsafePointerView(outputPtr);
      const outPtr = outputView.getBigUint64(0);
      const outLen = Number(outputView.getBigUint64(8));
      
      if (outPtr !== 0n && outLen > 0) {
        const result = new Deno.UnsafePointerView(outPtr).getUtf8String(outLen);
        this.#runtime.#lib.symbols.polyplug_host_free(outPtr, BigInt(outLen), 1);
        return result;
      }
    }
    
    throw new Error(`plugin returned error code=${errCode}`);
  }
}

/**
 * Open polyplug library.
 * @param {string} soPath - Path to libpolyplug.so
 * @returns {Deno.DynamicLibrary}
 */
export function openPolyplug(soPath) {
  return Deno.dlopen(soPath, SYMBOLS);
}

/**
 * Create new runtime instance.
 * @param {Deno.DynamicLibrary} lib - Dynamic library
 * @returns {Runtime}
 */
export function runtimeNew(lib) {
  // Register config before creating runtime
  if (_pendingConfig) {
    const config = _pendingConfig;
    const configBuf = new Uint8Array(16);
    const view = new DataView(configBuf.buffer);
    view.setUint32(0, config.hotReloadMaxRetries, true);
    view.setBigUint64(4, BigInt(config.hotReloadRetryIntervalMs), true);
    view.setUint8(12, config.hotReloadAbortOnMaxRetries ? 1 : 0);
    const configPtr = Deno.UnsafePointer.of(configBuf);
    const result = lib.symbols.polyplug_runtime_set_config(configPtr);
    if (result !== 0) {
      throw new Error(`polyplug_runtime_set_config failed: ${result}`);
    }
  }

  // Register callback before creating runtime
  if (_pendingReloadCallback) {
    const callback = _pendingReloadCallback;
    _ffiReloadCallback = new Deno.UnsafeCallback(_RELOAD_CALLBACK_TYPE, 
      (phaseType, bundleId, bundleNamePtr, bundleNameLen, retryCount, reasonPtr, reasonLen) => {
        let bundleName = "";
        if (bundleNamePtr !== 0n && bundleNameLen > 0) {
          bundleName = new Deno.UnsafePointerView(bundleNamePtr).getUtf8String(Number(bundleNameLen));
        }
        let reason = "";
        if (reasonPtr !== 0n && reasonLen > 0) {
          reason = new Deno.UnsafePointerView(reasonPtr).getUtf8String(Number(reasonLen));
        }
        const phase = new ReloadPhase(phaseType, bundleId, bundleName, retryCount, reason);
        callback(phase);
      }
    );
    lib.symbols.polyplug_runtime_on_reload(_ffiReloadCallback.pointer);
  }

  const ptr = lib.symbols.polyplug_runtime_create();
  if (ptr === null) {
    const lenVal = lib.symbols.polyplug_runtime_error_message_len();
    const len = Number(lenVal);
    let errMsg = "polyplug_runtime_create failed";
    if (len > 0) {
      const buf = new Uint8Array(len);
      lib.symbols.polyplug_runtime_last_error(Deno.UnsafePointer.of(buf), BigInt(len));
      errMsg += ": " + _decoder.decode(buf);
    }
    throw new Error(errMsg);
  }
  return new Runtime(lib, ptr);
}