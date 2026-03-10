// showcase/plugins/field_validator/bundle.js
// Field Validator — JS/QuickJS plugin for pipeline.validator@1
// Demonstrates: JS plugin as an active pipeline step
//
// VALIDATOR_CONTRACT_ID = 0x027ABCEBF8020D90
// lo = lower 32 bits = 0xF8020D90
// hi = upper 32 bits = 0x027ABCEB

const contractLo = 0xF8020D90 >>> 0;
const contractHi = 0x027ABCEB >>> 0;
const vtableLo = 1;
const vtableHi = 0;
const fnCount = 1;

if (typeof polyplug !== "undefined") {
  polyplug.registerVtable(contractLo, contractHi, vtableLo, vtableHi, fnCount);
} else {
  const contractId = BigInt(contractLo) | (BigInt(contractHi) << 32n);
  Deno.core.ops.op_register_vtable(contractId, 1n, 1);
}
