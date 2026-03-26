/**
 * @file polyplug.js
 * @description Host library for polyplug JavaScript/TypeScript hosts.
 * 
 * This module provides ABI types and helpers for hosting polyplug plugins
 * in JavaScript. Works with Deno FFI runtime.
 * 
 * @module polyplug
 */

if (typeof Deno === "undefined") {
  throw new Error(
    "@polyplug/runtime currently only supports Deno. " +
    "Node.js and Bun support is planned for future releases. " +
    "See https://github.com/polyplug/polyplug for updates."
  );
}

import { ReloadPhase } from "./reload_phase.js";

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
  polyplug_runtime_release_plugin: { parameters: ["pointer"], result: "void" },
  polyplug_runtime_last_error: { parameters: ["pointer", "pointer", "usize"], result: "usize" },
  polyplug_runtime_error_message_len: { parameters: ["pointer"], result: "usize" },
  polyplug_runtime_create_with_options: { parameters: ["pointer"], result: "pointer" },
  polyplug_host_free: { parameters: ["pointer", "usize", "usize"], result: "void" },
};

// Module-level caches for hot path performance
const _funcCache = new Map();
const _DISPATCH_FN_DEF = {
    parameters: ["pointer", "pointer"],
    result: { struct: ["u32", "u32", "pointer", "usize"] }
};
const _encoder = new TextEncoder();
const _decoder = new TextDecoder();

