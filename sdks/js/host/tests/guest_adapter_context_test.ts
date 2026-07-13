import { InternalPluginBundle, buildInternalPluginGuestContract } from "../polyplug/mod.js";
import { GUEST_CONTRACT_INTERFACE_ADAPTER_CONTEXT_OFFSET } from "../../abi/abi.ts";
import { getBackend } from "@polyplug/abi";
import { assertEquals, test } from "../../testing/harness.ts";
import { bridgeLibrary } from "../../loaders/js/mod.ts";

test("generated bridge owns the opaque context used by canonical lifecycle callbacks", () => {
    const adapter = buildInternalPluginGuestContract({
        contractId: 0xABCDn,
        version: { major: 1, minor: 0, patch: 0 },
        implementation: {},
        methods: [],
    }, bridgeLibrary());

    try {
        assertEquals(
            getBackend().pointerView(adapter.interfacePtr).getBigUint64(
                GUEST_CONTRACT_INTERFACE_ADAPTER_CONTEXT_OFFSET,
            ) !== 0n,
            true,
        );
    } finally {
        adapter.resident.release();
    }
});

test("generated adapter factories isolate stateful JavaScript implementations", () => {
    const built: { count: number }[] = [];
    const adapter = buildInternalPluginGuestContract({
        contractId: 0xA11CE42n,
        version: { major: 1, minor: 0, patch: 0 },
        implementation: () => {
            const implementation = { count: 0 };
            built.push(implementation);
            return implementation;
        },
        methods: [
            (implementation: object) => {
                if (!("count" in implementation) || typeof implementation.count !== "number") {
                    throw new TypeError("counter implementation is invalid");
                }
                implementation.count += 1;
                return 0;
            },
        ],
    }, bridgeLibrary());

    try {
        const first = adapter._createForTest(null);
        const second = adapter._createForTest(null);
        adapter._dispatchForTest(getBackend().pointerCreate(first), 0, null, null);
        adapter._dispatchForTest(getBackend().pointerCreate(first), 0, null, null);
        adapter._dispatchForTest(getBackend().pointerCreate(second), 0, null, null);
        assertEquals(built[0].count, 0, "the null-data default implementation remains separate");
        assertEquals(built[1].count, 2, "first instance retains its own state");
        assertEquals(built[2].count, 1, "second instance retains its own state");
        adapter._destroyForTest(getBackend().pointerCreate(first));
        adapter._destroyForTest(getBackend().pointerCreate(second));
    } finally {
        adapter.resident.release();
    }
});

test("generated adapter forwards the call arena to internal marshalling callbacks", () => {
    let observed: unknown = undefined;
    const adapter = buildInternalPluginGuestContract({
        contractId: 0xA11CE43n,
        version: { major: 1, minor: 0, patch: 0 },
        implementation: {},
        methods: [
            (_implementation: object, _args: unknown, _out: unknown, arena: unknown) => {
                observed = arena;
                return 0;
            },
        ],
    }, bridgeLibrary());
    const arena = getBackend().pointerCreate(0xA11CE44n);

    try {
        assertEquals(adapter._dispatchForTest(null, 0, null, null, arena), 0);
        assertEquals(observed, arena);
    } finally {
        adapter.resident.release();
    }
});

test("disposed internal bundle releases its untransferred resident exactly once", () => {
    let releases = 0;
    const bundle = new InternalPluginBundle("name = \"dispose\"\n", {
        release(): void {
            releases += 1;
        },
    }, () => {});

    bundle.dispose();
    bundle.dispose();
    assertEquals(releases, 1);
});

test("generated adapter translates thrown JavaScript errors to AbiErrorCode.Panic", () => {
    const adapter = buildInternalPluginGuestContract({
        contractId: 0xBADF00Dn,
        version: { major: 1, minor: 0, patch: 0 },
        implementation: {},
        methods: [
            () => {
                throw new Error("dispatch failure");
            },
        ],
    }, bridgeLibrary());

    try {
        const instance = adapter._createForTest(null);
        assertEquals(adapter._dispatchForTest(getBackend().pointerCreate(instance), 0, null, null), 3);
    } finally {
        adapter.resident.release();
    }
});
