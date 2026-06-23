// sdks/js/host/tests/signature_policy_config_test.ts
// Asserts the JS host SDK writes RuntimeConfig.signaturePolicy (offset 44) and
// the trusted_keys Array (offset 48) into the config buffer, without loading the
// native library.
//
// `runtimeNew` builds a RUNTIME_CONFIG_SIZE byte buffer, writes
// config.signaturePolicy at RUNTIME_CONFIG_SIGNATURE_POLICY_OFFSET, and packs
// config.trustedKeys into a contiguous N*32-byte buffer whose pointer/len/align
// fill the trusted_keys Array — each only when the option is provided. These
// tests reproduce those exact DataView writes and read them back. Full
// runtime-load coverage lives in reload_runtime_test.ts.
//
// Registered into the runtime-agnostic harness; run the whole suite from repo
// root via the Deno entrypoint (or `just test-host-js`):
//   deno run --allow-ffi --allow-env --allow-read --allow-write \
//     sdks/js/testing/run_deno.ts

import { assertEquals, test } from "../../testing/harness.ts";
import {
    ED25519_PUBLIC_KEY_SIZE,
    getBackend,
    RUNTIME_CONFIG_SIGNATURE_POLICY_OFFSET,
    RUNTIME_CONFIG_SIZE,
    RUNTIME_CONFIG_TRUSTED_KEYS_ALIGN_OFFSET,
    RUNTIME_CONFIG_TRUSTED_KEYS_LEN_OFFSET,
    RUNTIME_CONFIG_TRUSTED_KEYS_OFFSET,
} from "@polyplug/abi";
import { SignaturePolicy } from "../mod.js";

test("SignaturePolicy enum mirrors the ABI #[repr(u32)] values", () => {
    assertEquals(SignaturePolicy.Off, 0);
    assertEquals(SignaturePolicy.WarnOnly, 1);
    assertEquals(SignaturePolicy.Required, 2);
});

test("signature_policy at offset 44, trusted_keys Array at 48, struct 72 bytes", () => {
    assertEquals(RUNTIME_CONFIG_SIGNATURE_POLICY_OFFSET, 44);
    assertEquals(RUNTIME_CONFIG_TRUSTED_KEYS_OFFSET, 48);
    assertEquals(RUNTIME_CONFIG_TRUSTED_KEYS_LEN_OFFSET, 56);
    assertEquals(RUNTIME_CONFIG_TRUSTED_KEYS_ALIGN_OFFSET, 64);
    assertEquals(RUNTIME_CONFIG_SIZE, 72);
});

test("zeroed config reads SignaturePolicy.Off (default unchanged)", () => {
    const configBuf = new Uint8Array(RUNTIME_CONFIG_SIZE);
    const configView = new DataView(configBuf.buffer);
    assertEquals(
        configView.getUint32(RUNTIME_CONFIG_SIGNATURE_POLICY_OFFSET, true),
        SignaturePolicy.Off,
    );
});

test("setting Required writes 2 at offset 44 (mirrors runtimeNew)", () => {
    const configBuf = new Uint8Array(RUNTIME_CONFIG_SIZE);
    const configView = new DataView(configBuf.buffer);
    configView.setUint32(
        RUNTIME_CONFIG_SIGNATURE_POLICY_OFFSET,
        SignaturePolicy.Required,
        true,
    );
    assertEquals(
        configView.getUint32(RUNTIME_CONFIG_SIGNATURE_POLICY_OFFSET, true),
        2,
    );
});

test("setting WarnOnly writes 1 at offset 44", () => {
    const configBuf = new Uint8Array(RUNTIME_CONFIG_SIZE);
    const configView = new DataView(configBuf.buffer);
    configView.setUint32(
        RUNTIME_CONFIG_SIGNATURE_POLICY_OFFSET,
        SignaturePolicy.WarnOnly,
        true,
    );
    assertEquals(
        configView.getUint32(RUNTIME_CONFIG_SIGNATURE_POLICY_OFFSET, true),
        1,
    );
});

test("zeroed config reads empty trusted_keys (TOFU default)", () => {
    const configBuf = new Uint8Array(RUNTIME_CONFIG_SIZE);
    const configView = new DataView(configBuf.buffer);
    assertEquals(configView.getBigUint64(RUNTIME_CONFIG_TRUSTED_KEYS_OFFSET, true), 0n);
    assertEquals(configView.getBigUint64(RUNTIME_CONFIG_TRUSTED_KEYS_LEN_OFFSET, true), 0n);
    assertEquals(configView.getBigUint64(RUNTIME_CONFIG_TRUSTED_KEYS_ALIGN_OFFSET, true), 0n);
});

test("trustedKeys([k1, k2]) marshals non-null ptr, len 2, align 1", () => {
    // Mirrors runtimeNew's trusted_keys marshalling without loading the native
    // library: pack two 32-byte keys into one contiguous buffer through the FFI
    // seam, write the trusted_keys Array fields, and read them back.
    const ffi = getBackend();
    const k1 = new Uint8Array(ED25519_PUBLIC_KEY_SIZE).fill(0x11);
    const k2 = new Uint8Array(ED25519_PUBLIC_KEY_SIZE).fill(0x22);
    const keys = [k1, k2];

    const keysBuf = new Uint8Array(keys.length * ED25519_PUBLIC_KEY_SIZE);
    keysBuf.set(k1, 0);
    keysBuf.set(k2, ED25519_PUBLIC_KEY_SIZE);

    const configBuf = new Uint8Array(RUNTIME_CONFIG_SIZE);
    const configView = new DataView(configBuf.buffer);
    configView.setBigUint64(
        RUNTIME_CONFIG_TRUSTED_KEYS_OFFSET,
        ffi.pointerValue(ffi.pointerOf(keysBuf)),
        true,
    );
    configView.setBigUint64(RUNTIME_CONFIG_TRUSTED_KEYS_LEN_OFFSET, BigInt(keys.length), true);
    configView.setBigUint64(RUNTIME_CONFIG_TRUSTED_KEYS_ALIGN_OFFSET, 1n, true);

    // items ptr is non-null, len == 2, align == 1 (Ed25519PublicKey is align 1).
    const itemsPtr = configView.getBigUint64(RUNTIME_CONFIG_TRUSTED_KEYS_OFFSET, true);
    assertEquals(itemsPtr !== 0n, true);
    assertEquals(configView.getBigUint64(RUNTIME_CONFIG_TRUSTED_KEYS_LEN_OFFSET, true), 2n);
    assertEquals(configView.getBigUint64(RUNTIME_CONFIG_TRUSTED_KEYS_ALIGN_OFFSET, true), 1n);
    // The contiguous buffer holds key1 then key2 at the 32-byte stride.
    assertEquals(keysBuf.length, 2 * ED25519_PUBLIC_KEY_SIZE);
    assertEquals(keysBuf[0], 0x11);
    assertEquals(keysBuf[ED25519_PUBLIC_KEY_SIZE], 0x22);
});
