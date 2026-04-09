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

// Compatibility modes matching polyplug_abi::Compatibility (#[repr(u32)])
export const COMPATIBILITY_STRICT = 0;
export const COMPATIBILITY_RELAXED = 1;
export const COMPATIBILITY_YOLO = 2;

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
  polyplug_runtime_register_host_contract: { parameters: ["pointer", "pointer"], result: "u32" },
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
/** @type {{ hotReloadEnabled: boolean, hotReloadMaxRetries: number, hotReloadRetryIntervalMs: number, hotReloadAbortOnMaxRetries: boolean, compatibility: number } | null} */
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
 * Compute host contract ID using FNV-1a 64-bit hash.
 * Host contract IDs use a distinct prefix to avoid collisions with plugin contracts.
 * @param {string} name - Host contract name (must start with "host.", e.g., "host.logger")
 * @param {number} majorVersion - Major version number
 * @returns {bigint} 64-bit host contract ID
 */
export function hostContractId(name, majorVersion) {
    return fnv1a64(`host_contract:${name}@${majorVersion}`);
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
 * @param {Object} config - Configuration options
 * @param {boolean} [config.hotReloadEnabled=false] - Whether hot-reload is enabled
 * @param {number} [config.hotReloadMaxRetries=3] - Maximum retry attempts
 * @param {number} [config.hotReloadRetryIntervalMs=3000] - Interval between retries in ms
 * @param {boolean} [config.hotReloadAbortOnMaxRetries=true] - Abort after max retries
 * @param {number} [config.compatibility=0] - Compatibility mode (COMPATIBILITY_STRICT=0, RELAXED=1, YOLO=2)
 */
export function setConfig(config) {
    _pendingConfig = {
        hotReloadEnabled: config.hotReloadEnabled ?? false,
        hotReloadMaxRetries: config.hotReloadMaxRetries ?? 3,
        hotReloadRetryIntervalMs: config.hotReloadRetryIntervalMs ?? 3000,
        hotReloadAbortOnMaxRetries: config.hotReloadAbortOnMaxRetries ?? true,
        compatibility: config.compatibility ?? COMPATIBILITY_STRICT,
    };
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
   * Register a host contract interface with the runtime.
   * This allows VM-based hosts (JavaScript) to register host contract implementations.
   * @param {Deno.PointerValue} hostInterface - Pointer to a HostContractInterface struct
   * @throws {Error} If registration fails (null pointer, duplicate, or other error)
   */
  registerHostContract(hostInterface) {
    const result = this.#lib.symbols.polyplug_runtime_register_host_contract(this.#ptr, hostInterface);
    if (result === 1) {
      throw new Error("registerHostContract failed: null runtime or interface pointer");
    }
    if (result === 2) {
      throw new Error("registerHostContract failed: duplicate contract registration");
    }
    if (result === 3) {
      throw new Error(`registerHostContract failed: ${this.lastError()}`);
    }
    if (result !== 0) {
      throw new Error(`registerHostContract failed: unknown error code ${result}`);
    }
  }

  /**
   * Resolve plugin handle to raw pointer.
   * Returns the resolve handle pointer for instance-based model.
   * Host creates instances via interface.create_instance, calls methods,
   * and destroys instances via interface.destroy_instance before hot-reload.
   * @param {bigint} packedHandle - Packed plugin handle
   * @returns {Deno.PointerValue} Resolve handle pointer
   */
  resolvePlugin(packedHandle) {
    if (packedHandle === NULL_HANDLE) {
      return null;
    }
    return this.lib().symbols.polyplug_runtime_resolve_plugin(
      this.ptr(),
      packedHandle
    );
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
      // RuntimeConfig is 24 bytes matching polyplug_abi::RuntimeConfig
      configBuf = new Uint8Array(24);
      const configView = new DataView(configBuf.buffer);

      // offset 0: hot_reload_enabled (1 byte bool)
      configView.setUint8(0, _pendingConfig.hotReloadEnabled ? 1 : 0);
      // offset 4: hot_reload_max_retries (4 bytes u32)
      configView.setUint32(4, _pendingConfig.hotReloadMaxRetries, true);
      // offset 8: hot_reload_retry_interval_ms (8 bytes u64)
      configView.setBigUint64(8, BigInt(_pendingConfig.hotReloadRetryIntervalMs), true);
      // offset 16: hot_reload_abort_on_max_retries (1 byte bool)
      configView.setUint8(16, _pendingConfig.hotReloadAbortOnMaxRetries ? 1 : 0);
      // offset 20: compatibility (4 bytes u32 enum)
      configView.setUint32(20, _pendingConfig.compatibility, true);

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