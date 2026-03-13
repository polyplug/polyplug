// examples/hosts/js/host.ts
// JavaScript/Deno host for polyplug — loads all 12 guest plugins and runs pipeline.
//
// Run from the repository root:
//   deno run --allow-ffi --allow-env --allow-read --unstable-ffi examples/hosts/js/host.ts
//
// Environment variables:
//   POLYPLUG_SO   — path to libpolyplug.so (default: target/debug/libpolyplug.so)

import { openPolyplug, runtimeNew, NULL_HANDLE } from "../../../host-libs/js/polyplug.ts";

// ─── Contract IDs (FNV-1a-64 hashes) ─────────────────────────────────────────
const DECODER_CONTRACT_ID: bigint     = 0x133E62ABD6E7D5BEn;
const TRANSFORMER_CONTRACT_ID: bigint = 0x0E3044133E12EB05n;
const ENCODER_CONTRACT_ID: bigint     = 0x12AD37F43386F752n;
const REPORTER_CONTRACT_ID: bigint    = 0xD50E539CAE219A15n;
const VALIDATOR_CONTRACT_ID: bigint   = 0x027ABCEBF8020D90n;

// ─── ABI Type Sizes and Offsets ───────────────────────────────────────────────
// All sizes are for 64-bit systems. Layouts mirror abi_types.md exactly.
//
// StringView: ptr(8) + len(8) = 16 bytes
// DataRecord: name(16) + value(16) + count(4) + _pad(4) = 40 bytes
// Buffer:     ptr(8)  + len(8) + cap(8) = 24 bytes
// AbiError:   code(4) + pad(4) + message.ptr(8) + message.len(8) = 24 bytes
// PluginVTable: contract_id(8) + contract_version(4) + function_count(4) + functions*(8) = 24 bytes

const SZ_STRING_VIEW: number = 16;
const SZ_DATA_RECORD: number = 40;
const SZ_BUFFER: number      = 24;
const SZ_ABI_ERROR: number   = 24;

// ─── Pointer helpers ──────────────────────────────────────────────────────────

// Read a native-pointer-sized value (8 bytes on 64-bit) from a DataView at offset.
function readPtr(view: DataView, offset: number): bigint {
  return view.getBigUint64(offset, true);
}

// Read a u32 from a DataView at offset.
function readU32(view: DataView, offset: number): number {
  return view.getUint32(offset, true);
}

// Write a u64 into a DataView at offset.
function writeU64(view: DataView, offset: number, value: bigint): void {
  view.setBigUint64(offset, value, true);
}

// Write a u32 into a DataView at offset.
function writeU32(view: DataView, offset: number, value: number): void {
  view.setUint32(offset, value, true);
}

// Convert a Deno.PointerValue to bigint (null → 0n).
function ptrToBigInt(ptr: Deno.PointerValue): bigint {
  if (ptr === null) return 0n;
  return Deno.UnsafePointer.value(ptr);
}

// ─── Vtable Call Helpers ──────────────────────────────────────────────────────
// ABI function signature: (args: *const (), out: *mut ()) -> AbiError (by value)
// AbiError = { code: u32, _pad: u32, message_ptr: pointer, message_len: usize }
// Returned as struct: ["u32", "u32", "pointer", "usize"]

const ABI_FN_RESULT_STRUCT = { struct: ["u32", "u32", "pointer", "usize"] } as const;

// Call fn_id-th function from vtable pointer. Returns ABI error code (0 = success).
function callVtableFn(
  vtablePtr: Deno.PointerValue,
  fnId: number,
  argsPtr: Deno.PointerValue,
  outPtr: Deno.PointerValue,
): number {
  if (vtablePtr === null) throw new Error("null vtablePtr");

  // Read `functions` pointer from PluginVTable at offset 16.
  const vtableBuf: Deno.UnsafePointerView = new Deno.UnsafePointerView(vtablePtr);
  const functionsPtrVal: bigint = vtableBuf.getBigUint64(16);
  if (functionsPtrVal === 0n) throw new Error("vtable.functions is null");
  const functionsPtr: Deno.PointerValue = Deno.UnsafePointer.create(functionsPtrVal);

  // Read function pointer at index fnId (each pointer is 8 bytes).
  const fnArrayView: Deno.UnsafePointerView = new Deno.UnsafePointerView(functionsPtr!);
  const fnPtrVal: bigint = fnArrayView.getBigUint64(fnId * 8);
  if (fnPtrVal === 0n) throw new Error(`fn[${fnId}] is null`);
  const fnPtr: Deno.PointerValue = Deno.UnsafePointer.create(fnPtrVal);

  // Call the function. Signature: (args: pointer, out: pointer) -> struct AbiError.
  const fn_call = new Deno.UnsafeFnPointer(fnPtr!, {
    parameters: ["pointer", "pointer"] as const,
    result: ABI_FN_RESULT_STRUCT,
  });

  const result = fn_call.call(argsPtr, outPtr) as [number, number, Deno.PointerValue, number | bigint];
  return result[0]; // code field (u32)
}

