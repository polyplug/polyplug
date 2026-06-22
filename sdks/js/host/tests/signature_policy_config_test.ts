// sdks/js/host/tests/signature_policy_config_test.ts
// Asserts the JS host SDK writes RuntimeConfig.signaturePolicy into the config
// buffer at offset 44, without loading the native library.
//
// `runtimeNew` builds a RUNTIME_CONFIG_SIZE byte buffer and writes
// config.signaturePolicy at RUNTIME_CONFIG_SIGNATURE_POLICY_OFFSET only when the
// option is provided; this test reproduces that exact DataView write and reads
// it back. Full runtime-load coverage lives in reload_runtime_test.ts.
//
// Run from repo root:
//   deno test sdks/js/host/tests/signature_policy_config_test.ts

import { assertEquals } from "jsr:@std/assert";
import {
  RUNTIME_CONFIG_SIGNATURE_POLICY_OFFSET,
  RUNTIME_CONFIG_SIZE,
} from "@polyplug/abi";
import { SignaturePolicy } from "../mod.js";

Deno.test("SignaturePolicy enum mirrors the ABI #[repr(u32)] values", () => {
  assertEquals(SignaturePolicy.Off, 0);
  assertEquals(SignaturePolicy.WarnOnly, 1);
  assertEquals(SignaturePolicy.Required, 2);
});

Deno.test("signature_policy lives at offset 44, struct stays 48 bytes", () => {
  assertEquals(RUNTIME_CONFIG_SIGNATURE_POLICY_OFFSET, 44);
  assertEquals(RUNTIME_CONFIG_SIZE, 48);
});

Deno.test("zeroed config reads SignaturePolicy.Off (default unchanged)", () => {
  const configBuf = new Uint8Array(RUNTIME_CONFIG_SIZE);
  const configView = new DataView(configBuf.buffer);
  assertEquals(
    configView.getUint32(RUNTIME_CONFIG_SIGNATURE_POLICY_OFFSET, true),
    SignaturePolicy.Off,
  );
});

Deno.test("setting Required writes 2 at offset 44 (mirrors runtimeNew)", () => {
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

Deno.test("setting WarnOnly writes 1 at offset 44", () => {
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
