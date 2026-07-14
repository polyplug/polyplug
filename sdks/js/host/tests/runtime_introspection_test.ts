import { BundleSourceKind, Runtime } from "../polyplug/mod.js";
import {
    BUNDLE_DESCRIPTOR_VIEW_ID_OFFSET,
    BUNDLE_DESCRIPTOR_VIEW_NAME_OFFSET,
    BUNDLE_DESCRIPTOR_VIEW_RUNTIME_OFFSET,
    BUNDLE_DESCRIPTOR_VIEW_SIZE,
    BUNDLE_DESCRIPTOR_VIEW_SOURCE_KIND_OFFSET,
    BUNDLE_DESCRIPTOR_VIEW_VERSION_OFFSET,
    GUEST_CONTRACT_HANDLE_GENERATION_OFFSET,
    GUEST_CONTRACT_HANDLE_INDEX_OFFSET,
    GUEST_CONTRACT_HANDLE_SIZE,
    HOST_API_FIND_ALL_GUEST_CONTRACTS_OFFSET,
    HOST_API_FREE_OFFSET,
    HOST_API_LIST_BUNDLES_OFFSET,
    HOST_API_RESERVED_OFFSET,
    HOST_API_SIZE,
    ARRAY_ALIGN_OFFSET,
    ARRAY_ITEMS_OFFSET,
    ARRAY_LEN_OFFSET,
    OWNED_PLUGIN_DESCRIPTOR_VIEW_CONTRACT_NAME_OFFSET,
    OWNED_PLUGIN_DESCRIPTOR_VIEW_NAME_OFFSET,
    OWNED_PLUGIN_DESCRIPTOR_VIEW_VERSION_OFFSET,
    REGISTERED_CONTRACT_DESCRIPTOR_VIEW_BUNDLE_ID_OFFSET,
    REGISTERED_CONTRACT_DESCRIPTOR_VIEW_CONTRACT_ID_OFFSET,
    REGISTERED_CONTRACT_DESCRIPTOR_VIEW_HANDLE_OFFSET,
    REGISTERED_CONTRACT_DESCRIPTOR_VIEW_PLUGIN_OFFSET,
    REGISTERED_CONTRACT_DESCRIPTOR_VIEW_SIZE,
    RUNTIME_INTROSPECTION_GET_BUNDLE_DESCRIPTOR_OFFSET,
    RUNTIME_INTROSPECTION_GET_REGISTERED_CONTRACT_DESCRIPTOR_OFFSET,
    RUNTIME_INTROSPECTION_LIST_REGISTERED_GUEST_CONTRACTS_OFFSET,
    
    VERSION_MAJOR_OFFSET,
    VERSION_MINOR_OFFSET,
    VERSION_PATCH_OFFSET,
} from "../../abi/abi.ts";
import { getBackend, type FfiBackend } from "@polyplug/abi";
import { assertEquals, test } from "../../testing/harness.ts";

const ARRAY_STRUCT_SIZE = 24;
const encoder = new TextEncoder();

function writeVersion(view: DataView, offset: number, version: readonly [number, number, number]): void {
    view.setUint32(offset + VERSION_MAJOR_OFFSET, version[0], true);
    view.setUint32(offset + VERSION_MINOR_OFFSET, version[1], true);
    view.setUint32(offset + VERSION_PATCH_OFFSET, version[2], true);
}

function writeOwnedArray(
    view: DataView,
    offset: number,
    value: Uint8Array,
    pointerValue: bigint,
): void {
    view.setBigUint64(offset + ARRAY_ITEMS_OFFSET, pointerValue, true);
    view.setBigUint64(offset + ARRAY_LEN_OFFSET, BigInt(value.byteLength), true);
    view.setBigUint64(offset + ARRAY_ALIGN_OFFSET, 1n, true);
}

