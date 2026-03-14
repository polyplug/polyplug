// examples/guests/js_deno/transformer/index.ts
// Transformer — JS/Deno plugin for data.Transformer@1
// Contract: transform(input: string) -> string
// Returns: "js_deno:transform({input})"
//
// TRANSFORMER_CONTRACT_ID = 0x3D53C682F3F5A9EF
// lo = lower 32 bits = 0xF3F5A9EF
// hi = upper 32 bits = 0x3D53C682

const contractLo = 0xF3F5A9EF >>> 0;
const contractHi = 0x3D53C682 >>> 0;
const vtableLo = 1;
const vtableHi = 0;
const fnCount = 1;

const contractId = BigInt(contractLo) | (BigInt(contractHi) << 32n);
Deno.core.ops.op_register_vtable(contractId, 1n, 1);
