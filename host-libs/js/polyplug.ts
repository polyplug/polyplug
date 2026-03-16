// host-libs/js/polyplug.ts
// Deno.dlopen host library for polyplug.
// Requires --allow-ffi permission.

// The symbol table must use `as const satisfies Deno.ForeignLibraryInterface`
// for TypeScript type inference to work with Deno.dlopen.
const SYMBOLS = {
  polyplug_runtime_create: { parameters: [] as const, result: "pointer" as const },
  polyplug_runtime_destroy: { parameters: ["pointer"] as const, result: "void" as const },
  polyplug_runtime_load_bundle: { parameters: ["pointer", "pointer", "usize"] as const, result: "u32" as const },
  polyplug_runtime_reload_bundle: { parameters: ["pointer", "pointer", "usize"] as const, result: "u32" as const },
  polyplug_runtime_find_by_contract: { parameters: ["pointer", "u64", "u32"] as const, result: "u64" as const },
  polyplug_runtime_find_by_bundle: { parameters: ["pointer", "u64", "u64", "u32"] as const, result: "u64" as const },
  polyplug_runtime_find_all_by_contract: { parameters: ["pointer", "u64", "u32", "pointer", "usize"] as const, result: "usize" as const },
  polyplug_runtime_resolve_plugin: { parameters: ["pointer", "u64"] as const, result: "pointer" as const },
  polyplug_runtime_plugin_release: { parameters: ["pointer"] as const, result: "void" as const },
  polyplug_runtime_plugin_vtable: { parameters: ["pointer"] as const, result: "pointer" as const },
  polyplug_runtime_last_error: { parameters: ["pointer", "usize"] as const, result: "usize" as const },
  polyplug_runtime_error_message_len: { parameters: [] as const, result: "usize" as const },
} as const satisfies Deno.ForeignLibraryInterface;

export const NULL_HANDLE = 0xFFFFFFFFFFFFFFFFn;  // u64::MAX as BigInt

export class Runtime {
  readonly #lib: Deno.DynamicLibrary<typeof SYMBOLS>;
  readonly #ptr: Deno.PointerValue;

  constructor(lib: Deno.DynamicLibrary<typeof SYMBOLS>, ptr: Deno.PointerValue) {
    this.#lib = lib;
    this.#ptr = ptr;
  }

  [Symbol.dispose](): void {
    this.#lib.symbols.polyplug_runtime_destroy(this.#ptr);
  }

  ptr(): Deno.PointerValue {
    return this.#ptr;
  }

  lastError(): string {
    const len = Number(this.#lib.symbols.polyplug_runtime_error_message_len());
    if (len === 0) return "";
    const buf = new Uint8Array(len);
    const ptr = Deno.UnsafePointer.of(buf);
    this.#lib.symbols.polyplug_runtime_last_error(ptr, BigInt(len));
    return new TextDecoder().decode(buf);
  }

  loadBundle(path: string): void {
    const encoded = new TextEncoder().encode(path);
    const ptr = Deno.UnsafePointer.of(encoded);
    const result = this.#lib.symbols.polyplug_runtime_load_bundle(this.#ptr, ptr, BigInt(encoded.length));
    if (result !== 0) throw new Error(`polyplug_runtime_load_bundle failed: ${this.lastError()}`);
  }

  reloadBundle(path: string): void {
    const encoded = new TextEncoder().encode(path);
    const ptr = Deno.UnsafePointer.of(encoded);
    const result = this.#lib.symbols.polyplug_runtime_reload_bundle(this.#ptr, ptr, BigInt(encoded.length));
    if (result !== 0) throw new Error(`polyplug_runtime_reload_bundle failed: ${this.lastError()}`);
  }

  findByContract(contractId: bigint, minVersion = 0): bigint {
    return this.#lib.symbols.polyplug_runtime_find_by_contract(this.#ptr, contractId, minVersion) as bigint;
  }

  findByBundle(bundleId: bigint, contractId: bigint, minVersion = 0): bigint {
    return this.#lib.symbols.polyplug_runtime_find_by_bundle(this.#ptr, bundleId, contractId, minVersion) as bigint;
  }

  findAllByContract(contractId: bigint, minVersion = 0, cap = 64): bigint[] {
    const buf = new BigUint64Array(cap);
    const ptr = Deno.UnsafePointer.of(buf);
    const count = Number(this.#lib.symbols.polyplug_runtime_find_all_by_contract(this.#ptr, contractId, minVersion, ptr, BigInt(cap)));
    return Array.from(buf.slice(0, Math.min(count, cap)));
  }

  resolvePlugin(packedHandle: bigint): Guard {
    const ptr = this.#lib.symbols.polyplug_runtime_resolve_plugin(this.#ptr, packedHandle);
    if (ptr === null) throw new Error(`polyplug_runtime_resolve_plugin failed: ${this.lastError()}`);
    return new Guard(this.#lib, ptr);
  }
}

export class Guard {
  readonly #lib: Deno.DynamicLibrary<typeof SYMBOLS>;
  readonly #ptr: Deno.PointerValue;

  constructor(lib: Deno.DynamicLibrary<typeof SYMBOLS>, ptr: Deno.PointerValue) {
    this.#lib = lib;
    this.#ptr = ptr;
  }

  [Symbol.dispose](): void {
    this.#lib.symbols.polyplug_runtime_plugin_release(this.#ptr);
  }

  vtable(): Deno.PointerValue {
    return this.#lib.symbols.polyplug_runtime_plugin_vtable(this.#ptr);
  }
}

export function openPolyplug(soPath: string): Deno.DynamicLibrary<typeof SYMBOLS> {
  return Deno.dlopen(soPath, SYMBOLS);
}

export function runtimeNew(lib: Deno.DynamicLibrary<typeof SYMBOLS>): Runtime {
  const ptr = lib.symbols.polyplug_runtime_create();
  if (ptr === null) {
    // Read error without a Runtime object
    const lenVal = lib.symbols.polyplug_runtime_error_message_len();
    const len = Number(lenVal);
    let errMsg = "polyplug_runtime_create failed";
    if (len > 0) {
      const buf = new Uint8Array(len);
      lib.symbols.polyplug_runtime_last_error(Deno.UnsafePointer.of(buf), BigInt(len));
      errMsg += ": " + new TextDecoder().decode(buf);
    }
    throw new Error(errMsg);
  }
  return new Runtime(lib, ptr);
}

/**
 * Convert StringView to JavaScript string.
 * @param sv StringView from polyplug ABI
 * @returns JavaScript string
 */
export function toStr(sv: { ptr: bigint; len: number }): string {
    if (!sv || sv.ptr === 0n || sv.len === 0) return '';
    return new Deno.UnsafePointerView(sv.ptr).getUtf8String(sv.len);
}

/**
 * Alias for toStr().
 */
export const toString = toStr;

/**
 * Create StringView from JavaScript string (owned copy).
 * @param s JavaScript string
 * @returns StringView with allocated memory
 */
export function strAsView(s: string): { ptr: bigint; len: number } {
    const encoder = new TextEncoder();
    const data = encoder.encode(s);
    const ptr = Deno.UnsafePointer.of(new Uint8Array(data));
    return { ptr, len: data.length };
}