// Module-level pending callbacks for hot-reload notification
/** @type {function(ReloadPhase): void | null} */
let _pendingReloadCallback = null;
/** @type {import("./runtime_config.js").RuntimeConfig | null} */
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
 * @param {import("./runtime_config.js").RuntimeConfig} config - Configuration options
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
   * Get library instance.
   * @returns {Deno.DynamicLibrary}
   */
  lib() {
    return this.#lib;
  }

  /**
   * Get last error message.
   * @returns {string}
   */
  lastError() {
    const len = Number(this.#lib.symbols.polyplug_runtime_error_message_len(this.#ptr));
    if (len === 0) return "";
    const buf = new Uint8Array(len);
    const ptr = Deno.UnsafePointer.of(buf);
    this.#lib.symbols.polyplug_runtime_last_error(this.#ptr, ptr, BigInt(len));
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
 * Guard holds a ref-counted ResolveHandle that keeps the vtable alive.
 * Must call reset() to release the handle when done.
 */
export class Guard {
  #runtime;
  #resolveHandle;

  /**
   * @param {Runtime} runtime - Runtime instance
   * @param {bigint} packedHandle - Packed plugin handle
   */
  constructor(runtime, packedHandle) {
    this.#runtime = runtime;
    this.#resolveHandle = runtime.lib().symbols.polyplug_runtime_resolve_plugin(
      runtime.ptr(),
      packedHandle
    );
  }

  /**
   * Get the vtable pointer from the ResolveHandle.
   * @returns {Deno.PointerValue | null}
   */
  vtable() {
    if (this.#resolveHandle === null) {
      return null;
    }
    // ResolveHandle's first field is the vtable pointer
    const vtablePtr = new Deno.UnsafePointerView(this.#resolveHandle).getBigUint64(0);
    return Deno.UnsafePointer.create(vtablePtr);
  }

  /**
   * Check if this guard is valid.
   * @returns {boolean}
   */
  isValid() {
    return this.#resolveHandle !== null;
  }

  /**
   * Release the resolve handle.
   */
  reset() {
    if (this.#resolveHandle !== null) {
      this.#runtime.lib().symbols.polyplug_runtime_release_plugin(this.#resolveHandle);
      this.#resolveHandle = null;
    }
  }

  [Symbol.dispose]() {
    this.reset();
  }

  /**
   * Internal: resolve vtable for call (backwards compat).
   * @returns {Deno.PointerValue}
   */
  #resolveVtable() {
    const vt = this.vtable();
    if (vt === null) {
      throw new Error(`polyplug: guard is not valid`);
    }
    return vt;
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
    
    // PluginInterface layout (48 bytes):
    // - offset 0: rt_ctx (pointer, 8 bytes)
    // - offset 8: contract_id (u64, 8 bytes)
    // - offset 16: contract_version (u32, 4 bytes)
    // - offset 20: function_count (u32, 4 bytes)
    // - offset 24: dispatch_type (u32, 4 bytes)
    // - offset 28: _pad (u32, 4 bytes)
    // - offset 32: dispatch.native.functions (pointer, 8 bytes)
    const vtableBuf = new Deno.UnsafePointerView(vtablePtr).getArrayBuffer(48);
    const vtableView = new DataView(vtableBuf);
    const funcCount = vtableView.getUint32(20, true);
    const funcsPtr = vtableView.getBigUint64(32, true);
    
    if (funcIdx >= funcCount) {
      throw new Error(`function index ${funcIdx} out of bounds`);
    }
    
    // Read function pointer from funcs array
    const funcsBuf = new Deno.UnsafePointerView(Deno.UnsafePointer.create(funcsPtr)).getArrayBuffer(funcCount * 8);
    const funcs = new BigUint64Array(funcsBuf);
    const funcPtrBigInt = funcs[funcIdx];
    
    let func = _funcCache.get(funcPtrBigInt);
    if (!func) {
      const funcPtr = Deno.UnsafePointer.create(funcPtrBigInt);
      func = new Deno.UnsafeFnPointer(funcPtr, _DISPATCH_FN_DEF);
      _funcCache.set(funcPtrBigInt, func);
    }
    
    // Prepare input StringView struct (ptr: u64, len: u64) = 16 bytes
    const inputData = _encoder.encode(input);
    const inputPtr = Deno.UnsafePointer.of(inputData);
    const argsBuf = new Uint8Array(16);
    const argsView = new DataView(argsBuf.buffer);
    argsView.setBigUint64(0, Deno.UnsafePointer.value(inputPtr), true);
    argsView.setBigUint64(8, BigInt(inputData.length), true);
    const argsPtr = Deno.UnsafePointer.of(argsBuf);
    
    // Prepare output StringView struct (ptr: u64, len: u64) = 16 bytes
    const outBuf = new Uint8Array(16);
    const outPtr = Deno.UnsafePointer.of(outBuf);
    
    const result = func.call(argsPtr, outPtr);
    
    // result is AbiError struct: { code: u32, _pad: u32, message_ptr: pointer, message_len: usize }
    const errCode = result[0];
    
    if (errCode === 0) {
      const outView = new Deno.UnsafePointerView(outPtr);
      const resultPtr = outView.getBigUint64(0);
      const resultLen = Number(outView.getBigUint64(8));
      
      if (resultPtr !== 0n && resultLen > 0) {
        const resultPtrObj = Deno.UnsafePointer.create(resultPtr);
        const resultBuf = new Deno.UnsafePointerView(resultPtrObj).getArrayBuffer(resultLen);
        const outputStr = _decoder.decode(new Uint8Array(resultBuf));
        this.#runtime.lib().symbols.polyplug_host_free(resultPtrObj, BigInt(resultLen), 1);
        return outputStr;
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
  let ptr;

  if (_pendingConfig || _pendingReloadCallback) {
    const optionsBuf = new Uint8Array(24);
    const optionsView = new DataView(optionsBuf.buffer);
    let configBuf = null;

    if (_pendingConfig) {
      configBuf = new Uint8Array(17);
      const configView = new DataView(configBuf.buffer);
      configView.setUint32(0, _pendingConfig.hotReloadMaxRetries, true);
      configView.setBigUint64(4, BigInt(_pendingConfig.hotReloadRetryIntervalMs), true);
      configView.setUint8(12, _pendingConfig.hotReloadAbortOnMaxRetries ? 1 : 0);
      optionsView.setBigUint64(0, Deno.UnsafePointer.value(Deno.UnsafePointer.of(configBuf)), true);
    }

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
      optionsView.setBigUint64(8, Deno.UnsafePointer.value(_ffiReloadCallback.pointer), true);
    }

    const optionsPtr = Deno.UnsafePointer.of(optionsBuf);
    ptr = lib.symbols.polyplug_runtime_create_with_options(optionsPtr);
  } else {
    ptr = lib.symbols.polyplug_runtime_create();
  }

  if (ptr === null) {
    throw new Error("polyplug_runtime_create failed: unable to create runtime (no runtime pointer available for error details)");
  }
  return new Runtime(lib, ptr);
}