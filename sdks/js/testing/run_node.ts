// sdks/js/testing/run_node.ts
// The Node.js entrypoint for the runtime-agnostic JS SDK test suite.
//
// It imports the explicit manifest (registering every test for its side effect),
// runs them through the SAME shared harness as the Deno entrypoint, and sets the
// Node process exit code from the result so CI fails on any test failure. It
// reuses ./all_tests.ts + runRegisteredTests() verbatim — the only difference
// from run_deno.ts is the runtime-native exit call.
//
// Node runs the TypeScript test manifest through tsx. The bare `@polyplug/*`
// specifiers resolve through the sdks/js npm workspace; the koffi FFI backend is
// selected automatically by getBackend() (see abi/ffi/index.ts). Run from repo root:
//   cargo build --release -p polyplug -p polyplug_native -p polyplug_js
//   bash tests/fixtures/build_all.sh
//   (cd sdks/js && npm install)
//   POLYPLUG_LIB=$PWD/target/release/libpolyplug.so \
//   POLYPLUG_NATIVE_LIB=$PWD/target/release/libpolyplug_native.so \
//   POLYPLUG_JS_LIB=$PWD/target/release/libpolyplug_js.so \
//   node --import ./sdks/js/node_modules/tsx/dist/loader.mjs \
//     --conditions=polyplug-src sdks/js/testing/run_node.ts

import "./all_tests.ts";
import { runRegisteredTests, type TestRunResult } from "./harness.ts";

const result: TestRunResult = await runRegisteredTests();
process.exit(result.failed === 0 ? 0 : 1);
