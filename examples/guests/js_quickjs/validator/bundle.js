// examples/guests/js_quickjs/validator/bundle.js
// Validator — JS/QuickJS plugin for pipeline.Validator@1
// Contract: validate(data: string) -> string
// Input:  "DECODED:name|value|42"
// Output: "VALID:ok" or "INVALID:reason"
//
// VALIDATOR_CONTRACT_ID = 0xA553FAB5D11C7AF0
// lo = lower 32 bits = 0xD11C7AF0
// hi = upper 32 bits = 0xA553FAB5

const contractLo = 0xD11C7AF0 >>> 0;
const contractHi = 0xA553FAB5 >>> 0;
const vtableLo = 1;
const vtableHi = 0;
const fnCount = 1;

if (typeof polyplug !== "undefined") {
  polyplug.registerVtable(contractLo, contractHi, vtableLo, vtableHi, fnCount);
} else {
  const contractId = BigInt(contractLo) | (BigInt(contractHi) << 32n);
  Deno.core.ops.op_register_vtable(contractId, 1n, 1);
}