// ─── Pipeline Helper: allocate structs ───────────────────────────────────────

// Create a zeroed DataRecord buffer and return [buf, view].
function makeDataRecord(): [Uint8Array, DataView] {
  const buf: Uint8Array = new Uint8Array(SZ_DATA_RECORD);
  return [buf, new DataView(buf.buffer)];
}

// Create a Buffer struct pointing to a byte array.
function makeInputBuffer(data: Uint8Array): [Uint8Array, DataView] {
  const buf: Uint8Array = new Uint8Array(SZ_BUFFER);
  const view: DataView = new DataView(buf.buffer);
  const dataPtr: Deno.PointerValue = Deno.UnsafePointer.of(data);
  writeU64(view, 0, ptrToBigInt(dataPtr));   // ptr
  writeU64(view, 8, BigInt(data.length));    // len
  writeU64(view, 16, BigInt(data.length));   // cap
  return [buf, view];
}

// Create a zeroed Buffer struct (for encoder output).
function makeOutputBuffer(): [Uint8Array, DataView] {
  const buf: Uint8Array = new Uint8Array(SZ_BUFFER);
  return [buf, new DataView(buf.buffer)];
}

// Create a zeroed StringView buffer (for reporter output).
function makeStringView(): [Uint8Array, DataView] {
  const buf: Uint8Array = new Uint8Array(SZ_STRING_VIEW);
  return [buf, new DataView(buf.buffer)];
}

// Read a UTF-8 string from ptr+len stored in a DataView at a given offset.
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

// ─── Pipeline ─────────────────────────────────────────────────────────────────

function runPipeline(
  decoderVt: Deno.PointerValue,
  validatorVt: Deno.PointerValue,
  transformerVt: Deno.PointerValue,
  encoderVt: Deno.PointerValue,
  reporterVt: Deno.PointerValue,
  inputCsv: Uint8Array,
  label: string,
): void {
  console.log(`--- ${label} ---`);

  // Step 1: Decode
  const [inputBuf] = makeInputBuffer(inputCsv);
  const [recordBuf, recordView] = makeDataRecord();
  const decodeCode: number = callVtableFn(
    decoderVt,
    0,
    Deno.UnsafePointer.of(inputBuf),
    Deno.UnsafePointer.of(recordBuf),
  );
  if (decodeCode !== 0) throw new Error(`decode failed: code ${decodeCode}`);

  // Step 2: Validate
  const [voidBuf] = [new Uint8Array(8)];
  callVtableFn(
    validatorVt,
    0,
    Deno.UnsafePointer.of(recordBuf),
    Deno.UnsafePointer.of(voidBuf),
  );

  // Step 3: Transform
  const [transformedBuf] = makeDataRecord();
  callVtableFn(
    transformerVt,
    0,
    Deno.UnsafePointer.of(recordBuf),
    Deno.UnsafePointer.of(transformedBuf),
  );

  // Step 4: Encode
  const [encodedBuf, encodedView] = makeOutputBuffer();
  const encodeCode: number = callVtableFn(
    encoderVt,
    0,
    Deno.UnsafePointer.of(transformedBuf),
    Deno.UnsafePointer.of(encodedBuf),
  );
  if (encodeCode !== 0) throw new Error(`encode failed: code ${encodeCode}`);

  // Read encoded output
  const encPtrVal: bigint = readPtr(encodedView, 0);
  const encLen: number = Number(readPtr(encodedView, 8));
  if (encPtrVal !== 0n && encLen > 0) {
    const encPtr: Deno.PointerValue = Deno.UnsafePointer.create(encPtrVal);
    const encView: Deno.UnsafePointerView = new Deno.UnsafePointerView(encPtr!);
    const encBytes: Uint8Array = new Uint8Array(encLen);
    encView.copyInto(encBytes, 0);
    const output: string = new TextDecoder().decode(encBytes).trimEnd();
    console.log(`Run output: ${output}`);
  }

  // Step 5: Report
  const [reportSvBuf, reportSvView] = makeStringView();
  const reportCode: number = callVtableFn(
    reporterVt,
    0,
    Deno.UnsafePointer.of(transformedBuf),
    Deno.UnsafePointer.of(reportSvBuf),
  );
  if (reportCode !== 0) throw new Error(`report failed: code ${reportCode}`);

  const summary: string = readStringViewAt(reportSvView, 0);
  if (summary.length > 0) {
    console.log(`Run summary: ${summary}`);
  }
}

