// examples/guests/js_quickjs/encoder/bundle.js
// Encoder — JS/QuickJS plugin for pipeline.Encoder@1
// Contract: encode(data: StringView) -> StringView
// Input:  "TRANSFORMED:NAME|value (transformed)|43"
// Output: "NAME,value (transformed),43"
//
// ENCODER_CONTRACT_ID = 0x127D1703C6EFB432
// lo = lower 32 bits = 0xC6EFB432
// hi = upper 32 bits = 0x127D1703

const contractLo = 0xC6EFB432 >>> 0;
const contractHi = 0x127D1703 >>> 0;
const vtableLo = 1;
const vtableHi = 0;
const fnCount = 1;

if (typeof polyplug !== "undefined") {
  polyplug.registerVtable(contractLo, contractHi, vtableLo, vtableHi, fnCount);
} else {
  const contractId = BigInt(contractLo) | (BigInt(contractHi) << 32n);
  Deno.core.ops.op_register_vtable(contractId, 1n, 1);
}
