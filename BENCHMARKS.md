# polyplug Benchmark Results

## Methodology

- Tool: criterion 0.8 (https://docs.rs/criterion)
- Platform: Linux 6.19.5 x86_64 / AMD Ryzen 9 5900X 12-Core / 32 GiB RAM
- Rust toolchain: rustc 1.93.1 (01f6ddf75 2026-02-11)
- Optimization: `--release` (criterion default)
- Iterations: criterion auto-selects based on measurement time (100 samples each)

## Results

| Benchmark | Mean (ns) | Std Dev (ns) | Notes | Epic 6 Baseline |
|-----------|-----------|--------------|-------|-----------------|
| dispatch/noop | 1.97 | 0.007 | Pure vtable dispatch, add(0,0) — minimal computation | YES |
| dispatch/buffer_arg | 35.46 | 0.12 | 4096-byte buffer fill + dispatch — measures dispatch + memory write | YES |
| dispatch/struct_arg_and_return | 1.97 | 0.010 | AddArgs struct in, u32 out — dominant real-world path | YES |
| dispatch/cross_plugin | 39.71 | 0.22 | Full dispatcher chain: TLS Registry.find + Registry.resolve + dispatch | YES |

## Interpretation

- `dispatch/noop` and `dispatch/struct_arg_and_return` are both ~1.97 ns — establishes pure ABI overhead
- `dispatch/cross_plugin` (~39.71 ns) minus `dispatch/noop` (~1.97 ns) = **~37.7 ns cross-plugin indirection cost** (TLS lookup + HashMap find + slot resolve)
- `dispatch/buffer_arg` (~35.46 ns) is dominated by the 4096-byte memory write, not dispatch overhead
- Future epics: add new rows as new dispatch paths are introduced; compare against Epic 6 Baseline column

## Epic History

| Epic | Date | Notes |
|------|------|-------|
| Epic 6 | 2026-03-08 | Initial baseline — vtable dispatch, memory model, error model hardening |
