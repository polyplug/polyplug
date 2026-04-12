/**
 * @file polyplug.js
 * @description Host library for polyplug JavaScript/TypeScript hosts.
 *
 * Updated for HostInterface-based API (18-04 refactor).
 * All operations are accessed through HostInterface struct fields,
 * not via separate FFI functions.
 * Offset constants imported from auto-generated abi.ts (per D-26).
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

// Import offset constants from the auto-generated abi.ts
import {
  HOST_INTERFACE_RUNTIME_OFFSET,
  HOST_INTERFACE_REGISTER_CONTRACT_OFFSET,
  HOST_INTERFACE_ALLOC_OFFSET,
  HOST_INTERFACE_FREE_OFFSET,
  HOST_INTERFACE_FIND_GUEST_CONTRACT_OFFSET,
  HOST_INTERFACE_FIND_ALL_GUEST_CONTRACTS_OFFSET,
  HOST_INTERFACE_RESOLVE_GUEST_CONTRACT_OFFSET,
  HOST_INTERFACE_CALL_GUEST_METHOD_OFFSET,
  HOST_INTERFACE_GET_HOST_CONTRACT_OFFSET,
  HOST_INTERFACE_RESOLVE_HOST_CONTRACT_INTERFACE_OFFSET,
  HOST_INTERFACE_LIST_BUNDLES_OFFSET,
  HOST_INTERFACE_GET_DEPENDENCIES_OFFSET,
  HOST_INTERFACE_LOAD_BUNDLE_OFFSET,
  HOST_INTERFACE_RELOAD_BUNDLE_OFFSET,
  HOST_INTERFACE_REGISTER_HOST_CONTRACT_OFFSET,
  HOST_INTERFACE_REGISTER_LOADER_OFFSET,
  HOST_INTERFACE_GET_LAST_ERROR_OFFSET,
  HOST_INTERFACE_GET_ERROR_LEN_OFFSET,
  RUNTIME_CONFIG_COMPATIBILITY_OFFSET,
  RUNTIME_CONFIG_HOT_RELOAD_ENABLED_OFFSET,
  RUNTIME_CONFIG_ON_RELOAD_OFFSET,
  RUNTIME_CONFIG_SIZE,
} from "../../abi/abi.ts";

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

// ─── FFI Symbols: Only create and destroy ───────────────────────────────────────
// All operations are accessed through HostInterface struct fields.
const SYMBOLS = {
  polyplug_runtime_create: { parameters: [], result: "pointer" },
  polyplug_runtime_create_with_options: { parameters: ["pointer"], result: "pointer" },
  polyplug_runtime_destroy: { parameters: ["pointer"], result: "void" },
};

// HostInterface struct offsets imported from auto-generated abi.ts (144 bytes, 18 pointer fields)
const HOST_INTERFACE_OFFSETS = {
  runtime: HOST_INTERFACE_RUNTIME_OFFSET,
  register_contract: HOST_INTERFACE_REGISTER_CONTRACT_OFFSET,
  alloc: HOST_INTERFACE_ALLOC_OFFSET,
  free: HOST_INTERFACE_FREE_OFFSET,
  find_guest_contract: HOST_INTERFACE_FIND_GUEST_CONTRACT_OFFSET,
  find_all_guest_contracts: HOST_INTERFACE_FIND_ALL_GUEST_CONTRACTS_OFFSET,
  resolve_guest_contract: HOST_INTERFACE_RESOLVE_GUEST_CONTRACT_OFFSET,
  call_guest_method: HOST_INTERFACE_CALL_GUEST_METHOD_OFFSET,
  get_host_contract: HOST_INTERFACE_GET_HOST_CONTRACT_OFFSET,
  resolve_host_contract_interface: HOST_INTERFACE_RESOLVE_HOST_CONTRACT_INTERFACE_OFFSET,
  list_bundles: HOST_INTERFACE_LIST_BUNDLES_OFFSET,
  get_dependencies: HOST_INTERFACE_GET_DEPENDENCIES_OFFSET,
  load_bundle: HOST_INTERFACE_LOAD_BUNDLE_OFFSET,
  reload_bundle: HOST_INTERFACE_RELOAD_BUNDLE_OFFSET,
  register_host_contract: HOST_INTERFACE_REGISTER_HOST_CONTRACT_OFFSET,
  register_loader: HOST_INTERFACE_REGISTER_LOADER_OFFSET,
  get_last_error: HOST_INTERFACE_GET_LAST_ERROR_OFFSET,
  get_error_len: HOST_INTERFACE_GET_ERROR_LEN_OFFSET,
};

// Module-level caches for hot path performance
const _funcCache = new Map();
const _encoder = new TextEncoder();
const _decoder = new TextDecoder();

// Module-level pending callbacks for hot-reload notification
/** @type {function(ReloadPhase): void | null} */
let _pendingReloadCallback = null;
/** @type {{ hotReloadEnabled: boolean, compatibility: number } | null} */
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
 * Per D-22: RuntimeConfig is 16 bytes (compatibility, hot_reload_enabled, on_reload).
 * @param {Object} config - Configuration options
 * @param {boolean} [config.hotReloadEnabled=false] - Whether hot-reload is enabled
 * @param {number} [config.compatibility=0] - Compatibility mode (COMPATIBILITY_STRICT=0, RELAXED=1, YOLO=2)
 */
