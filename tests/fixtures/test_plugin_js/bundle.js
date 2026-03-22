// Test plugin for JS/QuickJS loader
// Contract: test.add@1 (contract_id = 0xCC4232FAB0410D2B)

function addOffset(lo, hi, offset) {
    var newLo = (lo + offset) >>> 0;
    var carry = (lo + offset) > 0xFFFFFFFF ? 1 : 0;
    var newHi = (hi + carry) >>> 0;
    return [newLo, newHi];
}

function add(argsLo, argsHi, outLo, outHi) {
    try {
        var a = polyplug.readI32(argsLo, argsHi);
        var ptr2 = addOffset(argsLo, argsHi, 4);
        var b = polyplug.readI32(ptr2[0], ptr2[1]);
        var sum = a + b;
        polyplug.writeI32(outLo, outHi, sum);
        return 0;
    } catch (e) {
        return 1;
    }
}

function subtract(argsLo, argsHi, outLo, outHi) {
    try {
        var a = polyplug.readI32(argsLo, argsHi);
        var ptr2 = addOffset(argsLo, argsHi, 4);
        var b = polyplug.readI32(ptr2[0], ptr2[1]);
        polyplug.writeI32(outLo, outHi, a - b);
        return 0;
    } catch (e) {
        return 1;
    }
}

function multiply(argsLo, argsHi, outLo, outHi) {
    try {
        var a = polyplug.readI32(argsLo, argsHi);
        var ptr2 = addOffset(argsLo, argsHi, 4);
        var b = polyplug.readI32(ptr2[0], ptr2[1]);
        polyplug.writeI32(outLo, outHi, a * b);
        return 0;
    } catch (e) {
        return 1;
    }
}

function divide(argsLo, argsHi, outLo, outHi) {
    try {
        var a = polyplug.readI32(argsLo, argsHi);
        var ptr2 = addOffset(argsLo, argsHi, 4);
        var b = polyplug.readI32(ptr2[0], ptr2[1]);
        polyplug.writeI32(outLo, outHi, b === 0 ? 0 : Math.floor(a / b));
        return 0;
    } catch (e) {
        return 1;
    }
}

globalThis.TEST_ADD_VTABLE = {
    contractLo: 0xB0410D2B >>> 0,
    contractHi: 0xCC4232FA >>> 0,
    fnCount: 4,
    contractName: "test.add",
    functions: [add, subtract, multiply, divide]
};