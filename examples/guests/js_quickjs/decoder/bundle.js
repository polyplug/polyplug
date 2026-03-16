// examples/guests/js_quickjs/decoder/bundle.js
// Decoder — JS/QuickJS plugin for pipeline.Decoder@1
// Contract: decode(input: string) -> string
// Input:  "name,value,42"
// Output: "DECODED:name|value|42"
//
// DECODER_CONTRACT_ID = 0x12F3C106B0C3DC1E
// lo = lower 32 bits = 0xB0C3DC1E
// hi = upper 32 bits = 0x12F3C106

const contractLo = 0xB0C3DC1E >>> 0;
const contractHi = 0x12F3C106 >>> 0;
const vtableLo = 1;
const vtableHi = 0;
const fnCount = 1;

if (typeof polyplug !== "undefined") {
  polyplug.registerVtable(contractLo, contractHi, vtableLo, vtableHi, fnCount);
} else {
  const contractId = BigInt(contractLo) | (BigInt(contractHi) << 32n);
  Deno.core.ops.op_register_vtable(contractId, 1n, 1);
}
