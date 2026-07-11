// sdks/js/testing/run_deno.ts
// The Deno entrypoint for the runtime-agnostic JS SDK test suite.
//
// It imports the explicit manifest (registering every test for its side effect),
// runs them through the shared harness, and sets the Deno process exit code from
// the result so CI fails on any test failure. Node and Bun entrypoints arrive in
// a later increment; they reuse the same manifest + runner.
//
// Run from repo root:
//   cargo build --release -p polyplug -p polyplug_native -p polyplug_js
//   bash tests/fixtures/build_all.sh
//   POLYPLUG_LIB=$PWD/target/release/libpolyplug.so \
//   POLYPLUG_NATIVE_LIB=$PWD/target/release/libpolyplug_native.so \
//   POLYPLUG_JS_LIB=$PWD/target/release/libpolyplug_js.so \
//   deno run --allow-ffi --allow-env --allow-read --allow-write \
//     sdks/js/testing/run_deno.ts

import "./all_tests.ts";
import { runRegisteredTests, type TestRunResult } from "./harness.ts";

const result: TestRunResult = await runRegisteredTests();
Deno.exit(result.failed === 0 ? 0 : 1);
