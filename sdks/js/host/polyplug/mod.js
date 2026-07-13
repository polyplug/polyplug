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

import { ReloadPhase } from "./reload_phase.js";

// All native FFI crosses through the backend seam — no direct runtime FFI globals
// in this file. getBackend() resolves the backend for the current runtime and throws a
// clear error on an unsupported one (replacing the former inline typeof-Deno guard).
import { getBackend } from "@polyplug/abi";
/**
 * @typedef {import("@polyplug/abi").PolyPtr} PolyPtr
 * @typedef {import("@polyplug/abi").FfiLibrary} FfiLibrary
 * @typedef {import("@polyplug/abi").FfiCallback} FfiCallback
 * @typedef {import("@polyplug/abi").FfiFunction} FfiFunction
 */

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
  HOST_API_REGISTRY_REVISION_OFFSET,
  RUNTIME_CONFIG_COMPATIBILITY_OFFSET,
  RUNTIME_CONFIG_HOT_RELOAD_ENABLED_OFFSET,
  RUNTIME_CONFIG_ON_RELOAD_OFFSET,
  RUNTIME_CONFIG_ON_RELOAD_USER_DATA_OFFSET,
  RUNTIME_CONFIG_LOG_OFFSET,
  RUNTIME_CONFIG_LOG_USER_DATA_OFFSET,
  RUNTIME_CONFIG_LOG_MAX_LEVEL_OFFSET,
  RUNTIME_CONFIG_SIGNATURE_POLICY_OFFSET,
  RUNTIME_CONFIG_TRUSTED_KEYS_OFFSET,
  RUNTIME_CONFIG_TRUSTED_KEYS_LEN_OFFSET,
  RUNTIME_CONFIG_TRUSTED_KEYS_ALIGN_OFFSET,
  RUNTIME_CONFIG_SIZE,
  ED25519_PUBLIC_KEY_SIZE,
  RELOAD_PHASE_PHASE_TYPE_OFFSET,
  RELOAD_PHASE_BUNDLE_ID_OFFSET,
  RELOAD_PHASE_BUNDLE_NAME_OFFSET,
  RELOAD_PHASE_REASON_OFFSET,
  GUEST_CONTRACT_INTERFACE_DISPATCH_TYPE_OFFSET,
  GUEST_CONTRACT_INTERFACE_ADAPTER_CONTEXT_OFFSET,
  GUEST_CONTRACT_INTERFACE_CREATE_INSTANCE_OFFSET,
  GUEST_CONTRACT_INTERFACE_DESTROY_INSTANCE_OFFSET,
  GUEST_CONTRACT_INTERFACE_DISPATCH_OFFSET,
  GUEST_CONTRACT_INSTANCE_SIZE,
  NATIVE_DISPATCH_FUNCTION_COUNT_OFFSET,
  NATIVE_DISPATCH_FUNCTIONS_OFFSET,
  VM_DISPATCH_CALL_OFFSET,
  VM_DISPATCH_LOADER_DATA_OFFSET,
  HOST_CONTRACT_INTERFACE_CONTRACT_ID_OFFSET,
  HOST_CONTRACT_INTERFACE_CONTRACT_VERSION_OFFSET,
  HOST_CONTRACT_INTERFACE_SINGLETON_OFFSET,
  HOST_CONTRACT_INTERFACE_DISPATCH_TYPE_OFFSET,
  HOST_CONTRACT_INTERFACE_USER_DATA_OFFSET,
  HOST_CONTRACT_INTERFACE_CREATE_INSTANCE_OFFSET,
  HOST_CONTRACT_INTERFACE_DESTROY_INSTANCE_OFFSET,
  HOST_CONTRACT_INTERFACE_DISPATCH_OFFSET,
  HOST_CONTRACT_INTERFACE_SIZE,
  PLUGIN_DESCRIPTOR_CONTRACT_NAME_OFFSET,
  PLUGIN_DESCRIPTOR_NAME_OFFSET,
  PLUGIN_DESCRIPTOR_SIZE,
  PLUGIN_DESCRIPTOR_VERSION_OFFSET,
  STRING_VIEW_LEN_OFFSET,
  STRING_VIEW_PTR_OFFSET,
  VERSION_MAJOR_OFFSET,
  VERSION_MINOR_OFFSET,
  VERSION_PATCH_OFFSET,
} from "@polyplug/abi";

import {
  GUEST_CONTRACT_INTERFACE_CONTRACT_ID_OFFSET,
  GUEST_CONTRACT_INTERFACE_CONTRACT_VERSION_OFFSET,
  GUEST_CONTRACT_INTERFACE_SIZE,
} from "@polyplug/abi";

// Import the GuestContractHandle layout constants so the by-value struct passing
// and the array element stride stay locked to the generated ABI definition.
import {
  GUEST_CONTRACT_HANDLE_INDEX_OFFSET,
  GUEST_CONTRACT_HANDLE_GENERATION_OFFSET,
  GUEST_CONTRACT_HANDLE_SIZE,
} from "@polyplug/abi";

// Re-export the contract-ID helpers from the auto-generated abi.ts (their single
// definition, per checks/sdk_validator.yaml method_targets) so host code that imports
// from this module keeps a stable surface without a duplicate implementation.
export {
  fnv1a64,
  guestContractId,
  hostContractId,
  bundleId,
} from "@polyplug/abi";

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
  Panic: 3,
  FunctionNotAvailable: 6,
  InvalidPointer: 8,
});

// Out-param ABI: AbiError is written through a trailing `*mut AbiError` pointer.
// It is a 24-byte struct { code: u32, _pad: u32, message: StringView{ ptr, len } };
// the caller allocates a zeroed buffer of this size, passes its pointer, and
// reads `code` (the first u32) back after the call.
const ABI_ERROR_SIZE = 24;
// GuestContractInstance crosses the ABI by value as { data: ptr, contract_id: u64 }.
const GUEST_CONTRACT_INSTANCE_STRUCT = { struct: ["pointer", "u64"] };
// HostContractInstance crosses the ABI by value as a single { data: ptr } field
// (8 bytes). The host-contract provider stores a non-zero per-instance id in
// `data`; id 0 denotes the per-contract default (stateless) instance.
const HOST_CONTRACT_INSTANCE_STRUCT = { struct: ["pointer"] };
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

// SignaturePolicy values (match polyplug_abi::SignaturePolicy #[repr(u32)]). The
// ABI definition in abi.ts is a TypeScript `const enum`, erased at compile time
// and therefore not importable as a runtime value into this `.js` module; the
// values are mirrored here so host code can use the named form
// (SignaturePolicy.Off) instead of magic numbers when setting config.signaturePolicy.
export const SignaturePolicy = Object.freeze({
  Off: 0,
  WarnOnly: 1,
  Required: 2,
});

// ─── FFI Symbols: Only create and destroy ───────────────────────────────────────
// All operations are accessed through HostApi struct fields.
const SYMBOLS = {
  polyplug_runtime_create: { parameters: ["pointer"], result: "pointer" },
  polyplug_runtime_destroy: { parameters: ["pointer"], result: "bool" },
  polyplug_begin_internal_plugin: {
    parameters: ["pointer", "pointer", "usize", "u32", "pointer", "pointer"],
    result: "void",
  },
  polyplug_commit_internal_plugin: {
    parameters: ["pointer", "u64", "pointer"],
    result: "void",
  },
  polyplug_commit_internal_plugin_with_handles: {
    parameters: ["pointer", "u64", "pointer", "usize", "pointer", "pointer"],
    result: "void",
  },
  polyplug_abort_internal_plugin: {
    parameters: ["pointer", "u64"],
    result: "void",
  },
};

// HostApi struct offsets imported from auto-generated abi.ts.
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
  registry_revision: HOST_API_REGISTRY_REVISION_OFFSET,
};

// Module-level caches for hot path performance (stateless encode/decode
// helpers only — runtime/plugin state never lives at module level, Rule 12).
const _funcCache = new Map();
const _encoder = new TextEncoder();
const _decoder = new TextDecoder();
// The FFI backend seam (stateless adapter, no runtime/plugin state — Rule 12).
const _ffi = getBackend();

