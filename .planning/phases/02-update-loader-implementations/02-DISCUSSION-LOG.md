# Phase 2: Update Loader Implementations - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-04-03
**Phase:** 02-update-loader-implementations
**Areas discussed:** Error conversion pattern, Internal error handling, Hot-reload errors, Error strategy

---

## Error Conversion Pattern

| Option | Description | Selected |
|--------|-------------|----------|
| Follow NativeLoader pattern | Internal methods return local error, load()/reload() convert at boundary with .map_err() | |
| Direct conversion at each site | Each error site in load()/reload() constructs InitFailed directly | ✓ |

**User's choice:** Keep inline, and make the native inline too!
**Notes:** User wants inline error handling for all loaders, including removing `load_internal()` from NativeLoader.

---

## Internal Error Handling

| Option | Description | Selected |
|--------|-------------|----------|
| Add load_internal to all | Add load_internal() returning local error type to each loader | |
| Keep inline | Keep logic inline in load(), convert each error site individually | ✓ |

**User's choice:** Keep inline, and make the native inline too!
**Notes:** Consistency across all loaders; remove `load_internal()` from NativeLoader.

---

## Hot-Reload Errors

| Option | Description | Selected |
|--------|-------------|----------|
| Use HotReloadDisabled | All loaders return RuntimeError::HotReloadDisabled for unsupported hot-reload | ✓ |
| Use loader-specific error | Each loader uses its local error converted to InitFailed | |

**User's choice:** Use HotReloadDisabled (Recommended)
**Notes:** Consistency and clarity — this is a runtime configuration issue, not a loader-specific error.

---

## Error Strategy (Key Decision)

| Option | Description | Selected |
|--------|-------------|----------|
| Keep local error types | Keep PythonLoaderError, LuaLoaderError, etc. and convert to InitFailed with .to_string() | |
| No local error types | Remove local error types. Just use LoaderError::InitFailed with string messages directly | ✓ |

**User's choice:** No local error types (use strings directly)
**Notes:** Simpler approach — no need for intermediate error enums. Phase 1 error type definitions become obsolete and should be removed.

---

## Claude's Discretion

[List areas where user said "you decide" or deferred to Claude]

None — all decisions were explicitly made by user.

## Deferred Ideas

[Ideas mentioned during discussion that were noted for future phases]

None — discussion stayed within phase scope.