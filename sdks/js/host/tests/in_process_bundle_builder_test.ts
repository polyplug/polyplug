import {
    buildInProcessBundle,
    buildInProcessGuestContract,
} from "../polyplug/mod.js";
import {
    IN_PROCESS_BUNDLE_REGISTRATION_CONTRACT_COUNT_OFFSET,
    IN_PROCESS_BUNDLE_REGISTRATION_CONTRACTS_OFFSET,
    IN_PROCESS_CONTRACT_REGISTRATION_ADAPTER_CONTEXT_OFFSET,
    IN_PROCESS_CONTRACT_REGISTRATION_INTERFACE_OFFSET,
    IN_PROCESS_CONTRACT_REGISTRATION_SIZE,
} from "../../abi/abi.ts";
import { getBackend } from "@polyplug/abi";
import { assertEquals, test } from "../../testing/harness.ts";
import { bridgeLibrary } from "../../loaders/js/mod.ts";

test("in-process bundle builds one atomic registration for every contract", () => {
    const bridge = bridgeLibrary();
    const first = buildInProcessGuestContract({
        contractId: 0xA001n,
        version: { major: 1, minor: 0, patch: 0 },
        implementation: {},
        methods: [],
    }, bridge);
    const second = buildInProcessGuestContract({
        contractId: 0xA002n,
        version: { major: 1, minor: 0, patch: 0 },
        implementation: {},
        methods: [],
    }, bridge);
    const bundle = buildInProcessBundle({
        name: "js.atomic",
        version: { major: 1, minor: 0, patch: 0 },
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

    try {
        const registration = new DataView(bundle._inProcessRegistration().buffer);
        assertEquals(
            registration.getBigUint64(IN_PROCESS_BUNDLE_REGISTRATION_CONTRACT_COUNT_OFFSET, true),
            2n,
        );
        const contracts = getBackend().pointerCreate(
            registration.getBigUint64(IN_PROCESS_BUNDLE_REGISTRATION_CONTRACTS_OFFSET, true),
        );
        if (contracts === null) {
            throw new Error("multi-contract registration has no contracts table");
        }
        const table = getBackend().pointerView(contracts);
        for (let index = 0; index < 2; index += 1) {
            const base = index * IN_PROCESS_CONTRACT_REGISTRATION_SIZE;
            assertEquals(
                table.getBigUint64(base + IN_PROCESS_CONTRACT_REGISTRATION_INTERFACE_OFFSET),
                getBackend().pointerValue(index === 0 ? first.interfacePtr : second.interfacePtr),
            );
            assertEquals(
                table.getBigUint64(base + IN_PROCESS_CONTRACT_REGISTRATION_ADAPTER_CONTEXT_OFFSET),
                getBackend().pointerValue(index === 0 ? first.adapterContext : second.adapterContext),
            );
        }
    } finally {
        bundle._takeInProcessResident().release();
    }
});