function writeAbiArray(
    backend: FfiBackend,
    out: unknown,
    items: unknown,
    length: bigint,
    align: bigint,
): void {
    const view = new DataView(backend.pointerView(out).getArrayBuffer(ARRAY_STRUCT_SIZE));
    view.setBigUint64(0, items === null ? 0n : backend.pointerValue(items), true);
    view.setBigUint64(8, length, true);
    view.setBigUint64(16, align, true);
}

function captureExplicitArrayCalls(
    backend: FfiBackend,
    pointers: ReadonlySet<bigint>,
): { readonly declarations: Array<{ readonly parameters: readonly unknown[]; readonly result: unknown }>; restore(): void } {
    const declarations: Array<{ readonly parameters: readonly unknown[]; readonly result: unknown }> = [];
    const original = backend.callFunction;
    const ownDescriptor = Object.getOwnPropertyDescriptor(backend, "callFunction");
    const mutableBackend = backend as Omit<FfiBackend, "callFunction"> & {
        callFunction?: (
            pointer: unknown,
            definition: { readonly parameters: readonly unknown[]; readonly result: unknown },
            args: readonly unknown[],
        ) => unknown;
    };
    mutableBackend.callFunction = (pointer, definition, args) => {
        if (pointers.has(backend.pointerValue(pointer))) {
            declarations.push(definition);
        }
        return original.call(backend, pointer, definition, args);
    };
    return {
        declarations,
        restore(): void {
            if (ownDescriptor === undefined) {
                delete mutableBackend.callFunction;
            } else {
                Object.defineProperty(backend, "callFunction", ownDescriptor);
            }
        },
    };
}

function captureArrayOutputAllocation(
    backend: FfiBackend,
): { readonly allocations: ArrayBufferView[]; restore(): void } {
    const allocations: ArrayBufferView[] = [];
    const original = backend.pointerOf;
    const ownDescriptor = Object.getOwnPropertyDescriptor(backend, "pointerOf");
    const mutableBackend = backend as Omit<FfiBackend, "pointerOf"> & {
        pointerOf?: (view: ArrayBufferView) => unknown;
    };
    mutableBackend.pointerOf = (view) => {
        if (view instanceof BigUint64Array && view.length === 3) {
            allocations.push(view);
        }
        return original.call(backend, view);
    };
    return {
        allocations,
        restore(): void {
            if (ownDescriptor === undefined) {
                delete mutableBackend.pointerOf;
            } else {
                Object.defineProperty(backend, "pointerOf", ownDescriptor);
            }
        },
    };
}

