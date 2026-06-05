function add(argsPtr, outPtr) {
    try {
        var a = polyplug.readI32(argsPtr);
        var b = polyplug.readI32(argsPtr + 4);
        var sum = a + b;
        polyplug.writeI32(outPtr, sum);
        return 0;
    } catch (e) {
        return 1;
    }
}

function subtract(argsPtr, outPtr) {
    try {
        var a = polyplug.readI32(argsPtr);
        var b = polyplug.readI32(argsPtr + 4);
        polyplug.writeI32(outPtr, a - b);
        return 0;
    } catch (e) {
        return 1;
    }
}

function multiply(argsPtr, outPtr) {
    try {
        var a = polyplug.readI32(argsPtr);
        var b = polyplug.readI32(argsPtr + 4);
        polyplug.writeI32(outPtr, a * b);
        return 0;
    } catch (e) {
        return 1;
    }
}

function divide(argsPtr, outPtr) {
    try {
        var a = polyplug.readI32(argsPtr);
        var b = polyplug.readI32(argsPtr + 4);
        polyplug.writeI32(outPtr, b === 0 ? 0 : Math.floor(a / b));
        return 0;
    } catch (e) {
        return 1;
    }
}

// echo returns its input string back as a freshly allocated StringView, sourced
// from the per-call CallArena via polyplug.arenaAlloc. The arena lets the host
// reclaim every return buffer with a single reset instead of per-value frees, so
// after warmup arenaAlloc serves from the bump region and triggers no host
// allocation. args/out are 12-byte StringView buffers { ptr_lo, ptr_hi, len }.
function echo(argsPtr, outPtr) {
    try {
        var inLo = polyplug.readU32(argsPtr);
        var inHi = polyplug.readU32(argsPtr + 4);
        var len = polyplug.readU32(argsPtr + 8);
        var inAddr = inHi * 0x100000000 + inLo;

        var outArr = polyplug.arenaAlloc(len === 0 ? 1 : len);
        var outAddr = outArr[1] * 0x100000000 + outArr[0];
        for (var i = 0; i < len; i++) {
            polyplug.writeByte(outAddr + i, polyplug.readByte(inAddr + i));
        }

        polyplug.writeU32(outPtr, outArr[0]);
        polyplug.writeU32(outPtr + 4, outArr[1]);
        polyplug.writeU32(outPtr + 8, len);
        return 0;
    } catch (e) {
        return 1;
    }
}

function polyplug_init(rt_ctx, host_vtable, ctx) {
    // Canonical contract id: fnv1a_64("guest_contract:test.add@1") = 0x40244DF59FCBECB6.
    // Passed split into 32-bit halves; the loader recomposes (hi << 32 | lo).
    var vtable = {
        contractLo: 0x9FCBECB6 >>> 0,
        contractHi: 0x40244DF5 >>> 0,
        fnCount: 5,
        contractName: "test.add",
        // Packed contract version: major << 16 (test.add@1 -> 0x10000).
        version: 0x10000,
        functions: [add, subtract, multiply, divide, echo]
    };
    polyplug.registerVtable(
        vtable.contractLo,
        vtable.contractHi,
        vtable,
        vtable.fnCount,
        vtable.contractName,
        vtable.version
    );
    return { code: 0, message: null };
}
