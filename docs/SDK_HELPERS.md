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
| `to_str` | Decode the viewed bytes as UTF-8. Null view → `""` (never errors — null is empty, not invalid). A non-null view whose bytes are NOT valid UTF-8 **errors** — see "Invalid UTF-8" below. Never silently substitutes `""` or replacement characters for a readable-but-invalid view. |
| `starts_with` | `to_str(sv)` starts with `prefix`. Empty prefix → `true`. Null view + non-empty prefix → `false`. Errors on invalid UTF-8 (decodes via `to_str`). |
| `ends_with` | `to_str(sv)` ends with `suffix`. Empty suffix → `true`. Null view + non-empty suffix → `false`. Errors on invalid UTF-8 (decodes via `to_str`). |
| `strip_prefix` | Returns the string with `prefix` removed if present, otherwise the original string. Errors on invalid UTF-8 (decodes via `to_str`). |
| `split` | See split rules below. Errors on invalid UTF-8 (decodes via `to_str`). |

### Invalid UTF-8 (all languages)

A readable view (non-null `ptr`, `len > 0`) whose bytes are not well-formed
UTF-8 — rejecting overlong forms, surrogates, and code points above U+10FFFF,
matching Rust's `core::str::from_utf8` — must surface an error, never silently
yield `""`, U+FFFD replacement characters, or raw mojibake. The error is a
**local helper error in the language's idiom**, NOT the ABI out-param channel:

| Language | Mechanism on invalid UTF-8 |
|---|---|
| Rust (`sdks/rust/guest`) | `to_str` and the derived helpers return `Result<_, GuestError>` (`Err` on invalid). A panic is not used — unwinding across the `extern "C"` guest boundary is UB. |
| Python (`sdks/python/polyplug_abi`) | `bytes.decode("utf-8")` raises `UnicodeDecodeError`. |
| C# (`sdks/csharp/abi`) | A strict `UTF8Encoding(throwOnInvalidBytes: true)` throws `DecoderFallbackException`. |
| C++ (`sdks/cpp/abi`) | `require_utf8` throws `std::runtime_error`. `to_string_view` is the explicit raw byte primitive and does NOT validate. |
| Lua (`sdks/lua/abi`) | A manual UTF-8 scan (LuaJIT 5.1 has no `utf8` library) calls `error(...)`. |
| JS (`sdks/js/abi` Deno, `sdks/js/guest` QuickJS) | A fatal `TextDecoder('utf-8', {fatal:true})` or the validating manual decoder throws a `TypeError`. |

Runtime proof: `examples/verify_to_str_errors.sh` (`just verify-to-str-errors`)
executes every SDK's real `to_str` against an invalid view and a valid view,
asserting the invalid one errors and the valid one decodes.

## Contract / Bundle ID Contracts (ContractId)

Every language SDK exposes the canonical FNV-1a 64-bit ID scheme so authors
compute IDs through the SDK instead of hand-writing hash literals. The Rust
authority is `crates/polyplug_utils`; all mirrors must produce **byte-identical**
`u64` results (a divergence means a plugin built in one language cannot be
resolved by a host in another).

| Method | Contract |
|---|---|
| `fnv1a_64` | FNV-1a 64-bit over the UTF-8 bytes of the input. Offset basis `0xcbf29ce484222325`, prime `0x100000001b3`, wrap mod 2^64. Empty input → the offset basis. |
| `bundle_id` | `fnv1a_64(name)`. |
| `guest_contract_id` | `fnv1a_64("guest_contract:" + name + "@" + major)`. |
| `host_contract_id` | `fnv1a_64("host_contract:" + name + "@" + major)`. The distinct prefix guarantees host and guest IDs never collide for the same `name@major`. |

The helper lives in each language's canonical, idiomatic home (the validated
`method_targets` in `sdk_validator.yaml`), not necessarily the `abi/` mirror:

| Language | Home | Names |
|---|---|---|
| Rust | `crates/polyplug_utils/src/lib.rs` | `fnv1a_64`, `bundle_id`, `guest_contract_id`, `host_contract_id` |
| Python | `sdks/python/abi/abi.py` | snake_case (same) |
| Lua | `sdks/lua/abi/abi.lua` | `M.fnv1a_64`, … (returns `uint64_t` cdata) |
| C# | `sdks/csharp/abi/Abi.cs` (`ContractId` class) | `Fnv1a64`, `BundleId`, `GuestContractId`, `HostContractId` (`ulong`) |
| C++ | `sdks/cpp/host/polyplug/id.hpp` | snake_case, **`constexpr`** (compile-time IDs) |
| JS | `sdks/js/host/polyplug/mod.js` | `fnv1a64`, `bundleId`, `guestContractId`, `hostContractId` (`bigint`) |

The C# / Python / Lua names are merged into the ABI mirror by
`crates/polyplug_abi/build/generate.rs`; Rust, C++, and JS keep their pre-existing
idiomatic implementations. Cross-language byte-parity is proven by
`examples/verify_id_helpers.sh` (`just verify-id-helpers`), which runs every
SDK against the Rust golden vectors.

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
| Rust (`sdks/rust/guest`) | raw slice (`core::slice::from_raw_parts`) | `to_str` / `strip_prefix` return **borrowed** `Result<&str, GuestError>`; `split` returns `Result<Vec<&str>, GuestError>` of borrows (UTF-8 validation is a scan, not a copy — the borrow is preserved) |
| C++ (`sdks/cpp/abi`) | `std::string_view` over `ptr/len` | `strip_prefix` / `split` are **borrowed** `string_view`s (now validating — a scan, not a copy); `to_string_view` is the raw non-validating primitive; `to_string`/`to_str` copy |
| C# (`sdks/csharp/abi`) | `unsafe s_strictUtf8.GetString((byte*)ptr, len)` (throwing decoder) | materializes a managed `string` per call |
| Python (`sdks/python/polyplug_abi`) | `ctypes.cast` + bytes copy | materializes a Python `str` per call |
| Lua (`sdks/lua/abi`) | LuaJIT `ffi.string(ptr, len)` | materializes an interned Lua string per call |
| JS guest (`sdks/js/guest`, QuickJS) | loader `readBytes` bridge + UTF-8 decode | materializes a JS string per call |
| JS abi mirror (`sdks/js/abi`, Deno host) | `Deno.UnsafePointerView(...).getArrayBuffer` + `TextDecoder` | materializes a JS string per call; **throws** (never silently returns `""`) if no Deno FFI environment exists |

A helper found making an FFI round-trip into the runtime, or silently
returning empty / lossy-decoding (U+FFFD) on a readable-but-invalid view
instead of erroring, is a defect.
