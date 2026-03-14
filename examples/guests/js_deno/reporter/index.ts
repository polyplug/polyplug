// examples/guests/js_deno/reporter/index.ts
// Reporter — JS/Deno plugin for data.Reporter@1
// Contract: report(value: string) -> string
// Returns: "js_deno:report({value})"
//
// REPORTER_CONTRACT_ID = 0x81D41D43E511D297
// lo = lower 32 bits = 0xE511D297
// hi = upper 32 bits = 0x81D41D43

const contractLo = 0xE511D297 >>> 0;
const contractHi = 0x81D41D43 >>> 0;
const vtableLo = 1;
const vtableHi = 0;
const fnCount = 1;

const contractId = BigInt(contractLo) | (BigInt(contractHi) << 32n);
Deno.core.ops.op_register_vtable(contractId, 1n, 1);
