# polyplug Deno Host Library

`Deno.dlopen` host library for the polyplug plugin runtime.

## Prerequisites

- **Deno** 1.38+ (for `Deno.dlopen` and `[Symbol.dispose]` support)
- Permissions: `--allow-ffi --allow-env --allow-read`
- A compiled `libpolyplug.so` shared library

## Quick Start

```typescript
import { openPolyplug, runtimeNew, NULL_HANDLE } from "./polyplug.ts";

// 1. Open the shared library (must use full absolute path)
const lib = openPolyplug("/usr/local/lib/libpolyplug.so");

try {
  // 2. Create a runtime
  const rt = runtimeNew(lib);
  try {
    // 3. Load a plugin bundle
    rt.loadBundle("/path/to/my/plugin_bundle");

    // 4. Find a plugin by contract ID (BigInt for u64 precision)
    const CONTRACT_ID = 0xCC4232FAB0410D2Bn;
    const handle = rt.findByContract(CONTRACT_ID);

    // 5. Check if found
    if (handle === NULL_HANDLE) {
      throw new Error("plugin not found");
    }

    // 6. Resolve to a guard and get vtable
    const guard = rt.resolvePlugin(handle);
    try {
      const vtable = guard.vtable();
      // Use vtable to call plugin functions
    } finally {
      guard[Symbol.dispose]();
    }
  } finally {
    rt[Symbol.dispose]();
  }
} finally {
  lib.close();
}
```

## Handle Encoding

Plugin handles are encoded as `bigint` (`u64`) values. The sentinel for "not found" is:

```typescript
export const NULL_HANDLE = 0xFFFFFFFFFFFFFFFFn;  // u64::MAX
```

**Important**: Always use `bigint` (not `number`) for contract IDs, bundle IDs, and packed handles. JavaScript `number` is a 64-bit float and loses precision for integers above 2^53. The `n` suffix creates a BigInt literal that handles the full 64-bit range.

```typescript
// Correct: BigInt literal
const contractId = 0xCC4232FAB0410D2Bn;

// Wrong: plain number (loses precision for large IDs)
// const contractId = 0xCC4232FAB0410D2B;  // may be rounded!
```

## using keyword / [Symbol.dispose] Cleanup

The `Runtime` and `Guard` classes implement `[Symbol.dispose]()` for use with the `using` keyword (Explicit Resource Management proposal, Deno 1.38+):

```typescript
// Automatic cleanup with 'using'
using rt = runtimeNew(lib);
// rt.[Symbol.dispose]() called automatically at end of scope

// Manual cleanup (equivalent)
const rt = runtimeNew(lib);
rt[Symbol.dispose]();
```

Always call `lib.close()` after you're done to release the shared library handle.

## Running Tests

```bash
# Set required environment variables
export POLYPLUG_SO=/path/to/libpolyplug.so
export TEST_PLUGIN_DIR=/path/to/test_plugin_dir

# Run the test suite
deno test --allow-ffi --allow-env --allow-read polyplug_test.ts
```

The tests require:
- `POLYPLUG_SO`: Absolute path to the compiled `libpolyplug.so`
- `TEST_PLUGIN_DIR`: Path to a directory containing a valid polyplug plugin bundle with the `test.add@1` contract
