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

interface DiscoveredBundle {
  path: string;
  bundleName: string;
  provides: string[];
}

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

function resolvePluginPath(): string {
  const envPath: string | undefined = Deno.env.get("POLYPLUG_PLUGIN_PATH");
  if (envPath && envPath.length > 0) {
    return envPath;
  }
  const repoRoot: string = new URL("../../..", import.meta.url).pathname;
  return `${repoRoot}examples/plugins`;
}

function scanPluginDir(dir: string): DiscoveredBundle[] {
  const bundles: DiscoveredBundle[] = [];

  try {
    for (const entry of Deno.readDirSync(dir)) {
      if (!entry.isDirectory) continue;

      const manifestPath: string = `${dir}/${entry.name}/manifest.toml`;
      let content: string;
      try {
        content = Deno.readTextFileSync(manifestPath);
      } catch {
        continue;
      }

      const bnMatch: RegExpMatchArray | null = content.match(/bundle_name\s*=\s*"([^"]+)"/);
      if (!bnMatch) continue;

      const provides: string[] = [];
      const provMatch: RegExpMatchArray | null = content.match(/provides\s*=\s*\[([^\]]+)\]/);
      if (provMatch) {
        const items: RegExpMatchArray | null = provMatch[1].match(/"([^"]+)"/g);
        if (items) {
          for (const item of items) {
            provides.push(item.replace(/"/g, ""));
          }
        }
      }

      bundles.push({
        path: `${dir}/${entry.name}`,
        bundleName: bnMatch[1],
        provides,
      });
    }
  } catch {
    return [];
  }

  bundles.sort((a, b) => a.bundleName.localeCompare(b.bundleName));
  return bundles;
}

const REGISTER_SYMBOLS = {
  polyplug_runtime_register_loader: {
    parameters: ["pointer", "pointer"] as const,
    result: "u32" as const,
  },
} as const satisfies Deno.ForeignLibraryInterface;

function main(): void {
  const pluginDir: string = resolvePluginPath();
  const soPath: string = Deno.env.get("POLYPLUG_SO") ??
    `${new URL("../../..", import.meta.url).pathname}target/debug/libpolyplug.so`;

  console.error(`plugin directory: ${pluginDir}`);

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

      const bundles: DiscoveredBundle[] = scanPluginDir(pluginDir);
      if (bundles.length === 0) {
        throw new Error(`no plugins found in ${pluginDir}. Run examples/build_all.sh first.`);
      }

      console.error(`discovered ${bundles.length} bundles`);

      for (const b of bundles) {
        rt.loadBundle(b.path);
        console.error(`  loaded: ${b.bundleName}`);
      }

      for (const b of bundles) {
        let contractId: bigint = 0n;
        let fnName: string = "";

        if (b.provides.includes("data.Transformer")) {
          contractId = TRANSFORMER_CONTRACT_ID;
          fnName = "transform";
        } else if (b.provides.includes("data.Reporter")) {
          contractId = REPORTER_CONTRACT_ID;
          fnName = "report";
        } else {
          continue;
        }

        const bid: bigint = bundleId(b.bundleName);
        const handle: bigint = rt.findByBundle(bid, contractId);
        if (handle === NULL_HANDLE) {
          throw new Error(`plugin not found: ${b.bundleName}`);
        }

        const guard: Guard = rt.resolvePlugin(handle);
        try {
          const vtable: Deno.PointerValue = guard.vtable();
          if (vtable === null) {
            throw new Error(`null vtable: ${b.bundleName}`);
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
            throw new Error(`call failed for ${b.bundleName}: code ${errCode}`);
          }

          const result: string = readStringViewAt(outputSvView, 0);
          const label: string = `[${b.bundleName}]`;
          console.log(`${label.padEnd(30)} ${fnName}("hello") = "${result}"`);
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
