---
phase: 10-sdk-cleanup-completion
plan: 02
type: execute
wave: 2
depends_on: [10-01]
status: completed
completed_at: 2026-04-06
---

## Summary: HostVTable → RuntimeAbi Naming Fixes

**Objective:** Fix HostVTable → RuntimeAbi naming in C++ guest SDK and C# test file to match polyplug_abi naming convention.

### Completed Tasks

| Task | Description | Status |
|------|-------------|--------|
| 1 | Update C++ guest.hpp to use RuntimeAbi naming | ✓ Complete |
| 2 | Update C# AbiSizeTests.cs to use RuntimeAbi naming | ✓ Complete |
| 3 | Update SDK build extractor to use RuntimeAbi | ✓ Complete |

### What Was Built

Updated naming to match polyplug_abi RuntimeAbi type across SDK files:

1. **C++ guest SDK** (`sdks/cpp/guest/polyplug/guest.hpp`):
   - Section comment renamed: "Host VTable Storage" → "RuntimeAbi Storage"
   - All function signatures updated: `HostVTable*` → `RuntimeAbi*`
   - Backward compatibility typedef added: `using HostVTable = RuntimeAbi`

2. **C# ABI size tests** (`sdks/csharp/guest/AbiSizeTests.cs`):
   - Comment updated: "HostVTable: 8 x..." → "RuntimeAbi: 8 x..."
   - Marshal.SizeOf call updated: `<HostVTable>` → `<RuntimeAbi>`
   - Error message updated for RuntimeAbi naming

3. **SDK build extractor** (`crates/polyplug_abi/build/extractor.rs`):
   - ABI_TYPES array updated: `"HostVTable"` → `"RuntimeAbi"`
   - Enables correct SDK generation from polyplug_abi source

### Key Files Modified

- `sdks/cpp/guest/polyplug/guest.hpp` — C++ guest SDK runtime ABI storage
- `sdks/csharp/guest/AbiSizeTests.cs` — C# ABI size verification tests
- `crates/polyplug_abi/build/extractor.rs` — ABI type extraction for SDK generation

### Verification Results

- C++ guest.hpp contains 6 RuntimeAbi references ✓
- C++ backward compatibility typedef present: `using HostVTable = RuntimeAbi` ✓
- C# AbiSizeTests.cs contains 3 RuntimeAbi references ✓
- SDK build extractor has RuntimeAbi, no HostVTable in ABI_TYPES ✓

### Deviations

None - all tasks completed as planned. Backward compatibility maintained via typedef matching Rust SDK pattern.

---
*Completed: 2026-04-06*