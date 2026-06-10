/**
 * @file polyplug.js
 * @description Host library for polyplug JavaScript/TypeScript hosts.
 *
 * Updated for HostApi-based API (18-04 refactor).
 * All operations are accessed through HostApi struct fields,
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
  HOST_API_RUNTIME_OFFSET,
  HOST_API_REGISTER_GUEST_CONTRACT_OFFSET,
  HOST_API_ALLOC_OFFSET,
  HOST_API_FREE_OFFSET,
  HOST_API_FIND_GUEST_CONTRACT_OFFSET,
  HOST_API_FIND_ALL_GUEST_CONTRACTS_OFFSET,
  HOST_API_RESOLVE_GUEST_CONTRACT_OFFSET,
  HOST_API_GET_HOST_CONTRACT_OFFSET,
  HOST_API_RESOLVE_HOST_CONTRACT_INTERFACE_OFFSET,
  HOST_API_LIST_BUNDLES_OFFSET,
  HOST_API_GET_DEPENDENCIES_OFFSET,
  HOST_API_LOAD_BUNDLE_OFFSET,
  HOST_API_RELOAD_BUNDLE_OFFSET,
  HOST_API_UNLOAD_BUNDLE_OFFSET,
  HOST_API_REGISTER_HOST_CONTRACT_OFFSET,
  HOST_API_REGISTER_LOADER_OFFSET,
  HOST_API_GET_LAST_ERROR_OFFSET,
  HOST_API_GET_ERROR_LEN_OFFSET,
  RUNTIME_CONFIG_COMPATIBILITY_OFFSET,
  RUNTIME_CONFIG_UNLOAD_MODE_OFFSET,
  RUNTIME_CONFIG_HOT_RELOAD_ENABLED_OFFSET,
  RUNTIME_CONFIG_ON_RELOAD_OFFSET,
  RUNTIME_CONFIG_ON_RELOAD_USER_DATA_OFFSET,
  RUNTIME_CONFIG_LOG_OFFSET,
  RUNTIME_CONFIG_LOG_USER_DATA_OFFSET,
  RUNTIME_CONFIG_LOG_MAX_LEVEL_OFFSET,
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

// Import the GuestContractHandle layout constants so the by-value struct passing
// and the array element stride stay locked to the generated ABI definition.
import {
  GUEST_CONTRACT_HANDLE_INDEX_OFFSET,
  GUEST_CONTRACT_HANDLE_GENERATION_OFFSET,
  GUEST_CONTRACT_HANDLE_SIZE,
} from "../../abi/abi.ts";

// DispatchType discriminants (match polyplug_abi::DispatchType #[repr(u32)]).
const DISPATCH_TYPE_NATIVE = 0;
const DISPATCH_TYPE_VIRTUAL_MACHINE = 1;

// AbiErrorCode values (match polyplug_abi::AbiErrorCode #[repr(u32)]). The ABI
// definition in abi.ts is a TypeScript `const enum`, which is erased at compile
// time and therefore not importable as a runtime value into this `.js` module;
// the values are mirrored here so generated/host code can use the named form
// (AbiErrorCode.Ok) instead of magic numbers.
const AbiErrorCode = Object.freeze({
  Ok: 0,
  InvalidPointer: 8,
});

// AbiError is returned by value from dispatch as a 24-byte struct
// { code: u32, _pad: u32, message: StringView{ ptr, len } }; code is the first u32.
const ABI_ERROR_STRUCT = { struct: ["u32", "u32", "pointer", "usize"] };
// GuestContractInstance crosses the ABI by value as { data: ptr, contract_id: u64 }.
const GUEST_CONTRACT_INSTANCE_STRUCT = { struct: ["pointer", "u64"] };
// GuestContractHandle crosses the ABI by value as { index: u32, generation: u32 }
// (8 bytes, align 4). Deno FFI passes it as a two-field u32 struct.
const GUEST_CONTRACT_HANDLE_STRUCT = { struct: ["u32", "u32"] };

export {
  getPlatformIdentifier,
  getNativeLibraryFilename,
  loadNativeLibrary,
  openNativeLibrary
} from "./native-loader.ts";

/**
 * The `index` value of a null/invalid GuestContractHandle.
 *
 * GuestContractHandle is `#[repr(C)] { index: u32, generation: u32 }` (8 bytes,
 * align 4). The null handle has `index == u32::MAX`; the generation is irrelevant
 * for the null sentinel. Null checks test the `index` field only.
 * @type {number}
 */
