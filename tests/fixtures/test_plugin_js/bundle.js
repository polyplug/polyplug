const contractLo = 0xB0410D2B >>> 0;
const contractHi = 0xCC4232FA >>> 0;
const vtableLo = 1;
const vtableHi = 0;
const fnCount = 4;

function init(bundlePath) {
    if (typeof polyplug !== "undefined") {
        polyplug.registerVtable(contractLo, contractHi, vtableLo, vtableHi, fnCount);
    } else {
        const contractId = BigInt(0xB0410D2B) | (BigInt(0xCC4232FA) << 32n);
        Deno.core.ops.op_register_vtable(contractId, 1n, 4);
    }
}
init(globalThis.bundlePath);  // bundlePath injected by loader