// ─── Error Scenario ───────────────────────────────────────────────────────────

function runErrorScenario(decoderVt: Deno.PointerValue): void {
  console.log("--- Error scenario: malformed input ---");
  const badInput: Uint8Array = new TextEncoder().encode("INVALID\n");
  const [inputBuf] = makeInputBuffer(badInput);
  const [recordBuf] = makeDataRecord();
  const code: number = callVtableFn(
    decoderVt,
    0,
    Deno.UnsafePointer.of(inputBuf),
    Deno.UnsafePointer.of(recordBuf),
  );
  if (code !== 0) {
    console.log(`Error: decode failed (code ${code})`);
  }
}

// ─── Main ─────────────────────────────────────────────────────────────────────

function main(): void {
  console.log("=== polyplug JS host ===");

  // Resolve paths relative to this script's directory.
  const scriptDir: string = new URL(".", import.meta.url).pathname;
  const repoRoot: string = new URL("../../..", import.meta.url).pathname;

  const soPath: string = Deno.env.get("POLYPLUG_SO") ??
    `${repoRoot}examples/hosts/js/target/debug/libpolyplug_full.so`;

  // Bundle paths — absolute paths to each guest bundle directory.
  const guestPaths: string[] = [
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

  console.log(`Loading libpolyplug from: ${soPath}`);

  const lib: Deno.DynamicLibrary<Deno.ForeignLibraryInterface> = openPolyplug(soPath);
  try {
    const rt = runtimeNew(lib);
    try {
      // Load all 12 guest bundles — some may fail if the runtime lacks a loader.
      console.log("\nLoading 12 guest plugins...");
      let loadedCount: number = 0;
      for (const guestPath of guestPaths) {
        try {
          rt.loadBundle(guestPath);
          console.log(`  [OK]   ${guestPath.split("/").slice(-3).join("/")}`);
          loadedCount++;
        } catch (err: unknown) {
          const msg: string = err instanceof Error ? err.message : String(err);
          console.log(`  [SKIP] ${guestPath.split("/").slice(-3).join("/")}: ${msg}`);
        }
      }
      console.log(`${loadedCount}/12 guests loaded.\n`);

      // Find plugins by contract.
      const decoderHandle: bigint    = rt.findByContract(DECODER_CONTRACT_ID);
      const validatorHandle: bigint  = rt.findByContract(VALIDATOR_CONTRACT_ID);
      const transformerHandle: bigint = rt.findByContract(TRANSFORMER_CONTRACT_ID);
      const encoderHandle: bigint    = rt.findByContract(ENCODER_CONTRACT_ID);
      const reporterHandle: bigint   = rt.findByContract(REPORTER_CONTRACT_ID);

      if (decoderHandle === NULL_HANDLE)     throw new Error("decoder plugin not found");
      if (validatorHandle === NULL_HANDLE)   throw new Error("validator plugin not found");
      if (transformerHandle === NULL_HANDLE) throw new Error("transformer plugin not found");
      if (encoderHandle === NULL_HANDLE)     throw new Error("encoder plugin not found");
      if (reporterHandle === NULL_HANDLE)    throw new Error("reporter plugin not found");

      // Resolve guards and get vtables.
      const decoderGuard    = rt.resolvePlugin(decoderHandle);
      const validatorGuard  = rt.resolvePlugin(validatorHandle);
      const transformerGuard = rt.resolvePlugin(transformerHandle);
      const encoderGuard    = rt.resolvePlugin(encoderHandle);
      const reporterGuard   = rt.resolvePlugin(reporterHandle);

      try {
        const decoderVt: Deno.PointerValue    = decoderGuard.vtable();
        const validatorVt: Deno.PointerValue  = validatorGuard.vtable();
        const transformerVt: Deno.PointerValue = transformerGuard.vtable();
        const encoderVt: Deno.PointerValue    = encoderGuard.vtable();
        const reporterVt: Deno.PointerValue   = reporterGuard.vtable();

        if (decoderVt === null)    throw new Error("decoder vtable is null");
        if (validatorVt === null)  throw new Error("validator vtable is null");
        if (transformerVt === null) throw new Error("transformer vtable is null");
        if (encoderVt === null)    throw new Error("encoder vtable is null");
        if (reporterVt === null)   throw new Error("reporter vtable is null");

        // Run pipeline using first-found plugins (one per contract).
        const inputCsv: Uint8Array = new TextEncoder().encode("Alice,hello,3\n");

        runPipeline(
          decoderVt,
          validatorVt,
          transformerVt,
          encoderVt,
          reporterVt,
          inputCsv,
          "Run 1: first-found plugins per contract",
        );

        // Show all found handles per contract to demonstrate all 12 are loaded.
        console.log("\n--- All loaded plugins per contract ---");
        const allDecoders: bigint[]    = rt.findAllByContract(DECODER_CONTRACT_ID);
        const allValidators: bigint[]  = rt.findAllByContract(VALIDATOR_CONTRACT_ID);
        const allTransformers: bigint[] = rt.findAllByContract(TRANSFORMER_CONTRACT_ID);
        const allEncoders: bigint[]    = rt.findAllByContract(ENCODER_CONTRACT_ID);
        const allReporters: bigint[]   = rt.findAllByContract(REPORTER_CONTRACT_ID);

        console.log(`Decoders:     ${allDecoders.length} (rust + python)`);
        console.log(`Validators:   ${allValidators.length} (cpp + lua + js)`);
        console.log(`Transformers: ${allTransformers.length} (cpp + lua)`);
        console.log(`Encoders:     ${allEncoders.length} (rust + csharp)`);
        console.log(`Reporters:    ${allReporters.length} (python + csharp + js)`);
        const total: number = allDecoders.length + allValidators.length +
          allTransformers.length + allEncoders.length + allReporters.length;
        console.log(`Total plugins: ${total}/12`);

        // Run a second pipeline pass with different plugin choices where available.
        if (allDecoders.length >= 2 || allEncoders.length >= 2) {
          const decoder2Handle: bigint = allDecoders.length >= 2
            ? allDecoders[allDecoders.length - 1]
            : decoderHandle;
          const encoder2Handle: bigint = allEncoders.length >= 2
            ? allEncoders[allEncoders.length - 1]
            : encoderHandle;
          const transformer2Handle: bigint = allTransformers.length >= 2
            ? allTransformers[allTransformers.length - 1]
            : transformerHandle;
          const validator2Handle: bigint = allValidators.length >= 2
            ? allValidators[allValidators.length - 1]
            : validatorHandle;
          const reporter2Handle: bigint = allReporters.length >= 2
            ? allReporters[allReporters.length - 1]
            : reporterHandle;

          const d2 = rt.resolvePlugin(decoder2Handle);
          const v2 = rt.resolvePlugin(validator2Handle);
          const t2 = rt.resolvePlugin(transformer2Handle);
          const e2 = rt.resolvePlugin(encoder2Handle);
          const r2 = rt.resolvePlugin(reporter2Handle);
          try {
            runPipeline(
              d2.vtable()!,
              v2.vtable()!,
              t2.vtable()!,
              e2.vtable()!,
              r2.vtable()!,
              inputCsv,
              "Run 2: alternate plugins per contract",
            );
          } finally {
            d2[Symbol.dispose]();
            v2[Symbol.dispose]();
            t2[Symbol.dispose]();
            e2[Symbol.dispose]();
            r2[Symbol.dispose]();
          }
        }

        runErrorScenario(decoderVt);

      } finally {
        decoderGuard[Symbol.dispose]();
        validatorGuard[Symbol.dispose]();
        transformerGuard[Symbol.dispose]();
        encoderGuard[Symbol.dispose]();
        reporterGuard[Symbol.dispose]();
      }
    } finally {
      rt[Symbol.dispose]();
    }
  } finally {
    lib.close();
  }

  console.log("\n=== pipeline complete ===");
}

main();