const NULL_HANDLE_INDEX = 0xFFFFFFFF;

/**
 * Null/invalid GuestContractHandle sentinel.
 *
 * GuestContractHandle is `#[repr(C)] { index: u32, generation: u32 }` (8 bytes,
 * align 4) and crosses the C ABI by value as a two-field u32 struct. The host
 * binding represents a resolved handle as `{ index, generation }`; the null
 * sentinel is `{ index: u32::MAX, generation: 0 }`.
 * @type {{ index: number, generation: number }}
 */
export const NULL_HANDLE = Object.freeze({ index: NULL_HANDLE_INDEX, generation: 0 });

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
// All operations are accessed through HostApi struct fields.
const SYMBOLS = {
  polyplug_runtime_create: { parameters: ["pointer"], result: "pointer" },
  polyplug_runtime_destroy: { parameters: ["pointer"], result: "void" },
};

// HostApi struct offsets imported from auto-generated abi.ts (160 bytes, 19 function pointer fields)
const HOST_API_OFFSETS = {
  runtime: HOST_API_RUNTIME_OFFSET,
  register_guest_contract: HOST_API_REGISTER_GUEST_CONTRACT_OFFSET,
  alloc: HOST_API_ALLOC_OFFSET,
  free: HOST_API_FREE_OFFSET,
  find_guest_contract: HOST_API_FIND_GUEST_CONTRACT_OFFSET,
  find_all_guest_contracts: HOST_API_FIND_ALL_GUEST_CONTRACTS_OFFSET,
  resolve_guest_contract: HOST_API_RESOLVE_GUEST_CONTRACT_OFFSET,
  get_host_contract: HOST_API_GET_HOST_CONTRACT_OFFSET,
  resolve_host_contract_interface: HOST_API_RESOLVE_HOST_CONTRACT_INTERFACE_OFFSET,
  list_bundles: HOST_API_LIST_BUNDLES_OFFSET,
  get_dependencies: HOST_API_GET_DEPENDENCIES_OFFSET,
  load_bundle: HOST_API_LOAD_BUNDLE_OFFSET,
  reload_bundle: HOST_API_RELOAD_BUNDLE_OFFSET,
  unload_bundle: HOST_API_UNLOAD_BUNDLE_OFFSET,
  register_host_contract: HOST_API_REGISTER_HOST_CONTRACT_OFFSET,
  register_loader: HOST_API_REGISTER_LOADER_OFFSET,
  get_last_error: HOST_API_GET_LAST_ERROR_OFFSET,
  get_error_len: HOST_API_GET_ERROR_LEN_OFFSET,
};

// Module-level caches for hot path performance (stateless encode/decode
// helpers only — runtime/plugin state never lives at module level, Rule 12).
const _funcCache = new Map();
const _encoder = new TextEncoder();
const _decoder = new TextDecoder();

// Callback type for reload notifications.
//
// The ABI signature is `void(*)(void* user_data, ReloadPhase)`: an opaque
// user-data pointer followed by ONE `ReloadPhase` struct (48 bytes) BY VALUE —
// not a list of scalar arguments. ReloadPhase layout (matches
// polyplug_abi::runtime::reload_phase):
//   phase_type: u32 @ 0, padding @ 4, bundle_id: u64 @ 8,
//   bundle_name: StringView{ ptr @ 16, len @ 24 },
//   reason: StringView{ ptr @ 32, len @ 40 }.
// There is NO retry_count field. The padding u32 is declared explicitly so the
// struct layout lines up at the by-value ABI boundary.
const _RELOAD_CALLBACK_TYPE = {
    parameters: ["pointer", { struct: ["u32", "u32", "u64", "pointer", "usize", "pointer", "usize"] }],
    result: "void"
};

// Callback type for the runtime logger.
//
// The ABI signature is `void(*)(void* log_user_data, u32 level, StringView scope,
// StringView message)`. Each StringView crosses BY VALUE as a 16-byte
// { ptr, len } struct.
const _LOG_CALLBACK_TYPE = {
    parameters: [
        "pointer",
        "u32",
        { struct: ["pointer", "usize"] },
        { struct: ["pointer", "usize"] },
    ],
    result: "void"
};