// Callback type for reload notifications.
//
// The ABI signature is `void(*)(void* user_data, const ReloadPhase* phase)`:
// an opaque user-data pointer followed by a CONST POINTER to the 48-byte
// `ReloadPhase` struct. The runtime always passes a non-null pointer; the
// pointee (and the StringViews inside it) is valid only for the duration of
// the call. Field offsets come from the generated abi.ts constants
// (RELOAD_PHASE_*): phase_type: u32 @ 0, bundle_id: u64 @ 8,
// bundle_name: StringView{ ptr @ +0, len @ +8 } @ 16,
// reason: StringView{ ptr @ +0, len @ +8 } @ 32.
const _RELOAD_CALLBACK_TYPE = {
    parameters: ["pointer", "pointer"],
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
 * Decode `len` UTF-8 bytes at a raw pointer into a string.
 *
 * Deno's UnsafePointerView has no `getUtf8String` — only `getCString` (which
 * needs a NUL terminator the ABI's StringView does not guarantee). Length-
 * bounded UTF-8 must go through `getArrayBuffer` + TextDecoder.
 * @param {PolyPtr} ptr - Non-null pointer to UTF-8 bytes.
 * @param {number} len - Byte count.
 * @returns {string}
 */
function utf8At(ptr, len) {
  const bytes = _ffi.pointerView(ptr).getArrayBuffer(len);
  return new TextDecoder().decode(bytes);
}

/**
 * Decode a by-value StringView struct buffer ({ ptr @ 0, len @ 8 }) into a string.
 * @param {Uint8Array} svStruct - 16-byte StringView struct buffer.
 * @returns {string}
 */
function decodeStringViewStruct(svStruct) {
  const dv = new DataView(svStruct.buffer, svStruct.byteOffset, svStruct.byteLength);
  const ptr = _ffi.pointerCreate(dv.getBigUint64(0, true));
  const len = Number(dv.getBigUint64(8, true));
  if (ptr === null || len === 0) {
    return "";
  }
  return utf8At(ptr, len);
}

/**
 * Write an AbiError `code` (the first u32 of the 24-byte AbiError) through a
 * runtime-owned out-param pointer. `getArrayBuffer` returns a zero-copy writable
 * view into the native AbiError, so the DataView write lands in the runtime's
 * buffer directly (no copy-back needed). A null pointer is ignored.
 */
function writeAbiErrorCode(outErrPtr, code) {
  if (outErrPtr === null) {
    return;
  }
  const ab = _ffi.pointerView(outErrPtr).getArrayBuffer(ABI_ERROR_SIZE);
  new DataView(ab).setUint32(0, code, true);
}

/**
 * Build a real C `HostContractInterface` for a host-provided contract, with
 * native dispatch backed by FFI callbacks and **per-instance** state.
 *
 * Mirrors the canonical per-instance host-provider model (see the Lua provider):
 * `create_instance` builds a fresh implementation from `factory`, mints a
 * non-zero id, and stores it keyed by that id; dispatch resolves the impl by the
 * instance's `data` (id 0 → a per-contract default impl built once here);
 * `destroy_instance` drops it. The `singleton` flag is honoured by the runtime's
 * own caching — `singleton: true` makes the runtime create one instance and share
 * it, `singleton: false` makes it call `create_instance` per caller.
 *
 * All FFI callbacks and backing buffers are returned in `owned` and MUST
 * be kept alive by the caller for as long as the contract is registered (the
 * runtime holds raw pointers into them). No module-level state is used (Rule 12).
 *
 * @param {Object} spec
 * @param {number} spec.contractIdLo - Low 32 bits of the host_contract id.
 * @param {number} spec.contractIdHi - High 32 bits of the host_contract id.
 * @param {number} spec.major - Contract major version.
 * @param {number} spec.minor - Contract minor version.
 * @param {boolean} spec.singleton - Whether the runtime should cache one instance.
 * @param {() => object} spec.factory - Builds a fresh impl object per instance.
 * @param {Array<(impl: object, argsPtr: PolyPtr, outPtr: PolyPtr) => number>} spec.methods
 *        One thunk per contract method (fn_id = array index). Each reads its args,
 *        calls the impl, writes any return through `outPtr`, and returns an
 *        AbiErrorCode (0 = Ok). Throwing is caught and reported as Panic.
 * @returns {{ interfacePtr: PolyPtr, owned: object[] }}
 */
export function buildHostContractInterface(spec) {
  const { contractIdLo, contractIdHi, major, minor, singleton, factory, methods } = spec;

  /** @type {Map<bigint, object>} */
  const instances = new Map();
  let nextId = 1n;
  const defaultImpl = factory();

  /** @type {FfiCallback[]} */
  const callbacks = [];

  // One native dispatch callback per method: (instance, args, out, out_err) -> void.
  const functionPointers = [];
  for (const invoke of methods) {
    const dispatchCb = _ffi.makeCallback(
      { parameters: [HOST_CONTRACT_INSTANCE_STRUCT, "pointer", "pointer", "pointer"], result: "void" },
      (instanceStruct, argsPtr, outPtr, outErrPtr) => {
        let code = AbiErrorCode.Ok;
        try {
          const id = new DataView(instanceStruct.buffer, instanceStruct.byteOffset, 8).getBigUint64(0, true);
          const impl = id === 0n ? defaultImpl : instances.get(id);
          if (impl === undefined) {
            code = AbiErrorCode.FunctionNotAvailable;
          } else {
            const ret = invoke(impl, argsPtr, outPtr);
            code = typeof ret === "number" ? ret : AbiErrorCode.Ok;
          }
        } catch (e) {
          console.error(`polyplug: host contract dispatch threw: ${e}`);
          code = AbiErrorCode.Panic;
        }
        writeAbiErrorCode(outErrPtr, code);
      },
    );
    callbacks.push(dispatchCb);
    functionPointers.push(_ffi.pointerValue(dispatchCb.pointer));
  }

  // create_instance(this, args, out_instance): build a fresh impl, mint a non-zero
  // id, store it, and write the id as the new instance's `data`.
  const createInstanceCb = _ffi.makeCallback(
    { parameters: ["pointer", "pointer", "pointer"], result: "void" },
    (_this, _args, outInstancePtr) => {
      let id = 0n;
      try {
        const impl = factory();
        id = nextId;
        nextId += 1n;
        instances.set(id, impl);
      } catch (e) {
        console.error(`polyplug: host contract create_instance threw: ${e}`);
        id = 0n;
      }
      if (outInstancePtr !== null) {
        const ab = _ffi.pointerView(outInstancePtr).getArrayBuffer(8);
        new DataView(ab).setBigUint64(0, id, true);
      }
    },
  );
  callbacks.push(createInstanceCb);

  // destroy_instance(this, instance): drop the impl keyed by the instance id.
  const destroyInstanceCb = _ffi.makeCallback(
    { parameters: ["pointer", HOST_CONTRACT_INSTANCE_STRUCT], result: "void" },
    (_this, instanceStruct) => {
      try {
        const id = new DataView(instanceStruct.buffer, instanceStruct.byteOffset, 8).getBigUint64(0, true);
        if (id !== 0n) {
          instances.delete(id);
        }
      } catch (e) {
        console.error(`polyplug: host contract destroy_instance threw: ${e}`);
      }
    },
  );
  callbacks.push(destroyInstanceCb);

  // Backing storage: the function-pointer array, then the interface struct. Both
  // are kept alive via `owned`; the runtime holds raw pointers into them.
  const functionsBuf = new Uint8Array(8 * Math.max(1, functionPointers.length));
  const functionsView = new DataView(functionsBuf.buffer);
  for (let i = 0; i < functionPointers.length; i += 1) {
    functionsView.setBigUint64(i * 8, functionPointers[i], true);
  }
  const functionsPtr = _ffi.pointerOf(functionsBuf);

  const ifaceBuf = new Uint8Array(HOST_CONTRACT_INTERFACE_SIZE);
  const ifaceView = new DataView(ifaceBuf.buffer);
  const contractId = BigInt(contractIdLo >>> 0) | (BigInt(contractIdHi >>> 0) << 32n);
  ifaceView.setBigUint64(HOST_CONTRACT_INTERFACE_CONTRACT_ID_OFFSET, contractId, true);
  ifaceView.setUint16(HOST_CONTRACT_INTERFACE_CONTRACT_VERSION_OFFSET, major, true);
  ifaceView.setUint16(HOST_CONTRACT_INTERFACE_CONTRACT_VERSION_OFFSET + 2, minor, true);
  ifaceView.setUint16(HOST_CONTRACT_INTERFACE_CONTRACT_VERSION_OFFSET + 4, 0, true);
  ifaceView.setUint8(HOST_CONTRACT_INTERFACE_SINGLETON_OFFSET, singleton ? 1 : 0);
  ifaceView.setUint32(HOST_CONTRACT_INTERFACE_DISPATCH_TYPE_OFFSET, DISPATCH_TYPE_NATIVE, true);
  // runtime @ offset is left zero — the runtime fills it in during registration.
  // user_data is left null — per-instance impls are carried in `instances`, not user_data.
  ifaceView.setBigUint64(
    HOST_CONTRACT_INTERFACE_USER_DATA_OFFSET,
    0n,
    true,
  );
  ifaceView.setBigUint64(
    HOST_CONTRACT_INTERFACE_CREATE_INSTANCE_OFFSET,
    _ffi.pointerValue(createInstanceCb.pointer),
    true,
  );
  ifaceView.setBigUint64(
    HOST_CONTRACT_INTERFACE_DESTROY_INSTANCE_OFFSET,
    _ffi.pointerValue(destroyInstanceCb.pointer),
    true,
  );
  // dispatch union (native): { function_count: u32, functions: *const *const () }.
  ifaceView.setUint32(
    HOST_CONTRACT_INTERFACE_DISPATCH_OFFSET + NATIVE_DISPATCH_FUNCTION_COUNT_OFFSET,
    functionPointers.length,
    true,
  );
  ifaceView.setBigUint64(
    HOST_CONTRACT_INTERFACE_DISPATCH_OFFSET + NATIVE_DISPATCH_FUNCTIONS_OFFSET,
    _ffi.pointerValue(functionsPtr),
    true,
  );

  const interfacePtr = _ffi.pointerOf(ifaceBuf);
  // `owned` keeps every callback + buffer + the per-instance maps reachable so the
  // GC cannot reclaim memory the runtime still points into.
  const owned = [...callbacks, functionsBuf, ifaceBuf, instances];
  return { interfacePtr, owned };
}

/**
 * Canonical manifest bytes, staged contract registrar, and rooted JavaScript
 * state for one internal plugin. The resident remains with its creator until
 * every descriptor/interface pair has staged and core commits the transaction.
 */
export class InternalPluginBundle {
  #manifest;
  #resident;
  #registerContracts;
  #transferReserved;

  /**
   * @param {string | Uint8Array} manifest Canonical bundle manifest TOML.
   * @param {{ release: () => void }} resident Rooted generated state.
   * @param {(host: PolyPtr) => void} registerContracts Stages descriptor/interface pairs.
   */
  constructor(manifest, resident, registerContracts) {
    if (typeof manifest !== "string" && !(manifest instanceof Uint8Array)) {
      throw new TypeError("InternalPluginBundle manifest must be TOML text or UTF-8 bytes");
    }
    const manifestBytes = typeof manifest === "string" ? _encoder.encode(manifest) : manifest.slice();
    if (manifestBytes.byteLength === 0) {
      throw new TypeError("InternalPluginBundle manifest must not be empty");
    }
    if (resident === null || typeof resident !== "object" || typeof resident.release !== "function") {
      throw new TypeError("InternalPluginBundle resident must release rooted state");
    }
    if (typeof registerContracts !== "function") {
      throw new TypeError("InternalPluginBundle must register descriptor/interface pairs");
    }
    this.#manifest = manifestBytes;
    this.#resident = resident;
    this.#registerContracts = registerContracts;
    this.#transferReserved = false;
  }

  _reserveInternalPluginTransfer() {
    if (this.#resident === null || this.#transferReserved) {
      throw new Error("InternalPluginBundle is already being registered or has been registered");
    }
    this.#transferReserved = true;
  }

  _cancelInternalPluginTransfer() {
    if (this.#resident !== null) {
      this.#transferReserved = false;
    }
  }

  /** Release an untransferred resident after a consumed registration attempt. */
  dispose() {
    if (this.#resident !== null) {
      this.#resident.release();
      this.#resident = null;
    }
    this.#transferReserved = false;
  }

  /** @returns {Uint8Array} Canonical manifest bytes held by this generated bundle. */
  _internalPluginManifest() {
    if (this.#resident === null) {
      throw new Error("InternalPluginBundle has already been registered");
    }
    return this.#manifest;
  }

  _registerGuestContracts(host) {
    if (!this.#transferReserved || this.#resident === null) {
      throw new Error("InternalPluginBundle registration is not active");
    }
    this.#registerContracts(host);
  }

  /** @returns {{ release: () => void }} The resident transferred to the Runtime. */
  _takeInternalPluginResident() {
    if (!this.#transferReserved || this.#resident === null) {
      throw new Error("InternalPluginBundle resident is not available for transfer");
    }
    const resident = this.#resident;
    this.#resident = null;
    this.#transferReserved = false;
    return resident;
  }
}

function writeVersion(view, offset, version) {
  view.setUint32(offset + VERSION_MAJOR_OFFSET, version.major, true);
  view.setUint32(offset + VERSION_MINOR_OFFSET, version.minor, true);
  view.setUint32(offset + VERSION_PATCH_OFFSET, version.patch, true);
}

function writeStringView(view, offset, value, roots) {
  const bytes = _encoder.encode(value);
  roots.push(bytes);
  view.setBigUint64(offset + STRING_VIEW_PTR_OFFSET, _ffi.pointerValue(_ffi.pointerOf(bytes)), true);
  view.setBigUint64(offset + STRING_VIEW_LEN_OFFSET, BigInt(bytes.byteLength), true);
}

/**
 * Builds one native guest interface from a JavaScript implementation object or
 * factory. The opaque context is retained in the returned resident and is only
 * forwarded by native code to its own callbacks.
 *
 * @param {{
 *   contractId: bigint,
 *   version: { major: number, minor: number, patch: number },
 *   implementation: object | (() => object),
 *   methods: Array<(implementation: object, args: PolyPtr, out: PolyPtr, arena: PolyPtr) => number | void>,
 * }} spec
 */
export function buildInternalPluginGuestContract(spec, bridgeLibrary) {
  if (spec === null || typeof spec !== "object" || typeof spec.contractId !== "bigint"
    || !Array.isArray(spec.methods)) {
    throw new TypeError("buildInternalPluginGuestContract requires a contract id and method adapters");
  }
  if (bridgeLibrary === null || typeof bridgeLibrary !== "object"
    || bridgeLibrary.symbols === null || typeof bridgeLibrary.symbols !== "object") {
    throw new TypeError("buildInternalPluginGuestContract requires an explicit polyplug_js bridge library");
  }
  const symbols = bridgeLibrary.symbols;
  for (const name of [
    "polyplug_js_internal_plugin_bridge_create",
    "polyplug_js_internal_plugin_bridge_interface",
    "polyplug_js_internal_plugin_bridge_free",
  ]) {
    if (typeof symbols[name] !== "function") {
      throw new TypeError(`polyplug_js bridge library is missing ${name}`);
    }
  }

  const factory = typeof spec.implementation === "function"
    ? spec.implementation
    : () => spec.implementation;
  const instances = new Map();
  const defaultImplementation = factory(null);
  let nextInstanceId = 1n;
  const createImplementation = (host) => {
    try {
      const id = nextInstanceId;
      nextInstanceId += 1n;
      instances.set(id, factory(host));
      return id;
    } catch (error) {
      console.error(`polyplug: internal-plugin create_instance threw: ${error}`);
      return 0n;
    }
  };
  const destroyImplementation = (instanceData) => {
    instances.delete(_ffi.pointerValue(instanceData));
  };
  const dispatchImplementation = (instanceData, functionId, args, out, arena) => {
    try {
      const implementation = _ffi.pointerValue(instanceData) === 0n
        ? defaultImplementation
        : instances.get(_ffi.pointerValue(instanceData));
      const invoke = spec.methods[functionId];
      if (implementation === undefined || typeof invoke !== "function") {
        return AbiErrorCode.FunctionNotAvailable;
      }
      const code = invoke(implementation, args, out, arena);
      return typeof code === "number" ? code : AbiErrorCode.Ok;
    } catch (error) {
      console.error(`polyplug: internal-plugin dispatch threw: ${error}`);
      return AbiErrorCode.Panic;
    }
  };

  const create = _ffi.makeCallback(
    { parameters: ["pointer", "pointer"], result: "u64" },
    (host, _args) => createImplementation(host),
  );
  const destroy = _ffi.makeCallback(
    { parameters: ["pointer"], result: "void" },
    (instanceData) => destroyImplementation(instanceData),
  );
  const dispatch = _ffi.makeCallback(
    { parameters: ["pointer", "u32", "pointer", "pointer", "pointer"], result: "u32" },
    (instanceData, functionId, args, out, arena) => dispatchImplementation(instanceData, functionId, args, out, arena),
  );
  const bridgeResident = symbols.polyplug_js_internal_plugin_bridge_create(
    dispatch.pointer,
    destroy.pointer,
    create.pointer,
    spec.contractId,
    spec.version.major,
    spec.version.minor,
    spec.version.patch,
  );
  if (bridgeResident === null) {
    destroy.close();
    create.close();
    dispatch.close();
    throw new Error("polyplug_js bridge could not create an internal-plugin adapter");
  }
  const interfacePtr = symbols.polyplug_js_internal_plugin_bridge_interface(bridgeResident);
  if (interfacePtr === null) {
    symbols.polyplug_js_internal_plugin_bridge_free(bridgeResident);
    destroy.close();
    create.close();
    dispatch.close();
    throw new Error("polyplug_js bridge returned an incomplete internal-plugin adapter");
  }

  let released = false;
  const resident = {
    release() {
      if (released) {
        return;
      }
      released = true;
      symbols.polyplug_js_internal_plugin_bridge_free(bridgeResident);
      instances.clear();
      dispatch.close();
      destroy.close();
      create.close();
    },
  };
  return {
    interfacePtr,
    resident,
    _createForTest: createImplementation,
    _destroyForTest: destroyImplementation,
    _dispatchForTest: dispatchImplementation,
    roots: [bridgeLibrary, bridgeResident, dispatch, destroy, create, instances, defaultImplementation, factory],
  };
}

/**
 * Creates the pointer bridge consumed by generated internal JavaScript provider
 * bindings. The bridge is per registration attempt and owns no plugin state;
 * the generated bundle resident retains the explicit native bridge library.
 *
 * @param {{ symbols: { polyplug_js_internal_plugin_arena_alloc: (arena: PolyPtr, size: bigint) => PolyPtr } }} bridgeLibrary
 */
export function createInternalPluginGuestBridge(bridgeLibrary) {
  if (bridgeLibrary === null || typeof bridgeLibrary !== "object"
    || bridgeLibrary.symbols === null
    || typeof bridgeLibrary.symbols.polyplug_js_internal_plugin_arena_alloc !== "function") {
    throw new TypeError("createInternalPluginGuestBridge requires the polyplug_js bridge library");
  }
  let host = null;
  const pointerAt = (address) => {
    if (!Number.isSafeInteger(address) || address <= 0) {
      throw new RangeError("generated JavaScript binding received an invalid pointer");
    }
    const pointer = _ffi.pointerCreate(BigInt(address));
    if (pointer === null) {
      throw new RangeError("generated JavaScript binding received a null pointer");
    }
    return pointer;
  };
  const viewAt = (address, size) => {
    return new DataView(_ffi.pointerView(pointerAt(address)).getArrayBuffer(size));
  };
  const splitPointer = (pointer) => {
    if (pointer === null) {
      throw new Error("generated JavaScript binding could not allocate return storage");
    }
    const value = _ffi.pointerValue(pointer);
    return [Number(value & 0xFFFF_FFFFn), Number(value >> 32n)];
  };
  return Object.freeze({
    addressOf(pointer) {
      return pointer === null ? 0 : Number(_ffi.pointerValue(pointer));
    },
    setHost(value) {
      host = value;
    },
    readByte(address) {
      return viewAt(address, 1).getUint8(0);
    },
    readU32(address) {
      return viewAt(address, 4).getUint32(0, true);
    },
    readI32(address) {
      return viewAt(address, 4).getInt32(0, true);
    },
    readF32(address) {
      return viewAt(address, 4).getFloat32(0, true);
    },
    readF64(address) {
      return viewAt(address, 8).getFloat64(0, true);
    },
    writeByte(address, value) {
      viewAt(address, 1).setUint8(0, value);
    },
    writeU32(address, value) {
      viewAt(address, 4).setUint32(0, value, true);
    },
    writeI32(address, value) {
      viewAt(address, 4).setInt32(0, value, true);
    },
    writeF32(address, value) {
      viewAt(address, 4).setFloat32(0, value, true);
    },
    writeF64(address, value) {
      viewAt(address, 8).setFloat64(0, value, true);
    },
    arenaAlloc(size, arenaAddress) {
      if (arenaAddress === 0) {
        if (host === null) {
          throw new Error("generated JavaScript binding has no host allocator");
        }
        return splitPointer(callHostMethod(
          host,
          HOST_API_OFFSETS.alloc,
          ["pointer", "usize", "usize"],
          "pointer",
          [host, BigInt(size), 1n],
        ));
      }
      const arena = pointerAt(arenaAddress);
      return splitPointer(
        bridgeLibrary.symbols.polyplug_js_internal_plugin_arena_alloc(arena, BigInt(size)),
      );
    },
  });
}

/**
 * Combines generated contract adapters with their canonical manifest bytes.
 * Existing PluginDescriptor and GuestContractInterface values are registered
 * directly during the runtime's staging transaction.
 *
 * @param {{
 *   manifest: string | Uint8Array,
 *   contracts: Array<{
 *     provider: string,
 *     contractName: string,
 *     version: { major: number, minor: number, patch: number },
 *     adapter: ReturnType<typeof buildInternalPluginGuestContract>,
 *   }>,
 * }} spec
 * @returns {InternalPluginBundle}
 */
export function buildInternalPluginBundle(spec) {
  if (spec === null || typeof spec !== "object" || !Array.isArray(spec.contracts)
    || spec.contracts.length === 0) {
    throw new TypeError("buildInternalPluginBundle requires a canonical manifest and at least one contract adapter");
  }

  const roots = [];
  const registrations = [];
  for (const contract of spec.contracts) {
    if (contract === null || typeof contract !== "object"
      || typeof contract.provider !== "string" || typeof contract.contractName !== "string"
      || contract.adapter === null || typeof contract.adapter !== "object"
      || contract.adapter.interfacePtr === null || contract.adapter.interfacePtr === undefined) {
      throw new TypeError("buildInternalPluginBundle contract must provide a descriptor and guest interface");
    }

    const descriptor = new Uint8Array(PLUGIN_DESCRIPTOR_SIZE);
    const descriptorView = new DataView(descriptor.buffer);
    writeStringView(descriptorView, PLUGIN_DESCRIPTOR_NAME_OFFSET, contract.provider, roots);
    writeStringView(
      descriptorView,
      PLUGIN_DESCRIPTOR_CONTRACT_NAME_OFFSET,
      contract.contractName,
      roots,
    );
    writeVersion(descriptorView, PLUGIN_DESCRIPTOR_VERSION_OFFSET, contract.version);
    roots.push(descriptor, contract.adapter);
    registrations.push({ descriptor, adapter: contract.adapter, contractName: contract.contractName });
  }

  let released = false;
  const resident = {
    release() {
      if (released) {
        return;
      }
      released = true;
      for (const root of roots) {
        root.resident?.release?.();
      }
      roots.length = 0;
    },
  };
  return new InternalPluginBundle(spec.manifest, resident, (host) => {
    for (const registration of registrations) {
      const error = new Uint8Array(ABI_ERROR_SIZE);
      callHostMethod(
        host,
        HOST_API_OFFSETS.register_guest_contract,
        ["pointer", "pointer", "pointer", "pointer"],
        "void",
        [host, _ffi.pointerOf(registration.descriptor), registration.adapter.interfacePtr, _ffi.pointerOf(error)],
      );
      if (new DataView(error.buffer).getUint32(0, true) !== AbiErrorCode.Ok) {
        throw new Error(`register_guest_contract failed for ${registration.contractName}`);
      }
    }
  });
}

/**
 * Read a function pointer from HostApi at given offset.
 * The raw 64-bit value is wrapped into a native pointer object so it can be
 * passed to the backend's function-pointer caller (which rejects bare BigInts).
 * @param {PolyPtr} hostPtr - HostApi pointer
 * @param {number} offset - Byte offset in struct
 * @returns {PolyPtr} Function pointer
 */
function readHostField(hostPtr, offset) {
  const view = _ffi.pointerView(hostPtr);
  return _ffi.pointerCreate(view.getBigUint64(offset));
}

/**
 * Call a HostApi method with self-passing pattern.
 * @param {PolyPtr} hostPtr - HostApi pointer
 * @param {number} fieldOffset - Offset of the function pointer field
 * @param {Array} paramTypes - FFI parameter types
 * @param {string|object} resultType - FFI result type
 * @param {Array} args - Arguments to pass (first arg is always hostPtr)
 * @returns {*} Result from FFI call
 */
function callHostMethod(hostPtr, fieldOffset, paramTypes, resultType, args) {
  const rawFunctionPointer = _ffi.pointerView(hostPtr).getBigUint64(fieldOffset);
  const funcPtr = _ffi.pointerCreate(rawFunctionPointer);
  if (funcPtr === null) {
    throw new Error(`HostApi field at offset ${fieldOffset} is null (raw ${rawFunctionPointer})`);
  }

  // Create function definition for this call
  const fnDef = { parameters: paramTypes, result: resultType };

  // Call through the function pointer
  return _ffi.callFunction(funcPtr, fnDef, args);
}

/**
 * Decoded view over a raw `GuestContractInterface*` pointer.
 *
 * `resolveGuestContract` returns a raw native pointer; the FFI backend does not
 * auto-decode C structs. This view reads the `#[repr(C)] GuestContractInterface`
 * fields at their byte offsets (see polyplug_abi guest_contract_interface.rs and
 * the auto-generated abi.ts offset constants) and exposes the lifecycle function
 * pointers, the dispatch type, the function count, and a per-slot dispatch entry
 * callable as `dispatch(slot, instance, argsPtr, outPtr)`.
 *
 * Layout (64 bytes):
 *   contract_id (u64)        @ 0
 *   contract_version (12)    @ 8
 *   dispatch_type (u32)      @ 20
 *   adapter_context (ptr)    @ 24
 *   create_instance (fn ptr) @ 32
 *   destroy_instance (fn ptr)@ 40
 *   dispatch (union, 16)     @ 48  (Native: function_count u32 @ +0, functions ptr @ +8)
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
  #interfacePtr;  // raw GuestContractInterface* (PolyPtr)
  #dispatchType;
  #adapterContext;
  #functionCount;
  #createInstancePtr;
  #destroyInstancePtr;
  #nativeFunctionsPtr;  // *const *const () (Native dispatch)
  #vmCallPtr;           // VmDispatch.call fn ptr (VM dispatch)
  #vmLoaderData;        // VmLoaderData (raw u64) (VM dispatch)
  #fnPtrCache;          // Map<slot, FfiFunction> for native dispatch

  /**
   * @param {PolyPtr} host - HostApi pointer
   * @param {PolyPtr} interfacePtr - Raw GuestContractInterface pointer
   */
  constructor(host, interfacePtr) {
    this.#host = host;
    this.#interfacePtr = interfacePtr;
    this.#fnPtrCache = new Map();

    const view = _ffi.pointerView(interfacePtr);
    this.#dispatchType = view.getUint32(GUEST_CONTRACT_INTERFACE_DISPATCH_TYPE_OFFSET);
    this.#adapterContext = _ffi.pointerCreate(
      view.getBigUint64(GUEST_CONTRACT_INTERFACE_ADAPTER_CONTEXT_OFFSET)
    );
    this.#createInstancePtr = _ffi.pointerCreate(
      view.getBigUint64(GUEST_CONTRACT_INTERFACE_CREATE_INSTANCE_OFFSET)
    );
    this.#destroyInstancePtr = _ffi.pointerCreate(
      view.getBigUint64(GUEST_CONTRACT_INTERFACE_DESTROY_INSTANCE_OFFSET)
    );

    const dispatchBase = GUEST_CONTRACT_INTERFACE_DISPATCH_OFFSET;
    if (this.#dispatchType === DISPATCH_TYPE_VIRTUAL_MACHINE) {
      // VmDispatch { call: fn ptr @ 0, loader_data: VmLoaderData @ 8 }.
      this.#functionCount = 0;
      this.#vmCallPtr = _ffi.pointerCreate(
        view.getBigUint64(dispatchBase + VM_DISPATCH_CALL_OFFSET)
      );
      this.#vmLoaderData = view.getBigUint64(dispatchBase + VM_DISPATCH_LOADER_DATA_OFFSET);
      this.#nativeFunctionsPtr = null;
    } else {
      // NativeDispatch { function_count: u32 @ 0, functions: *const *const () @ 8 }.
      this.#functionCount = view.getUint32(dispatchBase + NATIVE_DISPATCH_FUNCTION_COUNT_OFFSET);
      this.#nativeFunctionsPtr = _ffi.pointerCreate(
        view.getBigUint64(dispatchBase + NATIVE_DISPATCH_FUNCTIONS_OFFSET)
      );
      this.#vmCallPtr = null;
      this.#vmLoaderData = 0n;
    }
  }

  /** @returns {PolyPtr} Raw interface pointer (validity token). */
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
    // Out-param ABI: create_instance(adapter_context, loader_data: VmLoaderData,
    // host: *const HostApi, args: *const (), out_instance: *mut GuestContractInstance)
    // -> void. The interface's opaque adapter context is forwarded unchanged.
    const instance = new Uint8Array(GUEST_CONTRACT_INSTANCE_SIZE);
    const loaderData = _ffi.pointerCreate(this.#vmLoaderData);
    _ffi.callFunction(
      this.#createInstancePtr,
      { parameters: ["pointer", "pointer", "pointer", "pointer", "pointer"], result: "void" },
      [this.#adapterContext, loaderData, this.#host, null, _ffi.pointerOf(instance)],
    );
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
    // destroy_instance(adapter_context, loader_data: VmLoaderData,
    // host: *const HostApi, instance: GuestContractInstance) -> void.
    const loaderData = _ffi.pointerCreate(this.#vmLoaderData);
    _ffi.callFunction(
      this.#destroyInstancePtr,
      { parameters: ["pointer", "pointer", "pointer", { struct: ["pointer", "u64"] }], result: "void" },
      [this.#adapterContext, loaderData, this.#host, instance],
    );
  }

  /**
   * Dispatch a method directly through the resolved interface.
   *
   * This mirrors the canonical host-caller path (see polyplugc rust generator).
   * Out-param ABI: dispatch fns return void and write their AbiError through a
   * - Native: `dispatch.native.functions[slot](adapter_context, instance, args, out, out_err) -> void`.
   * - VM: `dispatch.vm.call(adapter_context, loader_data, instance, fn_id, args, out, arena, out_err) -> void`.
   *
   * Direct interface dispatch is the supported mechanism and works for both native
   * and VM (QuickJS/Lua/Python) guests, including stateless ones whose instance
   * carries a null `data`.
   * @param {number} slot - function_id / method index.
   * @param {Uint8Array} instance - GuestContractInstance struct (16 bytes, by value).
   * @param {PolyPtr} argsPtr - Pointer to packed args (or null).
   * @param {PolyPtr} outPtr - Pointer to output buffer (or null).
   * @returns {number} AbiError code (0 = Ok).
   */
  dispatch(slot, instance, argsPtr, outPtr) {
    // Out-param ABI: dispatch fns return void and write their AbiError through a
    // trailing *mut AbiError. Allocate a zeroed buffer and read `code` back.
    const errBuf = new Uint8Array(ABI_ERROR_SIZE);
    const errPtr = _ffi.pointerOf(errBuf);
    if (this.#dispatchType === DISPATCH_TYPE_VIRTUAL_MACHINE) {
      if (this.#vmCallPtr === null) {
        return AbiErrorCode.InvalidPointer; // null VM dispatch function.
      }
      // call(adapter_context, loader_data: VmLoaderData, instance, fn_id: u32,
      // args, out, arena, out_err: *mut AbiError) -> void.
      const loaderData = _ffi.pointerCreate(this.#vmLoaderData);
      _ffi.callFunction(
        this.#vmCallPtr,
        {
          parameters: ["pointer", "pointer", GUEST_CONTRACT_INSTANCE_STRUCT, "u32", "pointer", "pointer", "pointer", "pointer"],
          result: "void",
        },
        [this.#adapterContext, loaderData, instance, slot, argsPtr, outPtr, null, errPtr],
      );
    } else {
      const fn = this.#nativeFnPointer(slot);
      if (fn === null) {
        return AbiErrorCode.InvalidPointer; // null native function slot.
      }
      // functions[slot](adapter_context, instance, args, out, out_err) -> void.
      fn.call(this.#adapterContext, instance, argsPtr, outPtr, errPtr);
    }
    return new DataView(errBuf.buffer).getUint32(0, true);
  }

  /**
   * Resolve (and cache) the native dispatch callable for `slot`.
   * @param {number} slot - function_id / method index.
   * @returns {FfiFunction | null}
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
    const slotPtrRaw = _ffi.pointerView(this.#nativeFunctionsPtr).getBigUint64(slot * 8);
    const fnPtr = _ffi.pointerCreate(slotPtrRaw);
    if (fnPtr === null) {
      return null;
    }
    // Out-param ABI: functions[slot](adapter_context, instance, args, out, out_err) -> void.
    const fn = _ffi.prepareFunction(fnPtr, {
      parameters: ["pointer", GUEST_CONTRACT_INSTANCE_STRUCT, "pointer", "pointer", "pointer"],
      result: "void",
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
  // Per-instance FFI callback handles (on_reload / log trampolines).
  // Owned by THIS runtime (Rule 12: no module globals); closed on destroy.
  #callbacks;
  // Generated internal plugins transfer their rooted callback/table residents
  // here only after core accepts their complete registration.
  #internalPluginResidents;
  #destroyed;

  /**
   * @param {FfiLibrary} lib - Dynamic library instance
   * @param {PolyPtr} host - HostApi pointer
   * @param {FfiCallback[]} [callbacks=[]] - Owned FFI callbacks to close on destroy
   */
  constructor(lib, host, callbacks = []) {
    this.#lib = lib;
    this.#host = host;
    this.#callbacks = callbacks;
    this.#destroyed = false;
    this.#internalPluginResidents = new Map();
  }

  /**
   * Destroy the native runtime, then close the owned FFI callbacks.
   * The native side may invoke the trampolines up until
   * polyplug_runtime_destroy returns, so close() happens strictly after a
   * successful destroy. Returns false without releasing state when the native
   * runtime rejects this thread; callers may retry destroy() on its owner.
   * @returns {boolean}
   */
  destroy() {
    if (this.#destroyed) {
      return true;
    }
    if (!this.#lib.symbols.polyplug_runtime_destroy(this.#host)) {
      return false;
    }
    this.#destroyed = true;
    for (const resident of this.#internalPluginResidents.values()) {
      resident.release();
    }
    this.#internalPluginResidents.clear();
    for (const cb of this.#callbacks) {
      cb.close();
    }
    this.#callbacks = [];
    return true;
  }

  [Symbol.dispose]() {
    return this.destroy();
  }

  /**
   * Get HostApi pointer.
   * @returns {PolyPtr}
   */
  host() {
    return this.#host;
  }

  /**
   * Get library instance.
   * @returns {FfiLibrary}
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
    const bufPtr = _ffi.pointerOf(buf);
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
    const ptr = _ffi.pointerOf(encoded);
    // Out-param ABI: load_bundle returns void and writes its AbiError through a
    // trailing *mut AbiError.
    const errBuf = new Uint8Array(ABI_ERROR_SIZE);
    callHostMethod(
      this.#host,
      HOST_API_OFFSETS.load_bundle,
      ["pointer", "pointer", "usize", "pointer"],
      "void",
      [this.#host, ptr, BigInt(encoded.length), _ffi.pointerOf(errBuf)]
    );
    const code = new DataView(errBuf.buffer).getUint32(0, true);
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
    const ptr = _ffi.pointerOf(encoded);
    // Out-param ABI: reload_bundle returns void and writes its AbiError through a
    // trailing *mut AbiError.
    const errBuf = new Uint8Array(ABI_ERROR_SIZE);
    callHostMethod(
      this.#host,
      HOST_API_OFFSETS.reload_bundle,
      ["pointer", "pointer", "usize", "pointer"],
      "void",
      [this.#host, ptr, BigInt(encoded.length), _ffi.pointerOf(errBuf)]
    );
    const code = new DataView(errBuf.buffer).getUint32(0, true);
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
    // Out-param ABI: unload_bundle returns void and writes its AbiError through a
    // trailing *mut AbiError.
    const errBuf = new Uint8Array(ABI_ERROR_SIZE);
    callHostMethod(
      this.#host,
      HOST_API_OFFSETS.unload_bundle,
      ["pointer", "u64", "pointer"],
      "void",
      [this.#host, bundleId, _ffi.pointerOf(errBuf)]
    );
    const code = new DataView(errBuf.buffer).getUint32(0, true);
    if (code !== 0) {
      throw new Error(`unloadBundle failed: ${this.lastError()}`);
    }
    const resident = this.#internalPluginResidents.get(bundleId);
    if (resident !== undefined) {
      this.#internalPluginResidents.delete(bundleId);
      resident.release();
    }
  }

  /**
   * Synchronously stage and commit one generated internal plugin.
   *
   * The generated bundle owns canonical manifest bytes and rooted callbacks,
   * implementations, descriptors, and interfaces. Core receives each existing
   * PluginDescriptor/GuestContractInterface pair through HostApi while staging.
   * The resident transfers only after commit succeeds; logical unload releases it
   * only after core drains calls, instances, and leases.
   * @param {{
   *   _reserveInternalPluginTransfer: () => void,
   *   _cancelInternalPluginTransfer: () => void,
   *   _internalPluginManifest: () => Uint8Array,
   *   _registerGuestContracts: (host: PolyPtr) => void,
   *   _takeInternalPluginResident: () => { release: () => void },
   * }} bundle
   * @returns {bigint} Stable bundle identifier.
   */
  registerInternalPlugin(bundle) {
    if (this.#destroyed) {
      throw new Error("registerInternalPlugin: runtime is destroyed");
    }
    if (bundle === null || typeof bundle !== "object"
      || typeof bundle._reserveInternalPluginTransfer !== "function"
      || typeof bundle._cancelInternalPluginTransfer !== "function"
      || typeof bundle._internalPluginManifest !== "function"
      || typeof bundle._registerGuestContracts !== "function"
      || typeof bundle._takeInternalPluginResident !== "function") {
      throw new TypeError("registerInternalPlugin expects a generated internal plugin");
    }

    bundle._reserveInternalPluginTransfer();
    let staged = false;
    let bundleId = 0n;
    try {
      const manifest = bundle._internalPluginManifest();
      if (!(manifest instanceof Uint8Array) || manifest.byteLength === 0) {
        throw new TypeError("generated internal plugin returned invalid manifest bytes");
      }

      const bundleIdBuf = new Uint8Array(8);
      const beginError = new Uint8Array(ABI_ERROR_SIZE);
      this.#lib.symbols.polyplug_begin_internal_plugin(
        this.#host,
        _ffi.pointerOf(manifest),
        BigInt(manifest.byteLength),
        5,
        _ffi.pointerOf(bundleIdBuf),
        _ffi.pointerOf(beginError),
      );
      if (new DataView(beginError.buffer).getUint32(0, true) !== AbiErrorCode.Ok) {
        throw new Error(`registerInternalPlugin failed to begin: ${this.lastError()}`);
      }

      bundleId = new DataView(bundleIdBuf.buffer).getBigUint64(0, true);
      staged = true;
      bundle._registerGuestContracts(this.#host);

      const commitError = new Uint8Array(ABI_ERROR_SIZE);
      this.#lib.symbols.polyplug_commit_internal_plugin(
        this.#host,
        bundleId,
        _ffi.pointerOf(commitError),
      );
      staged = false;
      if (new DataView(commitError.buffer).getUint32(0, true) !== AbiErrorCode.Ok) {
        throw new Error(`registerInternalPlugin failed to commit: ${this.lastError()}`);
      }

      if (this.#internalPluginResidents.has(bundleId)) {
        throw new Error("registerInternalPlugin: runtime returned a duplicate live bundle id");
      }
      const resident = bundle._takeInternalPluginResident();
      if (resident === null || typeof resident !== "object" || typeof resident.release !== "function") {
        throw new Error("generated internal plugin lost its resident during registration");
      }
      this.#internalPluginResidents.set(bundleId, resident);
      return bundleId;
    } catch (error) {
      if (staged) {
        this.#lib.symbols.polyplug_abort_internal_plugin(this.#host, bundleId);
      }
      bundle._cancelInternalPluginTransfer();
      throw error;
    }
  }

  /**
   * Stage and commit a generated internal bundle, returning the exact committed
   * contract handles in registration order.
   *
   * @param {{
   *   _reserveInternalPluginTransfer: () => void,
   *   _cancelInternalPluginTransfer: () => void,
   *   _internalPluginManifest: () => Uint8Array,
   *   _registerGuestContracts: (host: PolyPtr) => void,
   *   _takeInternalPluginResident: () => { release: () => void },
   * }} bundle
   * @param {number} handleCount Number of generated provider contracts.
   * @returns {{ bundleId: bigint, handles: Array<{ index: number, generation: number }> }}
   */
  registerInternalPluginWithHandles(bundle, handleCount) {
    if (this.#destroyed) {
      throw new Error("registerInternalPluginWithHandles: runtime is destroyed");
    }
    if (!Number.isSafeInteger(handleCount) || handleCount <= 0) {
      throw new TypeError("registerInternalPluginWithHandles requires a positive handle count");
    }
    if (bundle === null || typeof bundle !== "object"
      || typeof bundle._reserveInternalPluginTransfer !== "function"
      || typeof bundle._cancelInternalPluginTransfer !== "function"
      || typeof bundle._internalPluginManifest !== "function"
      || typeof bundle._registerGuestContracts !== "function"
      || typeof bundle._takeInternalPluginResident !== "function") {
      throw new TypeError("registerInternalPluginWithHandles expects a generated internal plugin");
    }
    bundle._reserveInternalPluginTransfer();
    let staged = false;
    let bundleId = 0n;
    try {
      const manifest = bundle._internalPluginManifest();
      if (!(manifest instanceof Uint8Array) || manifest.byteLength === 0) {
        throw new TypeError("generated internal plugin returned invalid manifest bytes");
      }
      const bundleIdBuf = new Uint8Array(8);
      const beginError = new Uint8Array(ABI_ERROR_SIZE);
      this.#lib.symbols.polyplug_begin_internal_plugin(
        this.#host,
        _ffi.pointerOf(manifest),
        BigInt(manifest.byteLength),
        5,
        _ffi.pointerOf(bundleIdBuf),
        _ffi.pointerOf(beginError),
      );
      if (new DataView(beginError.buffer).getUint32(0, true) !== AbiErrorCode.Ok) {
        throw new Error(`registerInternalPluginWithHandles failed to begin: ${this.lastError()}`);
      }
      bundleId = new DataView(bundleIdBuf.buffer).getBigUint64(0, true);
      staged = true;
      bundle._registerGuestContracts(this.#host);
      const handleBytes = new Uint8Array(handleCount * GUEST_CONTRACT_HANDLE_SIZE);
      const handleCountBytes = new Uint8Array(8);
      const commitError = new Uint8Array(ABI_ERROR_SIZE);
      this.#lib.symbols.polyplug_commit_internal_plugin_with_handles(
        this.#host,
        bundleId,
        _ffi.pointerOf(handleBytes),
        BigInt(handleCount),
        _ffi.pointerOf(handleCountBytes),
        _ffi.pointerOf(commitError),
      );
      staged = false;
      if (new DataView(commitError.buffer).getUint32(0, true) !== AbiErrorCode.Ok) {
        throw new Error(`registerInternalPluginWithHandles failed to commit: ${this.lastError()}`);
      }
      const committedCount = Number(new DataView(handleCountBytes.buffer).getBigUint64(0, true));
      if (committedCount !== handleCount) {
        throw new Error(`generated internal registration committed ${committedCount} handles, expected ${handleCount}`);
      }
      const handles = [];
      const view = new DataView(handleBytes.buffer);
      for (let index = 0; index < handleCount; index++) {
        const offset = index * GUEST_CONTRACT_HANDLE_SIZE;
        handles.push({
          index: view.getUint32(offset + GUEST_CONTRACT_HANDLE_INDEX_OFFSET, true),
          generation: view.getUint32(offset + GUEST_CONTRACT_HANDLE_GENERATION_OFFSET, true),
        });
      }
      if (this.#internalPluginResidents.has(bundleId)) {
        throw new Error("registerInternalPluginWithHandles: runtime returned a duplicate live bundle id");
      }
      const resident = bundle._takeInternalPluginResident();
      if (resident === null || typeof resident !== "object" || typeof resident.release !== "function") {
        throw new Error("generated internal plugin lost its resident during registration");
      }
      this.#internalPluginResidents.set(bundleId, resident);
      return { bundleId, handles };
    } catch (error) {
      if (staged) {
        this.#lib.symbols.polyplug_abort_internal_plugin(this.#host, bundleId);
      }
      bundle._cancelInternalPluginTransfer();
      throw error;
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
    const arrPtr = _ffi.pointerCreate(arrPtrRaw);

    if (arrPtr === null || arrLen === 0) {
      return [];
    }

    // Read handles from array. GuestContractHandle is
    // `#[repr(C)] { index: u32, generation: u32 }` (8 bytes, align 4), so
    // elements have an 8-byte stride; each is read as a { index, generation }.
    const handles = [];
    const arrView = _ffi.pointerView(arrPtr);
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
   * @returns {PolyPtr} Resolved interface pointer (null if invalid/stale)
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
   * Return the synchronized monotonic registry revision (HostApi.registry_revision).
   *
   * The Rust callback performs the acquire load before returning the value, so every
   * JavaScript host caller uses the same reload/unload synchronization as native
   * callers. An observed change means its cached interface/instance must be
   * re-resolved before use.
   * @returns {bigint} Current registry revision.
   */
  registryRevision() {
    return callHostMethod(
      this.#host,
      HOST_API_OFFSETS.registry_revision,
      ["pointer"],
      "u64",
      [this.#host]
    );
  }

  /**
   * Allocate `size` bytes via the host allocator (HostApi.alloc).
   *
   * All memory crossing the plugin boundary must use the host allocator. The
   * returned pointer must be released via {@link Runtime#free} with the same
   * size and alignment.
   * @param {number} size - Number of bytes to allocate.
   * @param {number} [align=1] - Allocation alignment.
   * @returns {PolyPtr} Pointer to the allocated region (or null).
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
   * @param {PolyPtr} ptr - Pointer to free.
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
   * @param {PolyPtr} hostInterface - Pointer to HostContractInterface struct
   */
  registerHostContract(hostInterface) {
    // Out-param ABI: register_host_contract returns void and writes its AbiError
    // through a trailing *mut AbiError.
    const errBuf = new Uint8Array(ABI_ERROR_SIZE);
    callHostMethod(
      this.#host,
      HOST_API_OFFSETS.register_host_contract,
      ["pointer", "pointer", "pointer"],
      "void",
      [this.#host, hostInterface, _ffi.pointerOf(errBuf)]
    );
    const code = new DataView(errBuf.buffer).getUint32(0, true);
    if (code !== 0) {
      throw new Error(`registerHostContract failed: ${this.lastError()}`);
    }
  }

  /**
   * Register a language loader with the runtime.
   * Calls through HostApi.register_loader field. The loader's runtime name
   * comes from its own BundleLoader::runtime_name(); the AbiError is written
   * through the trailing out-param (code is the first u32).
   * @param {PolyPtr} loaderPtr - Opaque loader pointer from the loader cdylib's create function.
   */
  registerLoader(loaderPtr) {
    // Out-param ABI: register_loader returns void and writes its AbiError through
    // a trailing *mut AbiError.
    const errBuf = new Uint8Array(ABI_ERROR_SIZE);
    callHostMethod(
      this.#host,
      HOST_API_OFFSETS.register_loader,
      ["pointer", "pointer", "pointer"],
      "void",
      [this.#host, loaderPtr, _ffi.pointerOf(errBuf)]
    );

    const code = new DataView(errBuf.buffer).getUint32(0, true);
    if (code !== 0) {
      throw new Error(`registerLoader failed: ${this.lastError()}`);
    }
  }
}

/**
 * Open polyplug library.
 * @param {string} soPath - Path to libpolyplug.so
 * @returns {FfiLibrary}
 */
export function openPolyplug(soPath) {
  return _ffi.openLibrary(soPath, SYMBOLS);
}

/**
 * Pack an array of Ed25519 verifying keys into one contiguous N*32-byte buffer.
 *
 * Each entry must be exactly {@link ED25519_PUBLIC_KEY_SIZE} (32) raw bytes,
 * supplied as a Uint8Array or ArrayBuffer. The returned buffer is the storage
 * the runtime's `trusted_keys.items` pointer references during create; the
 * runtime copies the bytes out, so the caller only keeps it across that call.
 * @param {(Uint8Array|ArrayBuffer)[]} keys - Verifying keys, 32 bytes each.
 * @returns {Uint8Array} Contiguous key buffer (keys.length * 32 bytes).
 */
function marshalTrustedKeys(keys) {
  const keysBuf = new Uint8Array(keys.length * ED25519_PUBLIC_KEY_SIZE);
  for (let i = 0; i < keys.length; i += 1) {
    const key = keys[i] instanceof ArrayBuffer ? new Uint8Array(keys[i]) : keys[i];
    if (!(key instanceof Uint8Array) || key.length !== ED25519_PUBLIC_KEY_SIZE) {
      throw new Error(
        `trustedKeys[${i}] must be ${ED25519_PUBLIC_KEY_SIZE} bytes (Ed25519 public key), got ${key?.length}`,
      );
    }
    keysBuf.set(key, i * ED25519_PUBLIC_KEY_SIZE);
  }
  return keysBuf;
}

/**
 * Create new runtime instance.
 * Uses HostApi-based API: polyplug_runtime_create returns HostApi*.
 *
 * All configuration is per-instance (Rule 12: no module globals shared across
 * runtimes). The FFI callbacks created for `onReload` / `logger` are owned by
 * the returned Runtime and closed by {@link Runtime#destroy}.
 *
 * RuntimeConfig is the full 72-byte ABI struct: compatibility (u32 @ 0),
 * hot_reload_enabled (bool @ 4), on_reload (fn @ 8), on_reload_user_data (ptr @ 16),
 * log (fn @ 24), log_user_data (ptr @ 32), log_max_level (u32 @ 40),
 * signature_policy (u32 @ 44), trusted_keys (Array{ items @ 48, len @ 56,
 * align @ 64 }). The struct is 72 bytes. Offsets/size come from the abi.ts
 * constants.
 *
 * @param {FfiLibrary} lib - Dynamic library
 * @param {Object} [options] - Per-runtime options
 * @param {Object} [options.config] - RuntimeConfig fields
 * @param {number} [options.config.compatibility=0] - Compatibility mode (COMPATIBILITY_STRICT=0, RELAXED=1, YOLO=2)
 * @param {boolean} [options.config.hotReloadEnabled=false] - Whether hot-reload is enabled
 * @param {number} [options.config.logMaxLevel=5] - Max LogLevel (1=Error … 5=Trace) delivered to `logger`
 * @param {number} [options.config.signaturePolicy=0] - Bundle signature enforcement policy (SignaturePolicy.Off=0, WarnOnly=1, Required=2), written at offset 44
 * @param {(Uint8Array|ArrayBuffer)[]} [options.config.trustedKeys] - Ed25519 verifying-key allowlist (key pinning). Each entry is 32 raw bytes. Empty/unset = Trust-On-First-Use (default). The runtime copies the keys during create, so the backing buffer is only held across the create call.
 * @param {function(ReloadPhase): void} [options.onReload] - Hot-reload phase callback
 * @param {function(number, string, string): void} [options.logger] - Logger callback (level, scope, message)
 * @returns {Runtime}
 */
export function runtimeNew(lib, options = {}) {
  const config = options.config ?? null;
  const onReloadCallback = options.onReload ?? null;
  const loggerCallback = options.logger ?? null;

  /** @type {FfiCallback[]} */
  const ownedCallbacks = [];
  // The trusted-keys buffer (when pinned) lives here as a local: the runtime
  // copies the keys during polyplug_runtime_create, so it only needs to stay
  // reachable across that call.
  /** @type {Uint8Array | null} */
  let keysBuf = null;
  let host;

  if (config || onReloadCallback || loggerCallback) {
    const configBuf = new Uint8Array(RUNTIME_CONFIG_SIZE);
    const configView = new DataView(configBuf.buffer);

    configView.setUint32(RUNTIME_CONFIG_COMPATIBILITY_OFFSET, config?.compatibility ?? COMPATIBILITY_STRICT, true);
    configView.setUint8(RUNTIME_CONFIG_HOT_RELOAD_ENABLED_OFFSET, config?.hotReloadEnabled ? 1 : 0);

    // Bundle signature enforcement policy (SignaturePolicy, offset 44). Only
    // written when explicitly provided; the zeroed buffer already encodes the
    // SignaturePolicy.Off default (0), preserving existing behavior.
    if (config?.signaturePolicy !== undefined) {
      configView.setUint32(RUNTIME_CONFIG_SIGNATURE_POLICY_OFFSET, config.signaturePolicy, true);
    }

    // Trusted Ed25519 verifying-key allowlist (key pinning), the trusted_keys
    // Array at offset 48: { items @ 48, len @ 56, align @ 64 }. An empty/unset
    // list leaves the zeroed Array (null items, len 0) — Trust-On-First-Use.
    // The runtime copies this Array's keys during create, so the contiguous
    // N*32-byte buffer is held in the `keysBuf` local that stays reachable
    // across the polyplug_runtime_create call below.
    const trustedKeys = config?.trustedKeys ?? null;
    if (trustedKeys && trustedKeys.length > 0) {
      keysBuf = marshalTrustedKeys(trustedKeys);
      configView.setBigUint64(
        RUNTIME_CONFIG_TRUSTED_KEYS_OFFSET,
        _ffi.pointerValue(_ffi.pointerOf(keysBuf)),
        true,
      );
      configView.setBigUint64(RUNTIME_CONFIG_TRUSTED_KEYS_LEN_OFFSET, BigInt(trustedKeys.length), true);
      // Ed25519PublicKey is a 32-byte array, align 1.
      configView.setBigUint64(RUNTIME_CONFIG_TRUSTED_KEYS_ALIGN_OFFSET, 1n, true);
    }

    if (onReloadCallback) {
      const ffiReloadCallback = _ffi.makeCallback(_RELOAD_CALLBACK_TYPE,
        (_userData, phasePtr) => {
          // _userData is the opaque on_reload_user_data pointer (unused here — the
          // JS closure already captures the callback). phasePtr is a const pointer
          // to the 48-byte ReloadPhase; the runtime guarantees it is non-null and
          // valid only for the duration of this call, so every field is copied out
          // before the user callback returns. A JS exception must never unwind
          // across the C ABI mid-reload: catch-all, log to stderr.
          try {
            if (phasePtr === null) {
              // Contract: never happens. Defence-in-depth only.
              return;
            }
            const view = _ffi.pointerView(phasePtr);
            const phaseType = view.getUint32(RELOAD_PHASE_PHASE_TYPE_OFFSET);
            const bundleId = view.getBigUint64(RELOAD_PHASE_BUNDLE_ID_OFFSET);
            const bundleNamePtrRaw = view.getBigUint64(RELOAD_PHASE_BUNDLE_NAME_OFFSET);
            const bundleNameLen = Number(view.getBigUint64(RELOAD_PHASE_BUNDLE_NAME_OFFSET + 8));
            const reasonPtrRaw = view.getBigUint64(RELOAD_PHASE_REASON_OFFSET);
            const reasonLen = Number(view.getBigUint64(RELOAD_PHASE_REASON_OFFSET + 8));

            let bundleName = "";
            const bundleNamePtr = _ffi.pointerCreate(bundleNamePtrRaw);
            if (bundleNamePtr !== null && bundleNameLen > 0) {
              bundleName = utf8At(bundleNamePtr, bundleNameLen);
            }
            let reason = "";
            const reasonPtr = _ffi.pointerCreate(reasonPtrRaw);
            if (reasonPtr !== null && reasonLen > 0) {
              reason = utf8At(reasonPtr, reasonLen);
            }
            onReloadCallback(new ReloadPhase(phaseType, bundleId, bundleName, reason));
          } catch (e) {
            console.error(`polyplug: reload callback threw: ${e}`);
          }
        }
      );
      ownedCallbacks.push(ffiReloadCallback);
      // on_reload_user_data is left null: the JS closure already captures the callback.
      configView.setBigUint64(RUNTIME_CONFIG_ON_RELOAD_OFFSET, _ffi.pointerValue(ffiReloadCallback.pointer), true);
    }

    if (loggerCallback) {
      const ffiLogCallback = _ffi.makeCallback(_LOG_CALLBACK_TYPE,
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
      configView.setBigUint64(RUNTIME_CONFIG_LOG_OFFSET, _ffi.pointerValue(ffiLogCallback.pointer), true);
      // Default to Trace (5): deliver everything, filter inside the JS callback.
      configView.setUint32(RUNTIME_CONFIG_LOG_MAX_LEVEL_OFFSET, config?.logMaxLevel ?? 5, true);
    } else if (config?.logMaxLevel !== undefined) {
      configView.setUint32(RUNTIME_CONFIG_LOG_MAX_LEVEL_OFFSET, config.logMaxLevel, true);
    }

    const configPtr = _ffi.pointerOf(configBuf);
    host = lib.symbols.polyplug_runtime_create(configPtr);
    // The runtime copied the trusted keys during the call above; release the
    // local reference to the backing buffer.
    keysBuf = null;
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
