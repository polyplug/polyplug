# Deferred Items — Phase 17 Plan 02

## Pre-existing Test Failures

The following tests were failing BEFORE plan 17-02 changes and are unrelated to this plan:

- `ffi_edge_cases::test_find_all_guest_contracts_multiple_plugins` — loads plugin binary, fails with Generic error
- `ffi_edge_cases::test_find_all_guest_contracts_single_plugin` — loads plugin binary, fails with Generic error
- `ffi_edge_cases::test_resolve_plugin_stale_handle` — loads plugin binary, fails with Generic error

These tests require compiled plugin binaries that may not be present in the current build environment.
