import {
    buildInternalPluginBundle,
    buildInternalPluginGuestContract,
} from "../polyplug/mod.js";
import {
    HOST_API_REGISTER_GUEST_CONTRACT_OFFSET,
    HOST_API_SIZE,
    PLUGIN_DESCRIPTOR_CONTRACT_NAME_OFFSET,
    PLUGIN_DESCRIPTOR_NAME_OFFSET,
    STRING_VIEW_LEN_OFFSET,
    STRING_VIEW_PTR_OFFSET,
} from "../../abi/abi.ts";
import { type FfiPointerView, getBackend } from "@polyplug/abi";
import { assertEquals, test } from "../../testing/harness.ts";
import { bridgeLibrary } from "../../loaders/js/mod.ts";

const MANIFEST = 'name = "js.atomic"\nid = 40961\nversion = "1.0.0"\nloader = "js-quickjs"\nprovides = ["js.first@1", "js.second@1"]\nfunction_count = { "js.first@1" = 0, "js.second@1" = 0 }\nneeds_reinit_on_dep_reload = false\nfile = "plugin.js"\n';

function readString(view: FfiPointerView, offset: number): string {
    const backend = getBackend();
    const pointer = backend.pointerCreate(view.getBigUint64(offset + STRING_VIEW_PTR_OFFSET));
    if (pointer === null) {
        throw new Error("descriptor string pointer is null");
    }
    const length = Number(view.getBigUint64(offset + STRING_VIEW_LEN_OFFSET));
    return new TextDecoder().decode(backend.pointerView(pointer).getArrayBuffer(length));
}

test("internal-plugin bundle stages existing descriptor/interface pairs from canonical manifest bytes", () => {
    const backend = getBackend();
    const bridge = bridgeLibrary();
    const first = buildInternalPluginGuestContract({
        contractId: 0xA001n,
        version: { major: 1, minor: 0, patch: 0 },
        implementation: {},
        methods: [],
    }, bridge);
    const second = buildInternalPluginGuestContract({
        contractId: 0xA002n,
        version: { major: 1, minor: 0, patch: 0 },
        implementation: {},
        methods: [],
    }, bridge);
    const bundle = buildInternalPluginBundle({
        manifest: MANIFEST,
        contracts: [
            {
                provider: "js-first",
                contractName: "js.first",
                version: { major: 1, minor: 0, patch: 0 },
                adapter: first,
            },
            {
                provider: "js-second",
                contractName: "js.second",
                version: { major: 1, minor: 0, patch: 0 },
                adapter: second,
            },
        ],
    });
    const hostTable = new Uint8Array(HOST_API_SIZE);
    const hostView = new DataView(hostTable.buffer);
    const staged: Array<{ provider: string; contract: string; interfacePtr: bigint }> = [];
    const register = backend.makeCallback(
        { parameters: ["pointer", "pointer", "pointer", "pointer"], result: "void" },
        (_host, descriptor, interfacePtr, outError) => {
            const descriptorView = backend.pointerView(descriptor);
            staged.push({
                provider: readString(descriptorView, PLUGIN_DESCRIPTOR_NAME_OFFSET),
                contract: readString(descriptorView, PLUGIN_DESCRIPTOR_CONTRACT_NAME_OFFSET),
                interfacePtr: backend.pointerValue(interfacePtr),
            });
            new DataView(backend.pointerView(outError).getArrayBuffer(24)).setUint32(0, 0, true);
        },
    );
    hostView.setBigUint64(
        HOST_API_REGISTER_GUEST_CONTRACT_OFFSET,
        backend.pointerValue(register.pointer),
        true,
    );

    try {
        bundle._reserveInternalPluginTransfer();
        bundle._registerGuestContracts(backend.pointerOf(hostTable));
        assertEquals(staged.length, 2);
        assertEquals(staged[0].provider, "js-first");
        assertEquals(staged[0].contract, "js.first");
        assertEquals(staged[0].interfacePtr, backend.pointerValue(first.interfacePtr));
        assertEquals(staged[1].provider, "js-second");
        assertEquals(staged[1].contract, "js.second");
        assertEquals(staged[1].interfacePtr, backend.pointerValue(second.interfacePtr));
    } finally {
        bundle._takeInternalPluginResident().release();
        register.close();
    }
});