export function setConfig(config) {
    _pendingConfig = {
        hotReloadEnabled: config.hotReloadEnabled ?? false,
        compatibility: config.compatibility ?? COMPATIBILITY_STRICT,
    };
}

/**
 * Read a function pointer from HostInterface at given offset.
 * @param {Deno.PointerValue} hostPtr - HostInterface pointer
 * @param {number} offset - Byte offset in struct
 * @returns {Deno.PointerValue} Function pointer
 */
function readHostField(hostPtr, offset) {
  const view = new Deno.UnsafePointerView(hostPtr);
  return view.getBigUint64(offset);
}

/**
 * Call a HostInterface method with self-passing pattern.
 * @param {Deno.PointerValue} hostPtr - HostInterface pointer
 * @param {number} fieldOffset - Offset of the function pointer field
 * @param {Array} paramTypes - FFI parameter types
 * @param {string} resultType - FFI result type
 * @param {Array} args - Arguments to pass (first arg is always hostPtr)
 * @returns {*} Result from FFI call
 */
function callHostMethod(hostPtr, fieldOffset, paramTypes, resultType, args) {
  const funcPtr = readHostField(hostPtr, fieldOffset);
  if (funcPtr === 0n) {
    throw new Error(`HostInterface field at offset ${fieldOffset} is null`);
  }

  // Create function definition for this call
  const fnDef = { parameters: paramTypes, result: resultType };

  // Call through the function pointer
  const func = new Deno.UnsafeFnPointer(funcPtr, fnDef);
  return func.call(...args);
}

/**
 * Runtime class using HostInterface-based API.
 * All operations call through HostInterface struct fields.
 */
export class Runtime {
  #lib;
  #host;  // HostInterface pointer

  /**
   * @param {Deno.DynamicLibrary} lib - Dynamic library instance
   * @param {Deno.PointerValue} host - HostInterface pointer
   */
  constructor(lib, host) {
    this.#lib = lib;
    this.#host = host;
  }

  [Symbol.dispose]() {
    this.#lib.symbols.polyplug_runtime_destroy(this.#host);
  }

