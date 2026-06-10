# SDK Helpers — Golden Contract

`sdk_validator.yaml` is the single source of truth for the helper *set* (which
methods every language must implement, and the one file per language where they
live). This document pins the *semantics* every implementation must follow.
The validator gate: `cargo run -p sdk-validator -- --config sdk_validator.yaml --fail-on-missing`.

## Method Contracts (StringView)

All helpers operate on a borrowed `StringView { ptr, len }` (UTF-8, never
null-terminated). "Null view" = null `ptr` OR `len == 0`.

| Method | Contract |
|---|---|
| `to_str` | Decode the viewed bytes as UTF-8. Null view → `""`. Never raises for a null view. |
| `starts_with` | `to_str(sv)` starts with `prefix`. Empty prefix → `true`. Null view + non-empty prefix → `false`. |
| `ends_with` | `to_str(sv)` ends with `suffix`. Empty suffix → `true`. Null view + non-empty suffix → `false`. |
| `strip_prefix` | Returns the string with `prefix` removed if present, otherwise the original string. Never raises. |
| `split` | See split rules below. |

## Split Rules (all languages, identical)

`split(view, delimiter)` — the delimiter is a **literal string**. Never a Lua
pattern, never a regex, never a single char.

1. Null or zero-length view → **empty list** (`[]`).
2. Empty delimiter (`""`) → **single-element list containing the whole string** (`[s]`).
3. Otherwise: split on **every** occurrence, **keeping empty segments**:
   - `"a||b"` with `"|"` → `["a", "", "b"]`
   - `"|a|"` with `"|"` → `["", "a", ""]`
   - `"a::b::c"` with `"::"` → `["a", "b", "c"]`

## Hot-Path Guarantee (invariant)

Validated helpers **never call into `libpolyplug` or any `HostApi` function
pointer**. The logic runs natively in each language; the only memory access is
the language's native read primitive:

| Language | Read primitive | Allocation behaviour |
|---|---|---|
| Rust (`sdks/rust/guest`) | raw slice (`core::slice::from_raw_parts`) | `to_str` / `strip_prefix` return **borrowed** `&str`; `split` returns `Vec<&str>` of borrows |
| C++ (`sdks/cpp/abi`) | `std::string_view` over `ptr/len` | `to_string_view` / `strip_prefix` / `split` are **borrowed** `string_view`s; `to_string`/`to_str` copy |
| C# (`sdks/csharp/abi`) | `unsafe Encoding.UTF8.GetString((byte*)ptr, len)` | materializes a managed `string` per call |
| Python (`sdks/python/polyplug_abi`) | `ctypes.cast` + bytes copy | materializes a Python `str` per call |
| Lua (`sdks/lua/abi`) | LuaJIT `ffi.string(ptr, len)` | materializes an interned Lua string per call |
| JS guest (`sdks/js/guest`, QuickJS) | loader `readBytes` bridge + UTF-8 decode | materializes a JS string per call |
| JS abi mirror (`sdks/js/abi`, Deno host) | `Deno.UnsafePointerView(...).getArrayBuffer` + `TextDecoder` | materializes a JS string per call; **throws** (never silently returns `""`) if no Deno FFI environment exists |

A helper found making an FFI round-trip into the runtime, or silently
returning empty on a readable view, is a defect.
