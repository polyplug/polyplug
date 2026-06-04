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
  GUEST_CONTRACT_INTERFACE_DISPATCH_TYPE_OFFSET,
  GUEST_CONTRACT_INTERFACE_CREATE_INSTANCE_OFFSET,
  GUEST_CONTRACT_INTERFACE_DESTROY_INSTANCE_OFFSET,
  GUEST_CONTRACT_INTERFACE_DISPATCH_OFFSET,
  GUEST_CONTRACT_INSTANCE_SIZE,
  NATIVE_DISPATCH_FUNCTION_COUNT_OFFSET,
  NATIVE_DISPATCH_FUNCTIONS_OFFSET,
  VM_DISPATCH_CALL_OFFSET,
  VM_DISPATCH_LOADER_DATA_OFFSET,
} from "../../abi/abi.ts";

// DispatchType discriminants (match polyplug_abi::DispatchType #[repr(u32)]).
const DISPATCH_TYPE_NATIVE = 0;
const DISPATCH_TYPE_VIRTUAL_MACHINE = 1;

// AbiError is returned by value from dispatch as a 24-byte struct
// { code: u32, _pad: u32, message: StringView{ ptr, len } }; code is the first u32.
const ABI_ERROR_STRUCT = { struct: ["u32", "u32", "pointer", "usize"] };
// GuestContractInstance crosses the ABI by value as { data: ptr, contract_id: u64 }.
const GUEST_CONTRACT_INSTANCE_STRUCT = { struct: ["pointer", "u64"] };

export {
  getPlatformIdentifier,
  getNativeLibraryFilename,
  loadNativeLibrary,
  openNativeLibrary
} from "./native-loader.ts";

/**
 * Null/invalid GuestContractHandle sentinel.
 *
 * GuestContractHandle is `#[repr(C)] { index: u32 }` (4 bytes). The null handle
 * is `index == u32::MAX`. A single-field 4-byte repr(C) struct crosses the C ABI
 * as a `u32`, so the handle is a plain JS number, not a bigint.
 * @type {number}
 */
export const NULL_HANDLE = 0xFFFFFFFF;

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
  polyplug_runtime_create: { parameters: ["pointer"], result: "pointer" },
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
 * Compute guest contract ID using FNV-1a 64-bit hash.
 * Guest contract IDs use a distinct prefix to avoid collisions with host contracts.
 * @param {string} name - Contract name (e.g., "pipeline.Decoder")
 * @param {number} majorVersion - Major version number
 * @returns {bigint} 64-bit contract ID
 */
