import {
  openPolyplug,
  runtimeNew,
  registerDotnetLoader,
  registerJsLoader,
  registerLuaLoader,
  registerNativeLoader,
  registerPythonLoader,
  NULL_HANDLE,
} from "../../../host-libs/js/polyplug.ts";

const DECODER_CONTRACT_ID: bigint = 0x133E62ABD6E7D5BEn;
const TRANSFORMER_CONTRACT_ID: bigint = 0x0E3044133E12EB05n;
const ENCODER_CONTRACT_ID: bigint = 0x12AD37F43386F752n;
const REPORTER_CONTRACT_ID: bigint = 0xD50E539CAE219A15n;
const VALIDATOR_CONTRACT_ID: bigint = 0x027ABCEBF8020D90n;

const FNV_OFFSET: bigint = 0xCBF29CE484222325n;
const FNV_PRIME: bigint = 0x00000100000001B3n;

const SZ_STRING_VIEW: number = 16;
const SZ_DATA_RECORD: number = 40;
const SZ_BUFFER: number = 24;
const SZ_VALIDATION_RESULT: number = 24;

const ABI_FN_RESULT_STRUCT = { struct: ["u32", "u32", "pointer", "usize"] } as const;

function readPtr(view: DataView, offset: number): bigint {
  return view.getBigUint64(offset, true);
}

function writeU64(view: DataView, offset: number, value: bigint): void {
  view.setBigUint64(offset, value, true);
}

function ptrToBigInt(ptr: Deno.PointerValue): bigint {
  if (ptr === null) return 0n;
  return Deno.UnsafePointer.value(ptr);
}

function callVtableFn(
  vtablePtr: Deno.PointerValue,
  fnId: number,
  argsPtr: Deno.PointerValue,
  outPtr: Deno.PointerValue,
): number {
  if (vtablePtr === null) {
    throw new Error("null vtablePtr");
  }

  const vtableBuf: Deno.UnsafePointerView = new Deno.UnsafePointerView(vtablePtr);
  const functionsPtrVal: bigint = vtableBuf.getBigUint64(16);
  if (functionsPtrVal === 0n) {
    throw new Error("vtable.functions is null");
  }

  const functionsPtr: Deno.PointerValue = Deno.UnsafePointer.create(functionsPtrVal);
  const fnArrayView: Deno.UnsafePointerView = new Deno.UnsafePointerView(functionsPtr!);
  const fnPtrVal: bigint = fnArrayView.getBigUint64(fnId * 8);
  if (fnPtrVal === 0n) {
    throw new Error(`fn[${fnId}] is null`);
  }

  const fnPtr: Deno.PointerValue = Deno.UnsafePointer.create(fnPtrVal);
  const fnCall = new Deno.UnsafeFnPointer(fnPtr!, {
    parameters: ["pointer", "pointer"] as const,
    result: ABI_FN_RESULT_STRUCT,
  });
  const result = fnCall.call(argsPtr, outPtr) as [number, number, Deno.PointerValue, number | bigint];
  return result[0];
}

function makeDataRecord(): [Uint8Array, DataView] {
  const buf: Uint8Array = new Uint8Array(SZ_DATA_RECORD);
  const view: DataView = new DataView(buf.buffer);
  return [buf, view];
}

function makeInputBuffer(data: Uint8Array): [Uint8Array, DataView] {
  const buf: Uint8Array = new Uint8Array(SZ_BUFFER);
  const view: DataView = new DataView(buf.buffer);
  const dataPtr: Deno.PointerValue = Deno.UnsafePointer.of(data);
  writeU64(view, 0, ptrToBigInt(dataPtr));
  writeU64(view, 8, BigInt(data.length));
  writeU64(view, 16, BigInt(data.length));
  return [buf, view];
}

function makeOutputBuffer(): [Uint8Array, DataView] {
  const buf: Uint8Array = new Uint8Array(SZ_BUFFER);
  const view: DataView = new DataView(buf.buffer);
  return [buf, view];
}

function makeStringView(): [Uint8Array, DataView] {
  const buf: Uint8Array = new Uint8Array(SZ_STRING_VIEW);
  const view: DataView = new DataView(buf.buffer);
  return [buf, view];
}

function makeValidationResult(): [Uint8Array, DataView] {
  const buf: Uint8Array = new Uint8Array(SZ_VALIDATION_RESULT);
  const view: DataView = new DataView(buf.buffer);
  return [buf, view];
}

function readStringViewAt(view: DataView, offset: number): string {
  const ptrVal: bigint = readPtr(view, offset);
  const len: number = Number(readPtr(view, offset + 8));
  if (ptrVal === 0n || len === 0) return "";
  const ptr: Deno.PointerValue = Deno.UnsafePointer.create(ptrVal);
  const ptrView: Deno.UnsafePointerView = new Deno.UnsafePointerView(ptr!);
  const bytes: Uint8Array = new Uint8Array(len);
  ptrView.copyInto(bytes, 0);
  return new TextDecoder().decode(bytes);
}

