import { openPolyplug, runtimeNew, guestContractId, NULL_HANDLE } from "../../sdks/js/host/mod.js";
import { registerNativeLoader } from "../../sdks/js/loaders/native/mod.ts";

const POLYPLUG_SO = Deno.env.get("POLYPLUG_SO") ?? "";
const TEST_PLUGIN_DIR = Deno.env.get("TEST_PLUGIN_DIR") ?? "";
// Compute the canonical guest contract ID rather than hardcoding it, so the
// fixture stays in sync with the runtime's FNV-1a contract-id scheme.
const TEST_ADD_CONTRACT_ID = guestContractId("test.add", 1);
const NATIVE_AVAILABLE = Deno.env.get("POLYPLUG_NATIVE_LIB") !== undefined;

if (!POLYPLUG_SO) {
    console.error("FATAL: POLYPLUG_SO not set - libpolyplug.so not built. Run: cargo build -p polyplug");
    Deno.exit(1);
}

let passed = 0;
let failed = 0;

function runTest(name: string, fn: () => void): void {
    try {
        fn();
        console.log(`  ok: ${name}`);
        passed++;
    } catch (e: unknown) {
        const msg = e instanceof Error ? e.message : String(e);
        console.error(`  FAILED: ${name}: ${msg}`);
        failed++;
    }
}

function skipUnlessNative(name: string, fn: () => void): void {
    if (NATIVE_AVAILABLE) {
        runTest(name, fn);
    } else {
        console.log(`  SKIP: ${name} (native loader not available)`);
    }
}

runTest("runtime_new_succeeds", () => {
    const lib = openPolyplug(POLYPLUG_SO);
    try {
        const rt = runtimeNew(lib);
        rt[Symbol.dispose]();
    } finally {
        lib.close();
    }
});

skipUnlessNative("load_bundle_succeeds", () => {
    const lib = openPolyplug(POLYPLUG_SO);
    try {
        const rt = runtimeNew(lib);
        try { registerNativeLoader(rt); rt.loadBundle(TEST_PLUGIN_DIR); }
        finally { rt[Symbol.dispose](); }
    } finally { lib.close(); }
});

skipUnlessNative("find_guest_contract_returns_valid_handle", () => {
    const lib = openPolyplug(POLYPLUG_SO);
    try {
        const rt = runtimeNew(lib);
        try {
            registerNativeLoader(rt);
            rt.loadBundle(TEST_PLUGIN_DIR);
            const handle = rt.findGuestContract(TEST_ADD_CONTRACT_ID);
            if (handle === NULL_HANDLE) throw new Error("Got NULL_HANDLE");
        } finally { rt[Symbol.dispose](); }
    } finally { lib.close(); }
});

skipUnlessNative("find_all_guest_contracts_24_byte_array_return", () => {
    // The ABI returns `Array` BY VALUE (items ptr + len + align = 24 bytes).
    // This exercises the real sret path: one matching handle expected, and the
    // host must stay functional afterwards (a narrowed return type corrupts
    // adjacent memory).
    const lib = openPolyplug(POLYPLUG_SO);
    try {
        const rt = runtimeNew(lib);
        try {
            registerNativeLoader(rt);
            rt.loadBundle(TEST_PLUGIN_DIR);
            const handles = rt.findAllGuestContracts(TEST_ADD_CONTRACT_ID);
            if (handles.length !== 1) throw new Error(`Expected 1 handle, got ${handles.length}`);
            const none = rt.findAllGuestContracts(0xDEADBEEFn);
            if (none.length !== 0) throw new Error(`Expected 0 handles, got ${none.length}`);
            const handle = rt.findGuestContract(TEST_ADD_CONTRACT_ID);
            if (handle === NULL_HANDLE) throw new Error("host corrupted after find_all sret");
        } finally { rt[Symbol.dispose](); }
    } finally { lib.close(); }
});

skipUnlessNative("resolve_guest_contract_returns_interface", () => {
    const lib = openPolyplug(POLYPLUG_SO);
    try {
        const rt = runtimeNew(lib);
        try {
            registerNativeLoader(rt);
            rt.loadBundle(TEST_PLUGIN_DIR);
            const handle = rt.findGuestContract(TEST_ADD_CONTRACT_ID);
            const iface = rt.resolveGuestContract(handle);
            if (iface === null) throw new Error("interface is null");
        } finally { rt[Symbol.dispose](); }
    } finally { lib.close(); }
});

runTest("null_handle_for_missing_contract", () => {
    const lib = openPolyplug(POLYPLUG_SO);
    try {
        const rt = runtimeNew(lib);
        try {
            const handle = rt.findGuestContract(0n);
            if (handle !== NULL_HANDLE) throw new Error(`Expected NULL_HANDLE, got ${handle}`);
        } finally { rt[Symbol.dispose](); }
    } finally { lib.close(); }
});

runTest("last_error_after_failed_load", () => {
    const lib = openPolyplug(POLYPLUG_SO);
    try {
        const rt = runtimeNew(lib);
        try {
            let threw = false;
            try { rt.loadBundle("/does/not/exist"); }
            catch (_e) { threw = true; }
            if (!threw) throw new Error("Expected loadBundle to throw for invalid path");
        } finally { rt[Symbol.dispose](); }
    } finally { lib.close(); }
});

console.log(`\nResults: ${passed} passed, ${failed} failed`);
if (failed > 0) Deno.exit(1);
