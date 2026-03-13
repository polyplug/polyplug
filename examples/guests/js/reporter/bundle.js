// examples/guests/js/reporter/bundle.js
// Summary Reporter — JS/QuickJS plugin for pipeline.reporter@1
// Demonstrates: JS plugin as a terminal pipeline step (reporting/output)
//
// REPORTER_CONTRACT_ID = 0xD50E539CAE219A15
// lo = lower 32 bits = 0xAE219A15
// hi = upper 32 bits = 0xD50E539C

const contractLo = 0xAE219A15 >>> 0;
const contractHi = 0xD50E539C >>> 0;
const vtableLo = 1;
const vtableHi = 0;
const fnCount = 1;

if (typeof polyplug !== "undefined") {
  polyplug.registerVtable(contractLo, contractHi, vtableLo, vtableHi, fnCount);
} else {
  const contractId = BigInt(contractLo) | (BigInt(contractHi) << 32n);
  Deno.core.ops.op_register_vtable(contractId, 1n, 1);
}