test("runtime introspection snapshots copied immutable descriptors and releases ABI arrays", () => {
    const backend = getBackend();
    const bundleNames = ["internal", "path", "code", "bytes"].map((name) => encoder.encode(name));
    const pluginNames = ["first-provider", "second-provider"].map((name) => encoder.encode(name));
    const contractNames = ["demo.alpha@1", "demo.beta@2"].map((name) => encoder.encode(name));
    const bundleIds = new Uint8Array(4 * 8);
    const bundleIdView = new DataView(bundleIds.buffer);
    const bundles = [
        { id: 101n, version: [1, 0, 0] as const, runtime: 0, sourceKind: BundleSourceKind.Internal },
        { id: 102n, version: [2, 1, 0] as const, runtime: 1, sourceKind: BundleSourceKind.Path },
        { id: 103n, version: [3, 2, 1] as const, runtime: 2, sourceKind: BundleSourceKind.Code },
        { id: 104n, version: [4, 3, 2] as const, runtime: 3, sourceKind: BundleSourceKind.Bytes },
    ];
    for (let index = 0; index < bundles.length; index += 1) {
        bundleIdView.setBigUint64(index * 8, bundles[index].id, true);
    }

    const handles = new Uint8Array(2 * GUEST_CONTRACT_HANDLE_SIZE);
    const handleView = new DataView(handles.buffer);
    const contracts = [
        { index: 7, generation: 11, bundleId: 101n, contractId: 0xAA01n, version: [1, 2, 3] as const },
        { index: 9, generation: 13, bundleId: 104n, contractId: 0xBB02n, version: [4, 5, 6] as const },
    ];
    for (let index = 0; index < contracts.length; index += 1) {
        const offset = index * GUEST_CONTRACT_HANDLE_SIZE;
        handleView.setUint32(offset + GUEST_CONTRACT_HANDLE_INDEX_OFFSET, contracts[index].index, true);
        handleView.setUint32(offset + GUEST_CONTRACT_HANDLE_GENERATION_OFFSET, contracts[index].generation, true);
    }

    const bundleNamePointers = bundleNames.map((name) => backend.pointerValue(backend.pointerOf(name)));
    const pluginNamePointers = pluginNames.map((name) => backend.pointerValue(backend.pointerOf(name)));
    const contractNamePointers = contractNames.map((name) => backend.pointerValue(backend.pointerOf(name)));
    const frees: Array<{ readonly pointer: bigint; readonly size: bigint; readonly align: bigint }> = [];
    const arrayOutputPointers: bigint[] = [];

    const listBundles = backend.makeCallback(
        { parameters: ["pointer", "pointer"], result: "void" },
        (_host, out) => {
            arrayOutputPointers.push(backend.pointerValue(out));
            writeAbiArray(backend, out, backend.pointerOf(bundleIds), 4n, 8n);
        },
    );
    const getBundleDescriptor = backend.makeCallback(
        { parameters: ["pointer", "u64", "pointer"], result: "bool" },
        (_host, bundleId, outDescriptor) => {
            const index = bundles.findIndex((bundle) => bundle.id === bundleId);
            if (index < 0) {
                return false;
            }
            const bundle = bundles[index];
            const view = new DataView(
                backend.pointerView(outDescriptor).getArrayBuffer(BUNDLE_DESCRIPTOR_VIEW_SIZE),
            );
            view.setBigUint64(BUNDLE_DESCRIPTOR_VIEW_ID_OFFSET, bundle.id, true);
            writeOwnedArray(view, BUNDLE_DESCRIPTOR_VIEW_NAME_OFFSET, bundleNames[index], bundleNamePointers[index]);
            writeVersion(view, BUNDLE_DESCRIPTOR_VIEW_VERSION_OFFSET, bundle.version);
            view.setUint32(BUNDLE_DESCRIPTOR_VIEW_RUNTIME_OFFSET, bundle.runtime, true);
            view.setUint32(BUNDLE_DESCRIPTOR_VIEW_SOURCE_KIND_OFFSET, bundle.sourceKind, true);
            return true;
        },
    );
    const listContracts = backend.makeCallback(
        { parameters: ["pointer", "pointer"], result: "void" },
        (_host, out) => {
            arrayOutputPointers.push(backend.pointerValue(out));
            writeAbiArray(backend, out, backend.pointerOf(handles), 2n, 4n);
        },
    );
    const getContractDescriptor = backend.makeCallback(
        { parameters: ["pointer", "u64", "pointer"], result: "bool" },
        (_host, packedHandle, outDescriptor) => {
            const index = contracts.findIndex((contract) =>
                contract.index === Number(packedHandle & 0xFFFF_FFFFn)
                && contract.generation === Number(packedHandle >> 32n)
            );
            if (index < 0) {
                return false;
            }
            const contract = contracts[index];
            const view = new DataView(
                backend.pointerView(outDescriptor).getArrayBuffer(REGISTERED_CONTRACT_DESCRIPTOR_VIEW_SIZE),
            );
            view.setUint32(
                REGISTERED_CONTRACT_DESCRIPTOR_VIEW_HANDLE_OFFSET + GUEST_CONTRACT_HANDLE_INDEX_OFFSET,
                contract.index,
                true,
            );
            view.setUint32(
                REGISTERED_CONTRACT_DESCRIPTOR_VIEW_HANDLE_OFFSET + GUEST_CONTRACT_HANDLE_GENERATION_OFFSET,
                contract.generation,
                true,
            );
            view.setBigUint64(REGISTERED_CONTRACT_DESCRIPTOR_VIEW_BUNDLE_ID_OFFSET, contract.bundleId, true);
            view.setBigUint64(REGISTERED_CONTRACT_DESCRIPTOR_VIEW_CONTRACT_ID_OFFSET, contract.contractId, true);
            const pluginOffset = REGISTERED_CONTRACT_DESCRIPTOR_VIEW_PLUGIN_OFFSET;
            writeOwnedArray(
                view,
                pluginOffset + OWNED_PLUGIN_DESCRIPTOR_VIEW_NAME_OFFSET,
                pluginNames[index],
                pluginNamePointers[index],
            );
            writeOwnedArray(
                view,
                pluginOffset + OWNED_PLUGIN_DESCRIPTOR_VIEW_CONTRACT_NAME_OFFSET,
                contractNames[index],
                contractNamePointers[index],
            );
            writeVersion(
                view, pluginOffset + OWNED_PLUGIN_DESCRIPTOR_VIEW_VERSION_OFFSET, contract.version,
            );
            return true;
        },
    );
    const free = backend.makeCallback(
        { parameters: ["pointer", "pointer", "usize", "usize"], result: "void" },
        (_host, items, size, align) => {
            frees.push({ pointer: backend.pointerValue(items), size, align });
        },
    );

    const findAll = backend.makeCallback(
        { parameters: ["pointer", "u64", "u32", "pointer"], result: "void" },
        (_host, _contractId, _minVersion, out) => {
            arrayOutputPointers.push(backend.pointerValue(out));
            writeAbiArray(backend, out, backend.pointerOf(handles), 2n, 4n);
        },
    );
    const explicitArrayCalls = captureExplicitArrayCalls(
        backend,
        new Set([
            backend.pointerValue(listBundles.pointer),
            backend.pointerValue(findAll.pointer),
            backend.pointerValue(listContracts.pointer),
        ]),
    );
    const arrayOutputAllocation = captureArrayOutputAllocation(backend);
    const introspection = new Uint8Array(24);
    const introspectionView = new DataView(introspection.buffer);
    introspectionView.setBigUint64(
        RUNTIME_INTROSPECTION_GET_BUNDLE_DESCRIPTOR_OFFSET,
        backend.pointerValue(getBundleDescriptor.pointer),
        true,
    );
    introspectionView.setBigUint64(
        RUNTIME_INTROSPECTION_LIST_REGISTERED_GUEST_CONTRACTS_OFFSET,
        backend.pointerValue(listContracts.pointer),
        true,
    );
    introspectionView.setBigUint64(
        RUNTIME_INTROSPECTION_GET_REGISTERED_CONTRACT_DESCRIPTOR_OFFSET,
        backend.pointerValue(getContractDescriptor.pointer),
        true,
    );

    const host = new Uint8Array(HOST_API_SIZE);
    const hostView = new DataView(host.buffer);
    hostView.setBigUint64(HOST_API_LIST_BUNDLES_OFFSET, backend.pointerValue(listBundles.pointer), true);
    hostView.setBigUint64(HOST_API_FIND_ALL_GUEST_CONTRACTS_OFFSET, backend.pointerValue(findAll.pointer), true);
    hostView.setBigUint64(HOST_API_FREE_OFFSET, backend.pointerValue(free.pointer), true);
    hostView.setBigUint64(HOST_API_RESERVED_OFFSET, backend.pointerValue(backend.pointerOf(introspection)), true);
    const library = {
        symbols: { polyplug_runtime_destroy: () => true },
        _roots: [
            bundleNames,
            pluginNames,
            contractNames,
            bundleIds,
            handles,
            introspection,
            host,
            listBundles,
            getBundleDescriptor,
            listContracts,
            findAll,
            getContractDescriptor,
            free,
        ],
    };
    const runtime = new Runtime(library as never, backend.pointerOf(host));

    try {
        const bundleSnapshot = runtime.bundleDescriptors();
        assertEquals(bundleSnapshot.map((bundle) => bundle.sourceKind), [
            BundleSourceKind.Internal,
            BundleSourceKind.Path,
            BundleSourceKind.Code,
            BundleSourceKind.Bytes,
        ]);
        assertEquals(bundleSnapshot.map((bundle) => bundle.name), ["internal", "path", "code", "bytes"]);
        assertEquals(bundleSnapshot[2].version, { major: 3, minor: 2, patch: 1 });
        assertEquals(Object.isFrozen(bundleSnapshot), true);
        assertEquals(Object.isFrozen(bundleSnapshot[0]), true);
        assertEquals(Object.isFrozen(bundleSnapshot[0].version), true);
        bundleNames[0][0] = "X".charCodeAt(0);
        assertEquals(bundleSnapshot[0].name, "internal", "descriptor strings must be copied before return");
        assertEquals(runtime.findAllGuestContracts(0xAA01n), [
            { index: 7, generation: 11 },
            { index: 9, generation: 13 },
        ]);

        const contractSnapshot = runtime.registeredContractDescriptors();
        assertEquals(contractSnapshot, [
            {
                handle: { index: 7, generation: 11 },
                bundleId: 101n,
                contractId: 0xAA01n,
                pluginName: "first-provider",
                contractName: "demo.alpha@1",
                version: { major: 1, minor: 2, patch: 3 },
            },
            {
                handle: { index: 9, generation: 13 },
                bundleId: 104n,
                contractId: 0xBB02n,
                pluginName: "second-provider",
                contractName: "demo.beta@2",
                version: { major: 4, minor: 5, patch: 6 },
            },
        ]);
        assertEquals(Object.isFrozen(contractSnapshot), true);
        assertEquals(Object.isFrozen(contractSnapshot[0]), true);
        assertEquals(Object.isFrozen(contractSnapshot[0].handle), true);
        assertEquals(
            arrayOutputAllocation.allocations.length,
            3,
            "Array<T> output buffers must be BigUint64Array(3)",
        );
        assertEquals(Object.isFrozen(contractSnapshot[0].version), true);
        assertEquals(
            arrayOutputPointers.map((pointer) => pointer % 8n),
            [0n, 0n, 0n],
            "Array<T> output buffers must be eight-byte aligned",
        );
        assertEquals(explicitArrayCalls.declarations, [
            { parameters: ["pointer", "pointer"], result: "void" },
            { parameters: ["pointer", "u64", "u32", "pointer"], result: "void" },
            { parameters: ["pointer", "pointer"], result: "void" },
        ], "all maintained FFI backends must receive explicit trailing Array* signatures");
        assertEquals(frees, [
            { pointer: bundleNamePointers[0], size: BigInt(bundleNames[0].byteLength), align: 1n },
            { pointer: bundleNamePointers[1], size: BigInt(bundleNames[1].byteLength), align: 1n },
            { pointer: bundleNamePointers[2], size: BigInt(bundleNames[2].byteLength), align: 1n },
            { pointer: bundleNamePointers[3], size: BigInt(bundleNames[3].byteLength), align: 1n },
            { pointer: backend.pointerValue(backend.pointerOf(bundleIds)), size: 32n, align: 8n },
            { pointer: backend.pointerValue(backend.pointerOf(handles)), size: 16n, align: 4n },
            { pointer: pluginNamePointers[0], size: BigInt(pluginNames[0].byteLength), align: 1n },
            { pointer: contractNamePointers[0], size: BigInt(contractNames[0].byteLength), align: 1n },
            { pointer: pluginNamePointers[1], size: BigInt(pluginNames[1].byteLength), align: 1n },
            { pointer: contractNamePointers[1], size: BigInt(contractNames[1].byteLength), align: 1n },
            { pointer: backend.pointerValue(backend.pointerOf(handles)), size: 16n, align: 4n },
        ], "each descriptor allocation and returned ABI array must be released exactly once");
        assertEquals(runtime.destroy(), true, "introspection must preserve Runtime destruction");
    } finally {
        arrayOutputAllocation.restore();
        explicitArrayCalls.restore();
        runtime.destroy();
        listBundles.close();
        getBundleDescriptor.close();
        listContracts.close();
        findAll.close();
        getContractDescriptor.close();
        free.close();
    }
});

