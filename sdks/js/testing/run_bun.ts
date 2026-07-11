// sdks/js/testing/run_bun.ts
// The Bun entrypoint for the runtime-agnostic JS SDK test suite.
//
// It imports the explicit manifest (registering every test for its side effect),
// runs them through the SAME shared harness as the Deno and Node entrypoints, and
// sets the Bun process exit code from the result so CI fails on any test failure.
// It reuses ./all_tests.ts + runRegisteredTests() verbatim — the only difference
// from run_node.ts is nothing but the comment: Bun, like Node, exits via
// process.exit.
//
// Bun transpiles TypeScript natively (including `const enum`, which the generated
// abi.ts uses), so this runs as `.ts` directly with no transpile step (no tsx).
// The bare `@polyplug/*` specifiers resolve through the sdks/js npm workspace via
// the `polyplug-src` export condition (`bun --conditions=polyplug-src`); the
// bun:ffi FFI backend is selected automatically by getBackend() (see
// abi/ffi/index.ts). Run from repo root:
//   cargo build --release -p polyplug -p polyplug_native -p polyplug_js
//   bash tests/fixtures/build_all.sh
//   (cd sdks/js && bun install)
//   POLYPLUG_LIB=$PWD/target/release/libpolyplug.so \
//   POLYPLUG_NATIVE_LIB=$PWD/target/release/libpolyplug_native.so \
//   POLYPLUG_JS_LIB=$PWD/target/release/libpolyplug_js.so \
//   bun --conditions=polyplug-src sdks/js/testing/run_bun.ts

import "./all_tests.ts";
import { runRegisteredTests, type TestRunResult } from "./harness.ts";

const result: TestRunResult = await runRegisteredTests();
process.exit(result.failed === 0 ? 0 : 1);