/**
 * Decode a by-value StringView struct buffer ({ ptr @ 0, len @ 8 }) into a string.
 * @param {Uint8Array} svStruct - 16-byte StringView struct buffer.
 * @returns {string}
 */
function decodeStringViewStruct(svStruct) {
  const dv = new DataView(svStruct.buffer, svStruct.byteOffset, svStruct.byteLength);
  const ptr = Deno.UnsafePointer.create(dv.getBigUint64(0, true));
  const len = Number(dv.getBigUint64(8, true));
  if (ptr === null || len === 0) {
    return "";
  }
  return new Deno.UnsafePointerView(ptr).getUtf8String(len);
}

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
 * Read a function pointer from HostApi at given offset.
 * The raw 64-bit value is wrapped into a Deno pointer object so it can be
 * passed to Deno.UnsafeFnPointer (which rejects bare BigInts).
 * @param {Deno.PointerValue} hostPtr - HostApi pointer
 * @param {number} offset - Byte offset in struct
 * @returns {Deno.PointerValue} Function pointer
 */
function readHostField(hostPtr, offset) {
  const view = new Deno.UnsafePointerView(hostPtr);
  return Deno.UnsafePointer.create(view.getBigUint64(offset));
}

/**
 * Call a HostApi method with self-passing pattern.
 * @param {Deno.PointerValue} hostPtr - HostApi pointer
 * @param {number} fieldOffset - Offset of the function pointer field
 * @param {Array} paramTypes - FFI parameter types
 * @param {string} resultType - FFI result type
 * @param {Array} args - Arguments to pass (first arg is always hostPtr)
 * @returns {*} Result from FFI call
 */
