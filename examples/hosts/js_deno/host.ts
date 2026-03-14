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
import { registerJsDenoLoader } from "../../../host-libs/js/loaders/js_deno.ts";

const TRANSFORMER_CONTRACT_ID: bigint = 0x3D53C682F3F5A9EFn;
const REPORTER_CONTRACT_ID: bigint = 0x81D41D43E511D297n;

const FNV_OFFSET: bigint = 0xCBF29CE484222325n;
const FNV_PRIME: bigint = 0x00000100000001B3n;

const SZ_STRING_VIEW: number = 16;

const ABI_FN_RESULT_STRUCT = { struct: ["u32", "u32", "pointer", "usize"] } as const;

interface GuestSpec {
  dir: string;
  bundleName: string;
  contractId: bigint;
  fnName: string;
}

const GUESTS: GuestSpec[] = [
  { dir: "rust/decoder",           bundleName: "rust_transformer",       contractId: TRANSFORMER_CONTRACT_ID, fnName: "transform" },
  { dir: "rust/reporter",          bundleName: "rust_reporter",          contractId: REPORTER_CONTRACT_ID,    fnName: "report" },
  { dir: "cpp/transformer",        bundleName: "cpp_transformer",        contractId: TRANSFORMER_CONTRACT_ID, fnName: "transform" },
  { dir: "cpp/reporter",           bundleName: "cpp_reporter",           contractId: REPORTER_CONTRACT_ID,    fnName: "report" },
  { dir: "csharp/encoder",         bundleName: "csharp_transformer",     contractId: TRANSFORMER_CONTRACT_ID, fnName: "transform" },
  { dir: "csharp/reporter",        bundleName: "csharp_reporter",        contractId: REPORTER_CONTRACT_ID,    fnName: "report" },
  { dir: "python/decoder",         bundleName: "python_transformer",     contractId: TRANSFORMER_CONTRACT_ID, fnName: "transform" },
  { dir: "python/reporter",        bundleName: "python_reporter",        contractId: REPORTER_CONTRACT_ID,    fnName: "report" },
  { dir: "lua/transformer",        bundleName: "lua_transformer",        contractId: TRANSFORMER_CONTRACT_ID, fnName: "transform" },
  { dir: "lua/reporter",           bundleName: "lua_reporter",           contractId: REPORTER_CONTRACT_ID,    fnName: "report" },
  { dir: "js_quickjs/transformer", bundleName: "js_quickjs_transformer", contractId: TRANSFORMER_CONTRACT_ID, fnName: "transform" },
  { dir: "js_quickjs/reporter",    bundleName: "js_quickjs_reporter",    contractId: REPORTER_CONTRACT_ID,    fnName: "report" },
  { dir: "js_deno/transformer",    bundleName: "js_deno_transformer",    contractId: TRANSFORMER_CONTRACT_ID, fnName: "transform" },
  { dir: "js_deno/reporter",       bundleName: "js_deno_reporter",       contractId: REPORTER_CONTRACT_ID,    fnName: "report" },
];

function bundleId(name: string): bigint {
  const data: Uint8Array = new TextEncoder().encode(name);
  let hash: bigint = FNV_OFFSET;
  for (const byte of data) {
    hash ^= BigInt(byte);
    hash = BigInt.asUintN(64, hash * FNV_PRIME);
  }
  return hash;
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
  // deno-lint-ignore no-explicit-any
  const fnCall = new Deno.UnsafeFnPointer(fnPtr as any, fnDef);
  const result = fnCall.call(argsPtr, outPtr) as unknown as [number, number, Deno.PointerValue, number | bigint];
  return result[0];
}

const REGISTER_SYMBOLS = {
  polyplug_runtime_register_loader: {
    parameters: ["pointer", "pointer"] as const,
    result: "u32" as const,
  },
} as const satisfies Deno.ForeignLibraryInterface;

function main(): void {
  const repoRoot: string = new URL("../../..", import.meta.url).pathname;
  const soPath: string = Deno.env.get("POLYPLUG_SO") ??
    `${repoRoot}target/debug/libpolyplug.so`;

  const lib = openPolyplug(soPath);
  const registerLib = Deno.dlopen(soPath, REGISTER_SYMBOLS);

  try {
    const rt: Runtime = runtimeNew(lib);
    try {
      const rtPtr: Deno.PointerValue = rt.ptr();
      const registerFn = (rtP: Deno.PointerValue, loaderP: Deno.PointerValue): number => {
        return registerLib.symbols.polyplug_runtime_register_loader(rtP, loaderP);
      };

      registerNativeLoader(rtPtr, registerFn);
      registerDotnetLoader(rtPtr, registerFn);
      registerPythonLoader(rtPtr, registerFn);
      registerLuaLoader(rtPtr, registerFn);
      registerJsLoader(rtPtr, registerFn);
      registerJsDenoLoader(rtPtr, registerFn);

      for (const g of GUESTS) {
        rt.loadBundle(`${repoRoot}examples/guests/${g.dir}`);
      }

      for (const g of GUESTS) {
        const bid: bigint = bundleId(g.bundleName);
        const handle: bigint = rt.findByBundle(bid, g.contractId);
        if (handle === NULL_HANDLE) {
          throw new Error(`plugin not found: ${g.bundleName}`);
        }

        const guard: Guard = rt.resolvePlugin(handle);
        try {
          const vtable: Deno.PointerValue = guard.vtable();
          if (vtable === null) {
            throw new Error(`null vtable: ${g.bundleName}`);
          }

          const inputStr: string = "hello";
          const inputBytes: Uint8Array<ArrayBuffer> = new TextEncoder().encode(inputStr);
          const inputSvBuf: Uint8Array<ArrayBuffer> = new Uint8Array(new ArrayBuffer(SZ_STRING_VIEW));
          const inputSvView: DataView = new DataView(inputSvBuf.buffer);
          const inputDataPtr: Deno.PointerValue = Deno.UnsafePointer.of(inputBytes);
          inputSvView.setBigUint64(0, BigInt(Deno.UnsafePointer.value(inputDataPtr)), true);
          inputSvView.setBigUint64(8, BigInt(inputBytes.length), true);

          const outputSvBuf: Uint8Array<ArrayBuffer> = new Uint8Array(new ArrayBuffer(SZ_STRING_VIEW));
          const outputSvView: DataView = new DataView(outputSvBuf.buffer);

          const errCode: number = callVtableFn(
            vtable,
            Deno.UnsafePointer.of(inputSvBuf),
            Deno.UnsafePointer.of(outputSvBuf),
          );
          if (errCode !== 0) {
            throw new Error(`call failed for ${g.dir}: code ${errCode}`);
          }

          const result: string = readStringViewAt(outputSvView, 0);
          const label: string = `[${g.dir}]`;
          console.log(`${label.padEnd(30)} ${g.fnName}("hello") = "${result}"`);
        } finally {
          guard[Symbol.dispose]();
        }
      }
    } finally {
      rt[Symbol.dispose]();
    }
  } finally {
    registerLib.close();
    lib.close();
  }
}

main();
