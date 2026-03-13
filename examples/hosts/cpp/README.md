# C++ Host Example

A simple C++ host that loads a polyplug bundle and calls a plugin function.

## Files

- `host.cpp` — minimal host: create runtime, load bundle, find plugin, call function, print result
- `main.cpp` — full pipeline host demonstrating all example contracts
- `Makefile` — builds both targets

## Build

```bash
make host
```

Requires `libpolyplug.so` in `target/debug/` (build with `cargo build` from the repo root).

## Run

```bash
./host [bundle_dir]
```

Default bundle: `examples/guests/rust/decoder` (relative to repo root).

```bash
# From repo root:
./examples/hosts/cpp/host examples/guests/rust/decoder
```

Expected output:

```
=== polyplug C++ host (simple example) ===
Bundle dir: examples/guests/rust/decoder
Bundle loaded.
Plugin found.
Vtable: contract_id=0x... functions=1
Result:
  name  = Alice
  value = hello
  count = 3
Done.
```

## API Used

| Step | API |
|------|-----|
| Create runtime | `polyplug_runtime_new()` |
| Load bundle | `polyplug_load_bundle(rt, path, len)` |
| Find by contract | `polyplug_rt_find_by_contract(rt, contract_id, min_version)` |
| Resolve vtable | `polyplug_rt_resolve_plugin(rt, handle)` → `polyplug_get_vtable(guard)` |
| Call function | `vtable->functions[fn_id](args, out)` |
| Cleanup | `polyplug_guard_free(guard)`, `polyplug_runtime_free(rt)` |

Contract IDs are computed at compile time via `polyplug::fnv1a_contract_id(name, major)` defined in `polyplug/abi.hpp`.
