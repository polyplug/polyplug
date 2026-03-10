// THIS FILE IS PART OF THE POLYPLUG TEST FIXTURES
// DO NOT EDIT BY HAND
// Runtime: js-deno (loaded natively by deno_core — no compilation needed)

// Contract: test.add@1
// FNV-1a hash of "test.add@1": 0xCC4232FAB0410D2B
const CONTRACT_ID: bigint = 0xCC4232FAn << 32n | 0xB0410D2Bn;
const VTABLE_ID: bigint = 1n;
const FN_COUNT: number = 4;

// Register vtable with host
Deno.core.ops.op_register_vtable(CONTRACT_ID, VTABLE_ID, FN_COUNT);