export function contractId(name, majorVersion) {
    return fnv1a64(`guest_contract:${name}@${majorVersion}`);
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
 * The raw 64-bit value is wrapped into a Deno pointer object so it can be
 * passed to Deno.UnsafeFnPointer (which rejects bare BigInts).
 * @param {Deno.PointerValue} hostPtr - HostInterface pointer
 * @param {number} offset - Byte offset in struct
 * @returns {Deno.PointerValue} Function pointer
 */
function readHostField(hostPtr, offset) {
  const view = new Deno.UnsafePointerView(hostPtr);
  return Deno.UnsafePointer.create(view.getBigUint64(offset));
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
  if (funcPtr === null) {
    throw new Error(`HostInterface field at offset ${fieldOffset} is null`);
  }

  // Create function definition for this call
  const fnDef = { parameters: paramTypes, result: resultType };

  // Call through the function pointer
  const func = new Deno.UnsafeFnPointer(funcPtr, fnDef);
  return func.call(...args);
}

/**
 * Decoded view over a raw `GuestContractInterface*` pointer.
 *
 * `resolveGuestContract` returns a raw `Deno.PointerValue`; Deno FFI does not
 * auto-decode C structs. This view reads the `#[repr(C)] GuestContractInterface`
 * fields at their byte offsets (see polyplug_abi guest_contract_interface.rs and
 * the auto-generated abi.ts offset constants) and exposes the lifecycle function
 * pointers, the dispatch type, the function count, and a per-slot dispatch entry
 * callable as `dispatch(slot, instance, argsPtr, outPtr)`.
 *
 * Layout (56 bytes):
 *   contract_id (u64)        @ 0
 *   contract_version (12)    @ 8
 *   dispatch_type (u32)      @ 20
 *   create_instance (fn ptr) @ 24
 *   destroy_instance (fn ptr)@ 32
 *   dispatch (union, 16)     @ 40  (Native: function_count u32 @ +0, functions ptr @ +8)
 *
 * Dispatch is routed through `HostInterface.call_guest_method`, which is
 * dispatch-type agnostic (the runtime selects native vs VM internally), so this
 * view works identically for native guests and VM (QuickJS/Lua/Python) guests.
 *
 * Validity keys off the interface pointer, never off instance data: a null
 * `instance.data` is a VALID dispatch token because the runtime substitutes
 * stateless stubs for null lifecycle pointers and stateless contracts return a
 * null instance handle.
 */
export class GuestContractInterfaceView {
  #host;          // HostInterface pointer
  #interfacePtr;  // raw GuestContractInterface* (Deno.PointerValue)
  #dispatchType;
  #functionCount;
  #createInstancePtr;
  #destroyInstancePtr;
  #nativeFunctionsPtr;  // *const *const () (Native dispatch)
  #vmCallPtr;           // VmDispatch.call fn ptr (VM dispatch)
  #vmLoaderData;        // VmLoaderData (raw u64) (VM dispatch)
  #fnPtrCache;          // Map<slot, Deno.UnsafeFnPointer> for native dispatch

  /**
   * @param {Deno.PointerValue} host - HostInterface pointer
   * @param {Deno.PointerValue} interfacePtr - Raw GuestContractInterface pointer
   */
  constructor(host, interfacePtr) {
    this.#host = host;
    this.#interfacePtr = interfacePtr;
    this.#fnPtrCache = new Map();

    const view = new Deno.UnsafePointerView(interfacePtr);
    this.#dispatchType = view.getUint32(GUEST_CONTRACT_INTERFACE_DISPATCH_TYPE_OFFSET);
    this.#createInstancePtr = Deno.UnsafePointer.create(
      view.getBigUint64(GUEST_CONTRACT_INTERFACE_CREATE_INSTANCE_OFFSET)
    );
    this.#destroyInstancePtr = Deno.UnsafePointer.create(
      view.getBigUint64(GUEST_CONTRACT_INTERFACE_DESTROY_INSTANCE_OFFSET)
    );

    const dispatchBase = GUEST_CONTRACT_INTERFACE_DISPATCH_OFFSET;
    if (this.#dispatchType === DISPATCH_TYPE_VIRTUAL_MACHINE) {
      // VmDispatch { call: fn ptr @ 0, loader_data: VmLoaderData @ 8 }.
      this.#functionCount = 0;
      this.#vmCallPtr = Deno.UnsafePointer.create(
        view.getBigUint64(dispatchBase + VM_DISPATCH_CALL_OFFSET)
      );
      this.#vmLoaderData = view.getBigUint64(dispatchBase + VM_DISPATCH_LOADER_DATA_OFFSET);
      this.#nativeFunctionsPtr = null;
    } else {
      // NativeDispatch { function_count: u32 @ 0, functions: *const *const () @ 8 }.
      this.#functionCount = view.getUint32(dispatchBase + NATIVE_DISPATCH_FUNCTION_COUNT_OFFSET);
      this.#nativeFunctionsPtr = Deno.UnsafePointer.create(
        view.getBigUint64(dispatchBase + NATIVE_DISPATCH_FUNCTIONS_OFFSET)
      );
      this.#vmCallPtr = null;
      this.#vmLoaderData = 0n;
    }
  }

  /** @returns {Deno.PointerValue} Raw interface pointer (validity token). */
  interfacePtr() {
    return this.#interfacePtr;
  }

  /** @returns {boolean} True when the underlying interface pointer is non-null. */
  isValid() {
    return this.#interfacePtr !== null;
  }

  /** @returns {number} Dispatch type (0 = Native, 1 = VirtualMachine). */
  dispatchType() {
    return this.#dispatchType;
  }

  /** @returns {number} Number of dispatchable functions (native dispatch). */
  functionCount() {
    return this.#functionCount;
  }

  /**
   * Create an instance via the interface `create_instance` factory.
   *
   * Returns the raw 16-byte `GuestContractInstance` struct ({ data, contract_id })
   * as a Uint8Array, passed back by value into dispatch/destroy. A null
   * `instance.data` is valid for stateless contracts.
   * @returns {Uint8Array} GuestContractInstance struct (16 bytes).
   */
  createInstance() {
    if (this.#createInstancePtr === null) {
      // Null create_instance: the runtime substitutes a stateless stub, but if a
      // raw null pointer survived, fall back to a zeroed (null-data) instance.
      return new Uint8Array(GUEST_CONTRACT_INSTANCE_SIZE);
    }
    // create_instance(host: *const HostInterface, args: *const ()) -> GuestContractInstance
    const fn = new Deno.UnsafeFnPointer(this.#createInstancePtr, {
      parameters: ["pointer", "pointer"],
      result: { struct: ["pointer", "u64"] },
    });
    const result = fn.call(this.#host, null);
    // Normalize the struct result into a 16-byte buffer for by-value re-passing.
    const instance = new Uint8Array(GUEST_CONTRACT_INSTANCE_SIZE);
    instance.set(new Uint8Array(result.buffer, result.byteOffset, result.byteLength));
    return instance;
  }

  /**
   * Destroy an instance via the interface `destroy_instance` function.
   * @param {Uint8Array} instance - GuestContractInstance struct (16 bytes).
   */
  destroyInstance(instance) {
    if (this.#destroyInstancePtr === null) {
      return;
    }
    // destroy_instance(host: *const HostInterface, instance: GuestContractInstance)
    const fn = new Deno.UnsafeFnPointer(this.#destroyInstancePtr, {
      parameters: ["pointer", { struct: ["pointer", "u64"] }],
      result: "void",
    });
    fn.call(this.#host, instance);
  }

  /**
   * Dispatch a method directly through the resolved interface.
   *
   * This mirrors the canonical host-caller path (see polyplugc rust generator):
   * - Native: call `dispatch.native.functions[slot](instance, args, out) -> AbiError`.
   * - VM: call `dispatch.vm.call(loader_data, instance, fn_id, args, out) -> AbiError`.
   *
   * `HostInterface.call_guest_method` is intentionally NOT used: it is a stub that
   * only supports cross-instance routing (not yet implemented) and rejects a null
   * `instance.data`. Direct interface dispatch is the supported mechanism and works
   * for both native and VM (QuickJS/Lua/Python) guests, including stateless ones
   * whose instance carries a null `data`.
   * @param {number} slot - function_id / method index.
   * @param {Uint8Array} instance - GuestContractInstance struct (16 bytes, by value).
   * @param {Deno.PointerValue} argsPtr - Pointer to packed args (or null).
   * @param {Deno.PointerValue} outPtr - Pointer to output buffer (or null).
   * @returns {number} AbiError code (0 = Ok).
   */
  dispatch(slot, instance, argsPtr, outPtr) {
    let result;
    if (this.#dispatchType === DISPATCH_TYPE_VIRTUAL_MACHINE) {
      if (this.#vmCallPtr === null) {
        return 8; // AbiErrorCode::InvalidPointer — null VM dispatch function.
      }
      // call(loader_data: VmLoaderData, instance, fn_id: u32, args, out) -> AbiError.
      // VmLoaderData is a single opaque pointer (`{ data: *mut c_void }`).
      const fn = new Deno.UnsafeFnPointer(this.#vmCallPtr, {
        parameters: ["pointer", GUEST_CONTRACT_INSTANCE_STRUCT, "u32", "pointer", "pointer"],
        result: ABI_ERROR_STRUCT,
      });
      const loaderData = Deno.UnsafePointer.create(this.#vmLoaderData);
      result = fn.call(loaderData, instance, slot, argsPtr, outPtr);
    } else {
      const fn = this.#nativeFnPointer(slot);
      if (fn === null) {
        return 8; // AbiErrorCode::InvalidPointer — null native function slot.
      }
      // functions[slot](instance, args, out) -> AbiError.
      result = fn.call(instance, argsPtr, outPtr);
    }
    return new DataView(result.buffer, result.byteOffset, result.byteLength).getUint32(0, true);
  }

  /**
   * Resolve (and cache) the native dispatch `Deno.UnsafeFnPointer` for `slot`.
   * @param {number} slot - function_id / method index.
   * @returns {Deno.UnsafeFnPointer | null}
   */
  #nativeFnPointer(slot) {
    const cached = this.#fnPtrCache.get(slot);
    if (cached !== undefined) {
      return cached;
    }
    if (this.#nativeFunctionsPtr === null) {
      return null;
    }
    // functions is `*const *const ()`: read the slot-th 8-byte pointer entry.
    const slotPtrRaw = new Deno.UnsafePointerView(this.#nativeFunctionsPtr).getBigUint64(slot * 8);
    const fnPtr = Deno.UnsafePointer.create(slotPtrRaw);
    if (fnPtr === null) {
      return null;
    }
    const fn = new Deno.UnsafeFnPointer(fnPtr, {
      parameters: [GUEST_CONTRACT_INSTANCE_STRUCT, "pointer", "pointer"],
      result: ABI_ERROR_STRUCT,
    });
    this.#fnPtrCache.set(slot, fn);
    return fn;
  }
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
    // HostInterface.load_bundle returns AbiError (24-byte struct), not u32.
    const result = callHostMethod(
      this.#host,
      HOST_INTERFACE_OFFSETS.load_bundle,
      ["pointer", "pointer", "usize"],
      { struct: ["u32", "u32", "pointer", "usize"] },
      [this.#host, ptr, BigInt(encoded.length)]
    );
    const code = new DataView(result.buffer, result.byteOffset, result.byteLength).getUint32(0, true);
    if (code !== 0) {
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
    // HostInterface.reload_bundle returns AbiError (24-byte struct), not u32.
    const result = callHostMethod(
      this.#host,
      HOST_INTERFACE_OFFSETS.reload_bundle,
      ["pointer", "pointer", "usize"],
      { struct: ["u32", "u32", "pointer", "usize"] },
      [this.#host, ptr, BigInt(encoded.length)]
    );
    const code = new DataView(result.buffer, result.byteOffset, result.byteLength).getUint32(0, true);
    if (code !== 0) {
      throw new Error(`reloadBundle failed: ${this.lastError()}`);
    }
  }

  /**
   * Find guest contract by contract ID.
   * Calls through HostInterface.find_guest_contract field.
   *
   * Returns a GuestContractHandle, which is `#[repr(C)] { index: u32 }` and
   * crosses the C ABI as a `u32`. The result is therefore a JS number;
   * NULL_HANDLE (u32::MAX) signals "no matching contract".
   * @param {bigint} contractId - Contract identifier
   * @param {number} [minVersion=0] - Minimum version
   * @returns {number} Guest contract handle index (u32)
   */
  findGuestContract(contractId, minVersion = 0) {
    return callHostMethod(
      this.#host,
      HOST_INTERFACE_OFFSETS.find_guest_contract,
      ["pointer", "u64", "u32"],
      "u32",
      [this.#host, contractId, minVersion]
    );
  }

  /**
   * Find plugin by bundle ID (deprecated, not in HostInterface).
   * Returns NULL_HANDLE since this was removed from FFI surface.
   * @param {bigint} bundleId - Bundle identifier
   * @param {bigint} contractId - Contract identifier
   * @param {number} [minVersion=0] - Minimum version
   * @returns {number} NULL_HANDLE (not implemented)
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
   * @returns {number[]} Array of guest contract handle indices (u32 each)
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

    // Read handles from array. GuestContractHandle is `#[repr(C)] { index: u32 }`
    // (4 bytes), so elements have a 4-byte stride and are read as u32.
    const handles = [];
    const arrView = new Deno.UnsafePointerView(arrPtr);
    for (let i = 0; i < Math.min(arrLen, cap); i++) {
      handles.push(arrView.getUint32(i * 4));
    }

    // Free the array via HostInterface.free.
    // GuestContractHandle is `#[repr(C)] { index: u32 }` (4 bytes, align 4),
    // so the allocation size is `arrLen * 4` and alignment is 4 — matching the
    // 4-byte stride used above when reading the handles.
    if (arrLen > 0) {
      callHostMethod(
        this.#host,
        HOST_INTERFACE_OFFSETS.free,
        ["pointer", "pointer", "usize", "usize"],
        "void",
        [this.#host, arrPtr, BigInt(arrLen * 4), BigInt(4)]
      );
    }

    return handles;
  }

  /**
   * Resolve a guest contract handle to a raw interface pointer.
   * Calls through HostInterface.resolve_guest_contract field.
   *
   * The handle is a GuestContractHandle (`#[repr(C)] { index: u32 }`) passed by
   * value, which crosses the C ABI as a `u32`.
   * @param {number} handle - Guest contract handle index (u32)
   * @returns {Deno.PointerValue} Resolve handle pointer
   */
  resolveGuestContract(handle) {
    if (handle === NULL_HANDLE) {
      return null;
    }
    return callHostMethod(
      this.#host,
      HOST_INTERFACE_OFFSETS.resolve_guest_contract,
      ["pointer", "u32"],
      "pointer",
      [this.#host, handle]
    );
  }

  /**
   * Resolve a guest contract handle to a decoded interface view.
   *
   * Wraps `resolveGuestContract` (raw pointer) in a {@link GuestContractInterfaceView}
   * that decodes the `#[repr(C)] GuestContractInterface` fields and exposes the
   * lifecycle function pointers, dispatch type, function count, and a per-slot
   * dispatch entry. Returns null when the handle does not resolve.
   * @param {number} handle - Guest contract handle index (u32)
   * @returns {GuestContractInterfaceView | null}
   */
  resolveGuestContractInterface(handle) {
    const interfacePtr = this.resolveGuestContract(handle);
    if (interfacePtr === null) {
      return null;
    }
    return new GuestContractInterfaceView(this.#host, interfacePtr);
  }

  /**
   * Allocate `size` bytes via the host allocator (HostInterface.alloc).
   *
   * All memory crossing the plugin boundary must use the host allocator. The
   * returned pointer must be released via {@link Runtime#free} with the same
   * size and alignment.
   * @param {number} size - Number of bytes to allocate.
   * @param {number} [align=1] - Allocation alignment.
   * @returns {Deno.PointerValue} Pointer to the allocated region (or null).
   */
  alloc(size, align = 1) {
    return callHostMethod(
      this.#host,
      HOST_INTERFACE_OFFSETS.alloc,
      ["pointer", "usize", "usize"],
      "pointer",
      [this.#host, BigInt(size), BigInt(align)]
    );
  }

  /**
   * Free a region previously returned by {@link Runtime#alloc}.
   * @param {Deno.PointerValue} ptr - Pointer to free.
   * @param {number} size - Size used at allocation time.
   * @param {number} [align=1] - Alignment used at allocation time.
   */
  free(ptr, size, align = 1) {
    if (ptr === null) {
      return;
    }
    callHostMethod(
      this.#host,
      HOST_INTERFACE_OFFSETS.free,
      ["pointer", "pointer", "usize", "usize"],
      "void",
      [this.#host, ptr, BigInt(size), BigInt(align)]
    );
  }

  /**
   * Register a host contract interface with the runtime.
   * Calls through HostInterface.register_host_contract field.
   * @param {Deno.PointerValue} hostInterface - Pointer to HostContractInterface struct
   */
  registerHostContract(hostInterface) {
    // HostInterface.register_host_contract returns AbiError (24-byte struct), not u32.
    const result = callHostMethod(
      this.#host,
      HOST_INTERFACE_OFFSETS.register_host_contract,
      ["pointer", "pointer"],
      { struct: ["u32", "u32", "pointer", "usize"] },
      [this.#host, hostInterface]
    );
    const code = new DataView(result.buffer, result.byteOffset, result.byteLength).getUint32(0, true);
    if (code !== 0) {
      throw new Error(`registerHostContract failed: ${this.lastError()}`);
    }
  }

  /**
   * Register a language loader with the runtime.
   * Calls through HostInterface.register_loader field. The StringView runtime
   * name is passed by value (ptr + len); the AbiError return is read as a
   * struct by value (code is the first u32).
   * @param {string} runtimeName - Runtime name the loader handles (e.g. "native", "lua").
   * @param {Deno.PointerValue} loaderPtr - Opaque loader pointer from the loader cdylib's create function.
   */
  registerLoader(runtimeName, loaderPtr) {
    const encoded = _encoder.encode(runtimeName);
    const namePtr = Deno.UnsafePointer.of(encoded);

    // Build the StringView { ptr, len } as a 16-byte struct passed by value.
    const nameView = new Uint8Array(16);
    const nameDv = new DataView(nameView.buffer);
    nameDv.setBigUint64(0, BigInt(Deno.UnsafePointer.value(namePtr)), true);
    nameDv.setBigUint64(8, BigInt(encoded.length), true);

    const result = callHostMethod(
      this.#host,
      HOST_INTERFACE_OFFSETS.register_loader,
      ["pointer", { struct: ["pointer", "usize"] }, "pointer"],
      { struct: ["u32", "u32", "pointer", "usize"] },
      [this.#host, nameView, loaderPtr]
    );

    // AbiError struct returned by value; code is the first u32 field.
    const code = new DataView(result.buffer, result.byteOffset, result.byteLength).getUint32(0, true);
    if (code !== 0) {
      throw new Error(`registerLoader(${runtimeName}) failed: ${this.lastError()}`);
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
    host = lib.symbols.polyplug_runtime_create(configPtr);
  } else {
    host = lib.symbols.polyplug_runtime_create(null);
  }

  if (host === null) {
    throw new Error("polyplug_runtime_create failed: unable to create runtime (returned null HostInterface)");
  }
  return new Runtime(lib, host);
}
