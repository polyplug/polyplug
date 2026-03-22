// Test plugin for JS/QuickJS loader
// Contract: test.add@1 (contract_id = 0xCC4232FAB0410D2B)

function add(argsPtr, outPtr) {
    try {
        var a = polyplug.readI32(argsPtr);
        var b = polyplug.readI32(argsPtr + 4n);
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
        var b = polyplug.readI32(argsPtr + 4n);
        polyplug.writeI32(outPtr, a - b);
        return 0;
    } catch (e) {
        return 1;
    }
}

function multiply(argsPtr, outPtr) {
    try {
        var a = polyplug.readI32(argsPtr);
        var b = polyplug.readI32(argsPtr + 4n);
        polyplug.writeI32(outPtr, a * b);
        return 0;
    } catch (e) {
        return 1;
    }
}

function divide(argsPtr, outPtr) {
    try {
        var a = polyplug.readI32(argsPtr);
        var b = polyplug.readI32(argsPtr + 4n);
        polyplug.writeI32(outPtr, b === 0 ? 0 : Math.floor(a / b));
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