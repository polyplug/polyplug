// Hand-written QuickJS fixture for test.add@1. Mirrors the threaded-bridge model
// (Rule 12): polyplug_init takes the bridge as an explicit argument and RETURNS
// [registrations, abiError]; each ABI wrapper receives (impl, argsPtr, outPtr,
// arena_ptr, bridge); the factory receives (bridge, hostLo, hostHi). Nothing is
// read from any global — the bridge is threaded in everywhere.

function add(impl, argsPtr, outPtr, arena, bridge) {
    try {
        var a = bridge.readI32(argsPtr);
        var b = bridge.readI32(argsPtr + 4);
        bridge.writeI32(outPtr, a + b);
        return 0;
    } catch (e) {
        return 1;
    }
}

function subtract(impl, argsPtr, outPtr, arena, bridge) {
    try {
        var a = bridge.readI32(argsPtr);
        var b = bridge.readI32(argsPtr + 4);
        bridge.writeI32(outPtr, a - b);
        return 0;
    } catch (e) {
        return 1;
    }
}

function multiply(impl, argsPtr, outPtr, arena, bridge) {
    try {
        var a = bridge.readI32(argsPtr);
        var b = bridge.readI32(argsPtr + 4);
        bridge.writeI32(outPtr, a * b);
        return 0;
    } catch (e) {
        return 1;
    }
}

function divide(impl, argsPtr, outPtr, arena, bridge) {
    try {
        var a = bridge.readI32(argsPtr);
        var b = bridge.readI32(argsPtr + 4);
        bridge.writeI32(outPtr, b === 0 ? 0 : Math.floor(a / b));
        return 0;
    } catch (e) {
        return 1;
    }
}

// echo returns its input string back as a freshly allocated StringView, sourced
// from the per-call CallArena via bridge.arenaAlloc(size, arena_ptr) — the arena
// pointer is THREADED in as the wrapper's `arena` argument (no VM global). The
// arena lets the host reclaim every return buffer with a single reset instead of
// per-value frees. args/out are 12-byte StringView buffers { ptr_lo, ptr_hi, len }.
function echo(impl, argsPtr, outPtr, arena, bridge) {
    try {
        var inLo = bridge.readU32(argsPtr);
        var inHi = bridge.readU32(argsPtr + 4);
        var len = bridge.readU32(argsPtr + 8);
        var inAddr = inHi * 0x100000000 + inLo;

        var outArr = bridge.arenaAlloc(len === 0 ? 1 : len, arena);
        var outAddr = outArr[1] * 0x100000000 + outArr[0];
        for (var i = 0; i < len; i++) {
            bridge.writeByte(outAddr + i, bridge.readByte(inAddr + i));
        }

        bridge.writeU32(outPtr, outArr[0]);
        bridge.writeU32(outPtr + 4, outArr[1]);
        bridge.writeU32(outPtr + 8, len);
        return 0;
    } catch (e) {
        return 1;
    }
}

// The loader calls polyplug_init with the HostApi and BundleInitContext pointers
// split into 32-bit lo/hi f64 halves plus the bridge: (host_lo, host_hi, ctx_lo,
// ctx_hi, bridge). It RETURNS [registrations, abiError]; nothing is deposited
// into any global.
function polyplug_init(host_lo, host_hi, ctx_lo, ctx_hi, bridge) {
    // Canonical contract id: fnv1a_64("guest_contract:test.add@1") = 0x40244DF59FCBECB6.
    // Passed split into 32-bit halves; the loader recomposes (hi << 32 | lo).
    var iface = {
        fnCount: 5,
        // Packed contract version: major << 16 (test.add@1 -> 0x10000).
        version: 0x10000,
        factory: function (bridge, hostLo, hostHi) { return {}; },
        functions: [add, subtract, multiply, divide, echo]
    };
    var registrations = [{
        contractLo: 0x9FCBECB6 >>> 0,
        contractHi: 0x40244DF5 >>> 0,
        interface: iface,
        fnCount: iface.fnCount,
        contractName: "test.add",
        version: iface.version
    }];
    return [registrations, { code: 0, message: "" }];
}