function readBufferView(view: DataView): string {
  const ptrVal: bigint = readPtr(view, 0);
  const len: number = Number(readPtr(view, 8));
  if (ptrVal === 0n || len === 0) return "";
  const ptr: Deno.PointerValue = Deno.UnsafePointer.create(ptrVal);
  const ptrView: Deno.UnsafePointerView = new Deno.UnsafePointerView(ptr!);
  const bytes: Uint8Array = new Uint8Array(len);
  ptrView.copyInto(bytes, 0);
  return new TextDecoder().decode(bytes);
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

type PluginGuard = ReturnType<ReturnType<typeof runtimeNew>["resolvePlugin"]>;

type PluginEntry = {
  name: string;
  guard: PluginGuard;
  vtable: Deno.PointerValue;
};

function resolveByBundle(rt: ReturnType<typeof runtimeNew>, bundleName: string, contractId: bigint): PluginEntry {
  const bundle: bigint = bundleId(bundleName);
  const handle: bigint = rt.findByBundle(bundle, contractId);
  if (handle === NULL_HANDLE) {
    throw new Error(`plugin not found for bundle: ${bundleName}`);
  }
  const guard = rt.resolvePlugin(handle);
  const vtable: Deno.PointerValue = guard.vtable();
  if (vtable === null) {
    guard[Symbol.dispose]();
    throw new Error(`null vtable for bundle: ${bundleName}`);
  }
  return { name: bundleName, guard, vtable };
}

function runPipeline(
  label: string,
  decoder: PluginEntry,
  transformer: PluginEntry,
  encoder: PluginEntry,
  reporter: PluginEntry,
  validator: PluginEntry,
  inputCsv: string,
): void {
  console.log(`--- ${label} ---`);

  const inputBytes: Uint8Array = new TextEncoder().encode(inputCsv);
  const [inputBuf] = makeInputBuffer(inputBytes);
  const [recordBuf] = makeDataRecord();
  const decodeCode: number = callVtableFn(
    decoder.vtable,
    0,
    Deno.UnsafePointer.of(inputBuf),
    Deno.UnsafePointer.of(recordBuf),
  );
  if (decodeCode !== 0) {
    throw new Error(`decode failed: code ${decodeCode}`);
  }

  const [transformedBuf] = makeDataRecord();
  const transformCode: number = callVtableFn(
    transformer.vtable,
    0,
    Deno.UnsafePointer.of(recordBuf),
    Deno.UnsafePointer.of(transformedBuf),
  );
  if (transformCode !== 0) {
    throw new Error(`transform failed: code ${transformCode}`);
  }

  const [encodedBuf, encodedView] = makeOutputBuffer();
  const encodeCode: number = callVtableFn(
    encoder.vtable,
    0,
    Deno.UnsafePointer.of(transformedBuf),
    Deno.UnsafePointer.of(encodedBuf),
  );
  if (encodeCode !== 0) {
    throw new Error(`encode failed: code ${encodeCode}`);
  }

  const output: string = readBufferView(encodedView).trimEnd();
  console.log(`Run output: ${output}`);

  const [reportSvBuf, reportSvView] = makeStringView();
  const reportCode: number = callVtableFn(
    reporter.vtable,
    0,
    Deno.UnsafePointer.of(transformedBuf),
    Deno.UnsafePointer.of(reportSvBuf),
  );
  if (reportCode !== 0) {
    throw new Error(`report failed: code ${reportCode}`);
  }

  const report: string = readStringViewAt(reportSvView, 0);
  if (report.trim().length > 0) {
    console.log(`Run summary: ${report}`);
  }

  const [validationBuf, validationView] = makeValidationResult();
  const validateCode: number = callVtableFn(
    validator.vtable,
    0,
    Deno.UnsafePointer.of(transformedBuf),
    Deno.UnsafePointer.of(validationBuf),
  );
  if (validateCode !== 0) {
    throw new Error(`validate failed: code ${validateCode}`);
  }

  const validByte: number = validationView.getUint8(0);
  const reason: string = readStringViewAt(validationView, 8);
  const status: string = validByte === 0 ? "invalid" : "ok";
  console.log(`Validation: ${status} (${reason})`);
}

function buildBundlePaths(repoRoot: string): string[] {
  return [
    `${repoRoot}examples/guests/rust/decoder`,
    `${repoRoot}examples/guests/rust/encoder`,
    `${repoRoot}examples/guests/cpp/transformer`,
    `${repoRoot}examples/guests/cpp/validator`,
    `${repoRoot}examples/guests/csharp/encoder`,
    `${repoRoot}examples/guests/csharp/reporter`,
    `${repoRoot}examples/guests/python/decoder`,
    `${repoRoot}examples/guests/python/reporter`,
    `${repoRoot}examples/guests/lua/transformer`,
    `${repoRoot}examples/guests/lua/validator`,
    `${repoRoot}examples/guests/js/validator`,
    `${repoRoot}examples/guests/js/reporter`,
  ];
}

function loadAllBundles(rt: ReturnType<typeof runtimeNew>, bundles: string[]): void {
  console.log("Loading 12 guest plugins...");
  let index: number = 0;
  for (const path of bundles) {
    index += 1;
    rt.loadBundle(path);
    const parts: string[] = path.split("/");
    const lang: string = parts[parts.length - 2] ?? path;
    const name: string = parts[parts.length - 1] ?? path;
    const label: string = `${lang}/${name}`;
    const paddedIndex: string = index.toString().padStart(2, " ");
    console.log(`  [OK]  ${paddedIndex}/12 ${label}`);
  }
}

function resolvePlugins(rt: ReturnType<typeof runtimeNew>): Record<string, PluginEntry> {
  return {
    decoder_rust: resolveByBundle(rt, "csv_decoder", DECODER_CONTRACT_ID),
    encoder_rust: resolveByBundle(rt, "csv_encoder_rust", ENCODER_CONTRACT_ID),
    transformer_cpp: resolveByBundle(rt, "uppercase_transformer", TRANSFORMER_CONTRACT_ID),
    validator_cpp: resolveByBundle(rt, "cpp_validator", VALIDATOR_CONTRACT_ID),
    encoder_csharp: resolveByBundle(rt, "csv_encoder_csharp", ENCODER_CONTRACT_ID),
    reporter_csharp: resolveByBundle(rt, "csharp_reporter", REPORTER_CONTRACT_ID),
    decoder_python: resolveByBundle(rt, "python_decoder", DECODER_CONTRACT_ID),
    reporter_python: resolveByBundle(rt, "summary_reporter", REPORTER_CONTRACT_ID),
    transformer_lua: resolveByBundle(rt, "reverse_transformer", TRANSFORMER_CONTRACT_ID),
    validator_lua: resolveByBundle(rt, "lua_validator", VALIDATOR_CONTRACT_ID),
    validator_js: resolveByBundle(rt, "field_validator", VALIDATOR_CONTRACT_ID),
    reporter_js: resolveByBundle(rt, "js_reporter", REPORTER_CONTRACT_ID),
  };
}

function main(): void {
  const repoRoot: string = new URL("../../..", import.meta.url).pathname;
  const soPath: string = Deno.env.get("POLYPLUG_SO") ??
    `${repoRoot}examples/hosts/js/target/debug/libpolyplug_full.so`;

  console.log("=== polyplug C# host example ===");
  const lib: Deno.DynamicLibrary<Deno.ForeignLibraryInterface> = openPolyplug(soPath);
  try {
    const rt = runtimeNew(lib);
    try {
      registerNativeLoader(lib, rt.ptr());
      registerDotnetLoader(lib, rt.ptr());
      registerPythonLoader(lib, rt.ptr());
      registerLuaLoader(lib, rt.ptr());
      registerJsLoader(lib, rt.ptr());

      const bundles: string[] = buildBundlePaths(repoRoot);
      loadAllBundles(rt, bundles);

      const plugins: Record<string, PluginEntry> = resolvePlugins(rt);
      try {
        runPipeline(
          "Run 1: Rust decoder, C++ transformer, Rust encoder, C# reporter, C++ validator",
          plugins.decoder_rust,
          plugins.transformer_cpp,
          plugins.encoder_rust,
          plugins.reporter_csharp,
          plugins.validator_cpp,
          "Alice,hello,3\n",
        );

        runPipeline(
          "Run 2: Python decoder, Lua transformer, C# encoder, Python reporter, Lua validator",
          plugins.decoder_python,
          plugins.transformer_lua,
          plugins.encoder_csharp,
          plugins.reporter_python,
          plugins.validator_lua,
          "Bob,world,4\n",
        );

        runPipeline(
          "Run 3: Rust decoder, C++ transformer, C# encoder, JS reporter, JS validator",
          plugins.decoder_rust,
          plugins.transformer_cpp,
          plugins.encoder_csharp,
          plugins.reporter_js,
          plugins.validator_js,
          "Cara,polyplug,5\n",
        );
      } finally {
        for (const entry of Object.values(plugins)) {
          entry.guard[Symbol.dispose]();
        }
      }
    } finally {
      rt[Symbol.dispose]();
    }
  } finally {
    lib.close();
  }

  console.log("pipeline complete");
}

main();
