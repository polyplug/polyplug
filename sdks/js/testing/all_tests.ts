// sdks/js/testing/all_tests.ts
// The explicit test manifest for the polyplug JS host SDK.
//
// Importing this module registers every host-SDK test into the harness registry
// for its side effect. The import list is EXPLICIT (not a runtime glob) so the
// same manifest drives the runner identically under Deno, Node, and Bun — the
// later increments reuse this file verbatim behind their own entrypoints.
//
// To add a test file, add a single side-effect import line here.

import "../host/tests/reload_notification_test.ts";
import "../host/tests/native_loader_test.ts";
import "../host/tests/signature_policy_config_test.ts";
import "../host/tests/host_contract_provider_test.ts";
import "../host/tests/reload_runtime_test.ts";
