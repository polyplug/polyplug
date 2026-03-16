// JS Deno host example using polyplugc-generated bindings.
//
// This host demonstrates the real-world polyplug pattern:
//   1. Generate host bindings: polyplugc --api api.toml --lang js-deno --out generated/
//   2. Import generated contract IDs from generated/host/callers.ts
//   3. Use generated constants instead of hard-coded values
//
// Zero hand-written contract IDs.

import {
  openPolyplug,
  runtimeNew,
  Runtime,
  Guard,
  NULL_HANDLE,
} from "../../../host-libs/js/polyplug.ts";

import { registerNativeLoader } from "../../../host-libs/js/loaders/native.ts";
import { registerDotnetLoader } from "../../../host-libs/js/loaders/dotnet.ts";
import { registerPythonLoader } from "../../../host-libs/js/loaders/python.ts";
import { registerLuaLoader } from "../../../host-libs/js/loaders/lua.ts";
import { registerJsLoader } from "../../../host-libs/js/loaders/js.ts";

// Import generated contract IDs
import { ContractIds } from "./generated/host/callers.ts";

const SZ_STRING_VIEW: number = 16;
const ABI_FN_RESULT_STRUCT = { struct: ["u32", "u32", "pointer", "usize"] } as const;

interface DiscoveredBundle {
  path: string;
  bundleName: string;
  provides: string[];
}

function readStringViewAt(view: DataView, offset: number): string {
  const ptrVal: bigint = view.getBigUint64(offset, true);
  const len: number = Number(view.getBigUint64(offset + 8, true));
  if (ptrVal === 0n || len === 0) return "";
  const ptr: Deno.PointerValue = Deno.UnsafePointer.create(ptrVal);
  const ptrView: Deno.UnsafePointerView = new Deno.UnsafePointerView(ptr!);
  const buf: ArrayBuffer = new ArrayBuffer(len);
  const bytes: Uint8Array<ArrayBuffer> = new Uint8Array(buf);
  ptrView.copyInto(bytes, 0);
  return new TextDecoder().decode(bytes);
}

function callVtableFn(
  vtablePtr: Deno.PointerValue,
  argsPtr: Deno.PointerValue,
  outPtr: Deno.PointerValue,
): number {
  if (vtablePtr === null) throw new Error("null vtablePtr");
  const vtableBuf: Deno.UnsafePointerView = new Deno.UnsafePointerView(vtablePtr);
  const functionsPtrVal: bigint = vtableBuf.getBigUint64(16);
  if (functionsPtrVal === 0n) throw new Error("vtable.functions is null");
  const functionsPtr: Deno.PointerValue = Deno.UnsafePointer.create(functionsPtrVal);
  const fnArrayView: Deno.UnsafePointerView = new Deno.UnsafePointerView(functionsPtr!);
  const fnPtrVal: bigint = fnArrayView.getBigUint64(0);
  if (fnPtrVal === 0n) throw new Error("fn[0] is null");
  const fnPtr: Deno.PointerValue = Deno.UnsafePointer.create(fnPtrVal);
  const fnDef = {
    parameters: ["pointer", "pointer"],
    result: ABI_FN_RESULT_STRUCT,
  } as const;
  const fnCall = new Deno.UnsafeFnPointer(fnPtr as any, fnDef);
  const result = fnCall.call(argsPtr, outPtr) as unknown as [number, number, Deno.PointerValue, number | bigint];
  return result[0];
}

function resolvePluginPath(): string {
  const envPath: string | undefined = Deno.env.get("POLYPLUG_PLUGIN_PATH");
  if (envPath && envPath.length > 0) {
    return envPath;
  }
  const scriptDir: string = new URL(".", import.meta.url).pathname;
  return scriptDir + "../../../plugins";
}

async function main(): Promise<void> {
  const pluginPath: string = resolvePluginPath();
  console.error(`plugin directory: ${pluginPath}`);

  const libPath: string = "../../../target/debug/libpolyplug.so";
  const lib = openPolyplug(libPath);
  const rt: Runtime = runtimeNew(lib);

  // Register all loaders
  registerNativeLoader(lib, rt);
  registerDotnetLoader(lib, rt, "10.0");
  registerPythonLoader(lib, rt, "3.11");
  registerLuaLoader(lib, rt);
  registerJsLoader(lib, rt);

  console.log("\n=== polyplug js-deno host example ===");

  // Find plugins using generated contract IDs
  const decoderHandle = rt.findByContract(ContractIds.PIPELINE_DECODER_CONTRACT_ID, 0);
  if (decoderHandle !== NULL_HANDLE) {
    console.log("[js_deno_decoder] found decoder plugin");
  }

  const transformerHandle = rt.findByContract(ContractIds.DATA_TRANSFORMER_CONTRACT_ID, 0);
  if (transformerHandle !== NULL_HANDLE) {
    console.log("[js_deno_transformer] found transformer plugin");
  }

  const encoderHandle = rt.findByContract(ContractIds.PIPELINE_ENCODER_CONTRACT_ID, 0);
  if (encoderHandle !== NULL_HANDLE) {
    console.log("[js_deno_encoder] found encoder plugin");
  }

  const reporterHandle = rt.findByContract(ContractIds.DATA_REPORTER_CONTRACT_ID, 0);
  if (reporterHandle !== NULL_HANDLE) {
    console.log("[js_deno_reporter] found reporter plugin");
  }

  const validatorHandle = rt.findByContract(ContractIds.PIPELINE_VALIDATOR_CONTRACT_ID, 0);
  if (validatorHandle !== NULL_HANDLE) {
    console.log("[js_deno_validator] found validator plugin");
  }

  console.log("\n=== done ===");
}

await main();
