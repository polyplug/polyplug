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
| Rust (`sdks/rust/guest`) | raw slice (`core::slice::from_raw_parts`) | `to_str` / `strip_prefix` return **borrowed** `&str`; `split` returns `Vec<&str>` of borrows |
| C++ (`sdks/cpp/abi`) | `std::string_view` over `ptr/len` | `to_string_view` / `strip_prefix` / `split` are **borrowed** `string_view`s; `to_string`/`to_str` copy |
| C# (`sdks/csharp/abi`) | `unsafe Encoding.UTF8.GetString((byte*)ptr, len)` | materializes a managed `string` per call |
| Python (`sdks/python/polyplug_abi`) | `ctypes.cast` + bytes copy | materializes a Python `str` per call |
| Lua (`sdks/lua/abi`) | LuaJIT `ffi.string(ptr, len)` | materializes an interned Lua string per call |
| JS guest (`sdks/js/guest`, QuickJS) | loader `readBytes` bridge + UTF-8 decode | materializes a JS string per call |
| JS abi mirror (`sdks/js/abi`, Deno host) | `Deno.UnsafePointerView(...).getArrayBuffer` + `TextDecoder` | materializes a JS string per call; **throws** (never silently returns `""`) if no Deno FFI environment exists |

A helper found making an FFI round-trip into the runtime, or silently
returning empty on a readable view, is a defect.