function callHostMethod(hostPtr, fieldOffset, paramTypes, resultType, args) {
  const funcPtr = readHostField(hostPtr, fieldOffset);
  if (funcPtr === null) {
    throw new Error(`HostApi field at offset ${fieldOffset} is null`);
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
 * Dispatch is routed directly through the resolved interface's dispatch union
 * (native function table or VM call), so this view works identically for native
 * guests and VM (QuickJS/Lua/Python) guests.
 *
 * Validity keys off the interface pointer, never off instance data: a null
 * `instance.data` is a VALID dispatch token because the runtime substitutes
 * stateless stubs for null lifecycle pointers and stateless contracts return a
 * null instance handle.
 */
export class GuestContractInterfaceView {
  #host;          // HostApi pointer
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
   * @param {Deno.PointerValue} host - HostApi pointer
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
    // create_instance(host: *const HostApi, args: *const ()) -> GuestContractInstance
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
    // destroy_instance(host: *const HostApi, instance: GuestContractInstance)
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
   * Direct interface dispatch is the supported mechanism and works for both native
   * and VM (QuickJS/Lua/Python) guests, including stateless ones whose instance
   * carries a null `data`.
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
        return AbiErrorCode.InvalidPointer; // null VM dispatch function.
      }
      // call(loader_data: VmLoaderData, instance, fn_id: u32, args, out, arena) -> AbiError.
      // VmLoaderData is a single opaque pointer (`{ data: *mut c_void }`). The trailing
      // `arena` is a `*mut CallArena`; a null arena is the documented legacy fallback to
      // per-value host->alloc (host callers carry no per-call arena).
      const fn = new Deno.UnsafeFnPointer(this.#vmCallPtr, {
        parameters: ["pointer", GUEST_CONTRACT_INSTANCE_STRUCT, "u32", "pointer", "pointer", "pointer"],
        result: ABI_ERROR_STRUCT,
      });
      const loaderData = Deno.UnsafePointer.create(this.#vmLoaderData);
      result = fn.call(loaderData, instance, slot, argsPtr, outPtr, null);
    } else {
      const fn = this.#nativeFnPointer(slot);
      if (fn === null) {
        return AbiErrorCode.InvalidPointer; // null native function slot.
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
 * Runtime class using HostApi-based API.
 * All operations call through HostApi struct fields.
 */
export class Runtime {
  #lib;
  #host;  // HostApi pointer
  // Per-instance Deno.UnsafeCallback handles (on_reload / log trampolines).
  // Owned by THIS runtime (Rule 12: no module globals); closed on destroy.
  #callbacks;
  #destroyed;

  /**
   * @param {Deno.DynamicLibrary} lib - Dynamic library instance
   * @param {Deno.PointerValue} host - HostApi pointer
   * @param {Deno.UnsafeCallback[]} [callbacks=[]] - Owned FFI callbacks to close on destroy
   */
  constructor(lib, host, callbacks = []) {
    this.#lib = lib;
    this.#host = host;
    this.#callbacks = callbacks;
    this.#destroyed = false;
  }

  /**
   * Destroy the native runtime, then close the owned FFI callbacks.
   * The native side may invoke the trampolines up until
   * polyplug_runtime_destroy returns, so close() happens strictly after.
   * Idempotent.
   */
  destroy() {
    if (this.#destroyed) {
      return;
    }
    this.#destroyed = true;
    this.#lib.symbols.polyplug_runtime_destroy(this.#host);
    for (const cb of this.#callbacks) {
      cb.close();
    }
    this.#callbacks = [];
  }

  [Symbol.dispose]() {
    this.destroy();
  }

  /**
   * Get HostApi pointer.
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
   * Calls through HostApi.get_last_error and get_error_len fields.
   * @returns {string}
   */
  lastError() {
    // Get error length via get_error_len
    const len = Number(callHostMethod(
      this.#host,
      HOST_API_OFFSETS.get_error_len,
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
      HOST_API_OFFSETS.get_last_error,
      ["pointer", "pointer", "usize"],
      "usize",
      [this.#host, bufPtr, BigInt(len)]
    );

    return _decoder.decode(buf);
  }

  /**
   * Load a plugin bundle.
   * Calls through HostApi.load_bundle field.
   * @param {string} path - Path to bundle directory
   */
  loadBundle(path) {
    const encoded = _encoder.encode(path);
    const ptr = Deno.UnsafePointer.of(encoded);
    // HostApi.load_bundle returns AbiError (24-byte struct), not u32.
    const result = callHostMethod(
      this.#host,
      HOST_API_OFFSETS.load_bundle,
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
   * Calls through HostApi.reload_bundle field.
   * @param {string} path - Path to bundle directory
   */
  reloadBundle(path) {
    const encoded = _encoder.encode(path);
    const ptr = Deno.UnsafePointer.of(encoded);
    // HostApi.reload_bundle returns AbiError (24-byte struct), not u32.
    const result = callHostMethod(
      this.#host,
      HOST_API_OFFSETS.reload_bundle,
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
   * Unload a plugin bundle by bundle ID.
   * Calls through HostApi.unload_bundle field.
   * @param {bigint} bundleId - Bundle identifier
   */
  unloadBundle(bundleId) {
    // HostApi.unload_bundle returns AbiError (24-byte struct), not u32.
    const result = callHostMethod(
      this.#host,
      HOST_API_OFFSETS.unload_bundle,
      ["pointer", "u64"],
      { struct: ["u32", "u32", "pointer", "usize"] },
      [this.#host, bundleId]
    );
    const code = new DataView(result.buffer, result.byteOffset, result.byteLength).getUint32(0, true);
    if (code !== 0) {
      throw new Error(`unloadBundle failed: ${this.lastError()}`);
    }
  }

  /**
   * Find guest contract by contract ID.
   * Calls through HostApi.find_guest_contract field.
   *
   * Returns a GuestContractHandle, which is `#[repr(C)] { index: u32,
   * generation: u32 }` (8 bytes, align 4) and crosses the C ABI by value as a
   * two-field u32 struct. The result is a `{ index, generation }` object;
   * a null result (index == u32::MAX) signals "no matching contract".
   * @param {bigint} contractId - Contract identifier
   * @param {number} [minVersion=0] - Minimum version
   * @returns {{ index: number, generation: number }} Guest contract handle
   */
  findGuestContract(contractId, minVersion = 0) {
    // find_guest_contract returns GuestContractHandle by value as an 8-byte
    // struct. Deno FFI returns by-value structs as a Uint8Array buffer.
    const result = callHostMethod(
      this.#host,
      HOST_API_OFFSETS.find_guest_contract,
      ["pointer", "u64", "u32"],
      GUEST_CONTRACT_HANDLE_STRUCT,
      [this.#host, contractId, minVersion]
    );
    const dv = new DataView(result.buffer, result.byteOffset, result.byteLength);
    const index = dv.getUint32(GUEST_CONTRACT_HANDLE_INDEX_OFFSET, true);
    // A not-found result is the null sentinel (index == u32::MAX). Return the
    // canonical frozen NULL_HANDLE so callers can compare against it by identity
    // (`handle === NULL_HANDLE`) as well as by value.
    if (index === NULL_HANDLE_INDEX) {
      return NULL_HANDLE;
    }
    return {
      index,
      generation: dv.getUint32(GUEST_CONTRACT_HANDLE_GENERATION_OFFSET, true),
    };
  }

  /**
   * Find all guest contracts by contract ID.
   * Calls through HostApi.find_all_guest_contracts field.
   * @param {bigint} contractId - Contract identifier
   * @param {number} [minVersion=0] - Minimum version
   * @param {number} [cap=64] - Buffer capacity
   * @returns {{ index: number, generation: number }[]} Array of guest contract handles
   */
  findAllGuestContracts(contractId, minVersion = 0, cap = 64) {
    // find_all_guest_contracts returns the ABI `Array` struct BY VALUE:
    // #[repr(C)] { items: pointer @ 0, len: usize @ 8, align: usize @ 16 } =
    // 24 bytes. Declaring anything smaller makes the SysV sret write past the
    // buffer Deno allocates for the return value (memory corruption). Deno FFI
    // returns by-value structs as a Uint8Array buffer (mirrors the
    // AbiError-by-value pattern in loadBundle above).
    const result = callHostMethod(
      this.#host,
      HOST_API_OFFSETS.find_all_guest_contracts,
      ["pointer", "u64", "u32"],
      { struct: ["pointer", "usize", "usize"] },
      [this.#host, contractId, minVersion]
    );

    // Read the returned Array struct: { items @ 0, len @ 8, align @ 16 }.
    const resultDv = new DataView(result.buffer, result.byteOffset, result.byteLength);
    const arrPtrRaw = resultDv.getBigUint64(0, true);
    const arrLen = Number(resultDv.getBigUint64(8, true));
    const arrAlign = resultDv.getBigUint64(16, true);
    const arrPtr = Deno.UnsafePointer.create(arrPtrRaw);

    if (arrPtr === null || arrLen === 0) {
      return [];
    }

    // Read handles from array. GuestContractHandle is
    // `#[repr(C)] { index: u32, generation: u32 }` (8 bytes, align 4), so
    // elements have an 8-byte stride; each is read as a { index, generation }.
    const handles = [];
    const arrView = new Deno.UnsafePointerView(arrPtr);
    for (let i = 0; i < Math.min(arrLen, cap); i++) {
      const base = i * GUEST_CONTRACT_HANDLE_SIZE;
      handles.push({
        index: arrView.getUint32(base + GUEST_CONTRACT_HANDLE_INDEX_OFFSET),
        generation: arrView.getUint32(base + GUEST_CONTRACT_HANDLE_GENERATION_OFFSET),
      });
    }

    // Free the array via HostApi.free using the runtime's allocation size and
    // alignment: size = len * sizeof(GuestContractHandle), align = Array.align
    // as returned by the runtime.
    if (arrLen > 0) {
      callHostMethod(
        this.#host,
        HOST_API_OFFSETS.free,
        ["pointer", "pointer", "usize", "usize"],
        "void",
        [this.#host, arrPtr, BigInt(arrLen * GUEST_CONTRACT_HANDLE_SIZE), arrAlign]
      );
    }

    return handles;
  }

  /**
   * Resolve a guest contract handle to a raw interface pointer.
   * Calls through HostApi.resolve_guest_contract field.
   *
   * The handle is a GuestContractHandle (`#[repr(C)] { index: u32,
   * generation: u32 }`, 8 bytes) passed by value as a two-field u32 struct.
   * The null check tests the `index` field.
   * @param {{ index: number, generation: number }} handle - Guest contract handle
   * @returns {Deno.PointerValue} Resolved interface pointer (null if invalid/stale)
   */
  resolveGuestContract(handle) {
    if (handle === null || handle.index === NULL_HANDLE_INDEX) {
      return null;
    }
    // Build the 8-byte GuestContractHandle { index, generation } and pass it by
    // value, matching the C ABI struct layout.
    const handleBuf = new Uint8Array(GUEST_CONTRACT_HANDLE_SIZE);
    const handleDv = new DataView(handleBuf.buffer);
    handleDv.setUint32(GUEST_CONTRACT_HANDLE_INDEX_OFFSET, handle.index, true);
    handleDv.setUint32(GUEST_CONTRACT_HANDLE_GENERATION_OFFSET, handle.generation, true);
    return callHostMethod(
      this.#host,
      HOST_API_OFFSETS.resolve_guest_contract,
      ["pointer", GUEST_CONTRACT_HANDLE_STRUCT],
      "pointer",
      [this.#host, handleBuf]
    );
  }

  /**
   * Resolve a guest contract handle to a decoded interface view.
   *
   * Wraps `resolveGuestContract` (raw pointer) in a {@link GuestContractInterfaceView}
   * that decodes the `#[repr(C)] GuestContractInterface` fields and exposes the
   * lifecycle function pointers, dispatch type, function count, and a per-slot
   * dispatch entry. Returns null when the handle does not resolve.
   * @param {{ index: number, generation: number }} handle - Guest contract handle
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
   * Allocate `size` bytes via the host allocator (HostApi.alloc).
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
      HOST_API_OFFSETS.alloc,
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
      HOST_API_OFFSETS.free,
      ["pointer", "pointer", "usize", "usize"],
      "void",
      [this.#host, ptr, BigInt(size), BigInt(align)]
    );
  }

  /**
   * Register a host contract interface with the runtime.
   * Calls through HostApi.register_host_contract field.
   * @param {Deno.PointerValue} hostInterface - Pointer to HostContractInterface struct
   */
  registerHostContract(hostInterface) {
    // HostApi.register_host_contract returns AbiError (24-byte struct), not u32.
    const result = callHostMethod(
      this.#host,
      HOST_API_OFFSETS.register_host_contract,
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
   * Calls through HostApi.register_loader field. The StringView runtime
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
      HOST_API_OFFSETS.register_loader,
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
 * Uses HostApi-based API: polyplug_runtime_create returns HostApi*.
 *
 * All configuration is per-instance (Rule 12: no module globals shared across
 * runtimes). The FFI callbacks created for `onReload` / `logger` are owned by
 * the returned Runtime and closed by {@link Runtime#destroy}.
 *
 * RuntimeConfig is the full 56-byte ABI struct: compatibility (u32 @ 0),
 * unload_mode (u32 @ 4), hot_reload_enabled (bool @ 8), on_reload (fn @ 16),
 * on_reload_user_data (ptr @ 24), log (fn @ 32), log_user_data (ptr @ 40),
 * log_max_level (u32 @ 48). Offsets/size come from the abi.ts constants.
 *
 * @param {Deno.DynamicLibrary} lib - Dynamic library
 * @param {Object} [options] - Per-runtime options
 * @param {Object} [options.config] - RuntimeConfig fields
 * @param {number} [options.config.compatibility=0] - Compatibility mode (COMPATIBILITY_STRICT=0, RELAXED=1, YOLO=2)
 * @param {number} [options.config.unloadMode=0] - Unload mode discriminant (0 = Retire)
 * @param {boolean} [options.config.hotReloadEnabled=false] - Whether hot-reload is enabled
 * @param {number} [options.config.logMaxLevel=5] - Max LogLevel (1=Error … 5=Trace) delivered to `logger`
 * @param {function(ReloadPhase): void} [options.onReload] - Hot-reload phase callback
 * @param {function(number, string, string): void} [options.logger] - Logger callback (level, scope, message)
 * @returns {Runtime}
 */
export function runtimeNew(lib, options = {}) {
  const config = options.config ?? null;
  const onReloadCallback = options.onReload ?? null;
  const loggerCallback = options.logger ?? null;

  /** @type {Deno.UnsafeCallback[]} */
  const ownedCallbacks = [];
  let host;

  if (config || onReloadCallback || loggerCallback) {
    const configBuf = new Uint8Array(RUNTIME_CONFIG_SIZE);
    const configView = new DataView(configBuf.buffer);

    configView.setUint32(RUNTIME_CONFIG_COMPATIBILITY_OFFSET, config?.compatibility ?? COMPATIBILITY_STRICT, true);
    configView.setUint32(RUNTIME_CONFIG_UNLOAD_MODE_OFFSET, config?.unloadMode ?? 0, true);
    configView.setUint8(RUNTIME_CONFIG_HOT_RELOAD_ENABLED_OFFSET, config?.hotReloadEnabled ? 1 : 0);

    if (onReloadCallback) {
      const ffiReloadCallback = new Deno.UnsafeCallback(_RELOAD_CALLBACK_TYPE,
        (_userData, phaseStruct) => {
          // _userData is the opaque on_reload_user_data pointer (unused here — the
          // JS closure already captures the callback). phaseStruct is the 48-byte
          // ReloadPhase passed by value as a buffer. A JS exception must never
          // unwind across the C ABI mid-reload: catch-all, log to stderr.
          try {
            const dv = new DataView(phaseStruct.buffer, phaseStruct.byteOffset, phaseStruct.byteLength);
            const phaseType = dv.getUint32(0, true);
            const bundleId = dv.getBigUint64(8, true);
            const bundleNamePtrRaw = dv.getBigUint64(16, true);
            const bundleNameLen = Number(dv.getBigUint64(24, true));
            const reasonPtrRaw = dv.getBigUint64(32, true);
            const reasonLen = Number(dv.getBigUint64(40, true));

            let bundleName = "";
            const bundleNamePtr = Deno.UnsafePointer.create(bundleNamePtrRaw);
            if (bundleNamePtr !== null && bundleNameLen > 0) {
              bundleName = new Deno.UnsafePointerView(bundleNamePtr).getUtf8String(bundleNameLen);
            }
            let reason = "";
            const reasonPtr = Deno.UnsafePointer.create(reasonPtrRaw);
            if (reasonPtr !== null && reasonLen > 0) {
              reason = new Deno.UnsafePointerView(reasonPtr).getUtf8String(reasonLen);
            }
            onReloadCallback(new ReloadPhase(phaseType, bundleId, bundleName, reason));
          } catch (e) {
            console.error(`polyplug: reload callback threw: ${e}`);
          }
        }
      );
      ownedCallbacks.push(ffiReloadCallback);
      // on_reload_user_data is left null: the JS closure already captures the callback.
      configView.setBigUint64(RUNTIME_CONFIG_ON_RELOAD_OFFSET, BigInt(Deno.UnsafePointer.value(ffiReloadCallback.pointer)), true);
    }

    if (loggerCallback) {
      const ffiLogCallback = new Deno.UnsafeCallback(_LOG_CALLBACK_TYPE,
        (_userData, level, scopeStruct, messageStruct) => {
          // A JS exception must never unwind across the C ABI from inside the
          // runtime's logging funnel: catch-all, report to stderr.
          try {
            loggerCallback(level, decodeStringViewStruct(scopeStruct), decodeStringViewStruct(messageStruct));
          } catch (e) {
            console.error(`polyplug: logger callback threw: ${e}`);
          }
        }
      );
      ownedCallbacks.push(ffiLogCallback);
      // log_user_data is left null: the JS closure already captures the callback.
      configView.setBigUint64(RUNTIME_CONFIG_LOG_OFFSET, BigInt(Deno.UnsafePointer.value(ffiLogCallback.pointer)), true);
      // Default to Trace (5): deliver everything, filter inside the JS callback.
      configView.setUint32(RUNTIME_CONFIG_LOG_MAX_LEVEL_OFFSET, config?.logMaxLevel ?? 5, true);
    } else if (config?.logMaxLevel !== undefined) {
      configView.setUint32(RUNTIME_CONFIG_LOG_MAX_LEVEL_OFFSET, config.logMaxLevel, true);
    }

    const configPtr = Deno.UnsafePointer.of(configBuf);
    host = lib.symbols.polyplug_runtime_create(configPtr);
  } else {
    host = lib.symbols.polyplug_runtime_create(null);
  }

  if (host === null) {
    for (const cb of ownedCallbacks) {
      cb.close();
    }
    throw new Error("polyplug_runtime_create failed: unable to create runtime (returned null HostApi)");
  }
  return new Runtime(lib, host, ownedCallbacks);
}
