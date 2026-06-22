// sdks/js/host/tests/native_loader_test.ts
// Unit tests for native-loader platform resolution (SDK-side only, no native runtime).
//
// Registered into the runtime-agnostic harness; run the whole suite from repo
// root via the Deno entrypoint (or `just test-host-js`):
//   deno run --allow-ffi --allow-env --allow-read --allow-write \
//     sdks/js/testing/run_deno.ts

import { assertEquals, assertThrows, test } from "../../testing/harness.ts";
import { nativeLibraryFilenameFor, platformFor } from "../polyplug/native-loader.ts";

test("platformFor maps Deno OS names to staged platform directories", () => {
    assertEquals(platformFor("linux", "x86_64"), "linux-x64");
    assertEquals(platformFor("darwin", "x86_64"), "macos-x64");
    assertEquals(platformFor("darwin", "aarch64"), "macos-arm64");
    assertEquals(platformFor("windows", "x86_64"), "windows-x64");
});

test("platformFor rejects unsupported OS and architecture", () => {
    assertThrows(() => platformFor("freebsd", "x86_64"), Error, "Unsupported OS");
    assertThrows(() => platformFor("linux", "riscv64"), Error, "Unsupported architecture");
});

test("nativeLibraryFilenameFor returns the per-OS library filename", () => {
    assertEquals(nativeLibraryFilenameFor("linux"), "libpolyplug.so");
    assertEquals(nativeLibraryFilenameFor("darwin"), "libpolyplug.dylib");
    assertEquals(nativeLibraryFilenameFor("windows"), "polyplug.dll");
});

test("nativeLibraryFilenameFor rejects unsupported OS", () => {
    assertThrows(() => nativeLibraryFilenameFor("freebsd"), Error, "Unsupported OS");
});