  /**
   * Get HostInterface pointer.
   * @returns {Deno.PointerValue}
   */
  host() {
    return this.#host;
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
   * Calls through HostInterface.get_last_error and get_error_len fields.
   * @returns {string}
   */
  lastError() {
    // Get error length via get_error_len
    const len = Number(callHostMethod(
      this.#host,
      HOST_INTERFACE_OFFSETS.get_error_len,
      ["pointer"],
      "usize",
      [this.#host]
    ));

    if (len === 0) return "";

    // Get error message via get_last_error
    const buf = new Uint8Array(len);
    const bufPtr = Deno.UnsafePointer.of(buf);
    callHostMethod(
      this.#host,
      HOST_INTERFACE_OFFSETS.get_last_error,
      ["pointer", "pointer", "usize"],
      "usize",
      [this.#host, bufPtr, BigInt(len)]
    );

    return _decoder.decode(buf);
  }

  /**
   * Load a plugin bundle.
   * Calls through HostInterface.load_bundle field.
   * @param {string} path - Path to bundle directory
   */
  loadBundle(path) {
    const encoded = _encoder.encode(path);
    const ptr = Deno.UnsafePointer.of(encoded);
    const result = callHostMethod(
      this.#host,
      HOST_INTERFACE_OFFSETS.load_bundle,
      ["pointer", "pointer", "usize"],
      "u32",
      [this.#host, ptr, BigInt(encoded.length)]
    );
    if (result !== 0) {
      throw new Error(`loadBundle failed: ${this.lastError()}`);
    }
  }

  /**
   * Reload a plugin bundle (hot-reload).
   * Calls through HostInterface.reload_bundle field.
   * @param {string} path - Path to bundle directory
   */
  reloadBundle(path) {
    const encoded = _encoder.encode(path);
    const ptr = Deno.UnsafePointer.of(encoded);
    const result = callHostMethod(
      this.#host,
      HOST_INTERFACE_OFFSETS.reload_bundle,
      ["pointer", "pointer", "usize"],
      "u32",
      [this.#host, ptr, BigInt(encoded.length)]
    );
    if (result !== 0) {
      throw new Error(`reloadBundle failed: ${this.lastError()}`);
    }
  }

  /**
   * Find guest contract by contract ID.
   * Calls through HostInterface.find_guest_contract field.
   * @param {bigint} contractId - Contract identifier
   * @param {number} [minVersion=0] - Minimum version
   * @returns {bigint} Plugin handle
   */
  findGuestContract(contractId, minVersion = 0) {
    return callHostMethod(
      this.#host,
      HOST_INTERFACE_OFFSETS.find_guest_contract,
      ["pointer", "u64", "u32"],
      "u64",
      [this.#host, contractId, minVersion]
    );
  }

  /**
   * Find plugin by bundle ID (deprecated, not in HostInterface).
   * Returns NULL_HANDLE since this was removed from FFI surface.
   * @param {bigint} bundleId - Bundle identifier
   * @param {bigint} contractId - Contract identifier
   * @param {number} [minVersion=0] - Minimum version
   * @returns {bigint} NULL_HANDLE (not implemented)
   */
  findByBundle(bundleId, contractId, minVersion = 0) {
    // Note: find_by_bundle is not in HostInterface (18-02 removed from FFI surface)
    return NULL_HANDLE;
  }

  /**
   * Find all guest contracts by contract ID.
   * Calls through HostInterface.find_all_guest_contracts field.
   * @param {bigint} contractId - Contract identifier
   * @param {number} [minVersion=0] - Minimum version
   * @param {number} [cap=64] - Buffer capacity
   * @returns {bigint[]} Array of plugin handles
   */
  findAllGuestContracts(contractId, minVersion = 0, cap = 64) {
    // The function returns Array<GuestContractHandle> struct { ptr, len }
    // We need to call and then read the result struct
    const resultPtr = callHostMethod(
      this.#host,
      HOST_INTERFACE_OFFSETS.find_all_guest_contracts,
      ["pointer", "u64", "u32"],
      "pointer",
      [this.#host, contractId, minVersion]
    );

    // Read Array struct: { ptr: pointer, len: usize }
    const view = new Deno.UnsafePointerView(resultPtr);
    const arrPtr = view.getPointer(0);
    const arrLen = Number(view.getBigUint64(8));

    if (arrPtr === null || arrLen === 0) {
      return [];
    }

    // Read handles from array
    const handles = [];
    const arrView = new Deno.UnsafePointerView(arrPtr);
    for (let i = 0; i < Math.min(arrLen, cap); i++) {
      handles.push(arrView.getBigUint64(i * 8));
    }

    // Free the array via HostInterface.free
    if (arrLen > 0) {
      callHostMethod(
        this.#host,
        HOST_INTERFACE_OFFSETS.free,
        ["pointer", "pointer", "usize", "usize"],
        "void",
        [this.#host, arrPtr, BigInt(arrLen * 8), BigInt(8)]
      );
    }

    return handles;
  }

  /**
   * Resolve plugin handle to raw pointer.
   * Calls through HostInterface.resolve_guest_contract field.
   * @param {bigint} packedHandle - Packed plugin handle
   * @returns {Deno.PointerValue} Resolve handle pointer
   */
  resolveGuestContract(packedHandle) {
    if (packedHandle === NULL_HANDLE) {
      return null;
    }
    return callHostMethod(
      this.#host,
      HOST_INTERFACE_OFFSETS.resolve_guest_contract,
      ["pointer", "u64"],
      "pointer",
      [this.#host, packedHandle]
    );
  }

  /**
   * Register a host contract interface with the runtime.
   * Calls through HostInterface.register_host_contract field.
   * @param {Deno.PointerValue} hostInterface - Pointer to HostContractInterface struct
   */
  registerHostContract(hostInterface) {
    const result = callHostMethod(
      this.#host,
      HOST_INTERFACE_OFFSETS.register_host_contract,
      ["pointer", "pointer"],
      "u32",
      [this.#host, hostInterface]
    );
    if (result === 1) {
      throw new Error("registerHostContract: null interface pointer");
    } else if (result === 2) {
      throw new Error("registerHostContract: duplicate contract registration");
    } else if (result !== 0) {
      throw new Error(`registerHostContract failed: ${this.lastError()}`);
    }
  }

  // ─── Backward Compatibility Aliases ───────────────────────────────────────────

  /**
   * Alias for findGuestContract (deprecated).
   * @deprecated Use findGuestContract instead.
   */
  findByContract(contractId, minVersion = 0) {
    return this.findGuestContract(contractId, minVersion);
  }

  /**
   * Alias for findAllGuestContracts (deprecated).
   * @deprecated Use findAllGuestContracts instead.
   */
  findAllByContract(contractId, minVersion = 0, cap = 64) {
    return this.findAllGuestContracts(contractId, minVersion, cap);
  }

  /**
   * Alias for resolveGuestContract (deprecated).
   * @deprecated Use resolveGuestContract instead.
   */
  resolvePlugin(packedHandle) {
    return this.resolveGuestContract(packedHandle);
  }

  /**
   * Get runtime pointer (deprecated).
   * @deprecated Use host() instead.
   */
  ptr() {
    return this.#host;
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
 * Uses HostInterface-based API: polyplug_runtime_create returns HostInterface*.
 * Per D-22: RuntimeConfig is 16 bytes (compatibility, hot_reload_enabled, on_reload).
 * @param {Deno.DynamicLibrary} lib - Dynamic library
 * @returns {Runtime}
 */
export function runtimeNew(lib) {
  let host;

  if (_pendingConfig || _pendingReloadCallback) {
    // RuntimeConfig is 16 bytes per D-22: compatibility(u32) + padding(4) + hot_reload_enabled(bool/u8) + padding(7) + on_reload(fn ptr)
    const configBuf = new Uint8Array(RUNTIME_CONFIG_SIZE);
    const configView = new DataView(configBuf.buffer);

    if (_pendingConfig) {
      // offset 0: compatibility (4 bytes u32)
      configView.setUint32(RUNTIME_CONFIG_COMPATIBILITY_OFFSET, _pendingConfig.compatibility, true);
      // offset 8: hot_reload_enabled (1 byte bool)
      configView.setUint8(RUNTIME_CONFIG_HOT_RELOAD_ENABLED_OFFSET, _pendingConfig.hotReloadEnabled ? 1 : 0);
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
      // offset 16: on_reload (fn pointer, 8 bytes)
      configView.setBigUint64(RUNTIME_CONFIG_ON_RELOAD_OFFSET, Deno.UnsafePointer.value(_ffiReloadCallback.pointer), true);
    }

    const configPtr = Deno.UnsafePointer.of(configBuf);
    host = lib.symbols.polyplug_runtime_create_with_options(configPtr);
  } else {
    host = lib.symbols.polyplug_runtime_create();
  }

  if (host === null) {
    throw new Error("polyplug_runtime_create failed: unable to create runtime (returned null HostInterface)");
  }
  return new Runtime(lib, host);
}