test("runtime introspection returns frozen empty snapshots for empty and legacy hosts", () => {
    const backend = getBackend();
    const emptyItems = new Uint8Array(1);
    const emptyFrees: Array<{ readonly size: bigint; readonly align: bigint }> = [];
    const emptyArray = backend.makeCallback(
        { parameters: ["pointer", "pointer"], result: "void" },
        (_host, out) => writeAbiArray(backend, out, backend.pointerOf(emptyItems), 0n, 1n),
    );
    const descriptor = backend.makeCallback(
        { parameters: ["pointer", "u64", "pointer"], result: "bool" },
        () => false,
    );
    const contractDescriptor = backend.makeCallback(
        { parameters: ["pointer", "u64", "pointer"], result: "bool" },
        () => false,
    );
    const free = backend.makeCallback(
        { parameters: ["pointer", "pointer", "usize", "usize"], result: "void" },
        (_host, _items, size, align) => { emptyFrees.push({ size, align }); },
    );
    const introspection = new Uint8Array(24);
    const introspectionView = new DataView(introspection.buffer);
    introspectionView.setBigUint64(
        RUNTIME_INTROSPECTION_GET_BUNDLE_DESCRIPTOR_OFFSET,
        backend.pointerValue(descriptor.pointer),
        true,
    );
    introspectionView.setBigUint64(
        RUNTIME_INTROSPECTION_LIST_REGISTERED_GUEST_CONTRACTS_OFFSET,
        backend.pointerValue(emptyArray.pointer),
        true,
    );
    introspectionView.setBigUint64(
        RUNTIME_INTROSPECTION_GET_REGISTERED_CONTRACT_DESCRIPTOR_OFFSET,
        backend.pointerValue(contractDescriptor.pointer),
        true,
    );

    const emptyHost = new Uint8Array(HOST_API_SIZE);
    const emptyHostView = new DataView(emptyHost.buffer);
    emptyHostView.setBigUint64(HOST_API_LIST_BUNDLES_OFFSET, backend.pointerValue(emptyArray.pointer), true);
    emptyHostView.setBigUint64(HOST_API_FREE_OFFSET, backend.pointerValue(free.pointer), true);
    emptyHostView.setBigUint64(HOST_API_RESERVED_OFFSET, backend.pointerValue(backend.pointerOf(introspection)), true);
    const emptyRuntime = new Runtime(
        { symbols: { polyplug_runtime_destroy: () => true } } as never,
        backend.pointerOf(emptyHost),
    );

    const legacyHost = new Uint8Array(HOST_API_SIZE);
    const legacyRuntime = new Runtime(
        { symbols: { polyplug_runtime_destroy: () => true } } as never,
        backend.pointerOf(legacyHost),
    );

    try {
        const emptyBundles = emptyRuntime.bundleDescriptors();
        const emptyContracts = emptyRuntime.registeredContractDescriptors();
        const legacyBundles = legacyRuntime.bundleDescriptors();
        const legacyContracts = legacyRuntime.registeredContractDescriptors();
        assertEquals(emptyBundles, []);
        assertEquals(emptyContracts, []);
        assertEquals(legacyBundles, []);
        assertEquals(legacyContracts, []);
        assertEquals(Object.isFrozen(emptyBundles), true);
        assertEquals(Object.isFrozen(emptyContracts), true);
        assertEquals(Object.isFrozen(legacyBundles), true);
        assertEquals(Object.isFrozen(legacyContracts), true);
        assertEquals(emptyFrees, [
            { size: 0n, align: 1n },
            { size: 0n, align: 1n },
        ], "non-null empty ABI arrays must each be released exactly once");
        assertEquals(emptyRuntime.destroy(), true);
        assertEquals(legacyRuntime.destroy(), true);
    } finally {
        emptyRuntime.destroy();
        legacyRuntime.destroy();
        emptyArray.close();
        descriptor.close();
        contractDescriptor.close();
        free.close();
    }
});
