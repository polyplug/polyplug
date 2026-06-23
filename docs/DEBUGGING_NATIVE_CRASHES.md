# Debugging Native Crashes

polyplug runs plugins **in-process, with the host's privileges, and no sandbox** (see
the [Trust Model](TRUST_MODEL.md)). That is the right model for *trusted* plugins, and it
has a direct operational consequence: **a defect in a plugin can crash the host
process.** There is no fault isolation boundary to catch a wild pointer write, a stack
smash, or a foreign-memory free inside a loaded bundle.

This guide is the field manual for diagnosing those crashes in a trusted-plugin
deployment — what crashes look like, how to build for debuggability, and how to extract a
symbolicated backtrace from a live crash, a core dump, or a sanitizer run, across all six
plugin languages.

> If you are evaluating *whether* in-process is acceptable for your threat model, read
> the [Trust Model](TRUST_MODEL.md) first — this guide assumes you have already chosen the
> trusted-plugin path.

---

## 1. The crash model — what can and cannot crash the host

Not everything that goes wrong is a crash. polyplug deliberately converts most failure
modes into typed errors and reserves a process abort for genuinely unrecoverable plugin
defects. Knowing which bucket you are in saves hours.

| Failure | How it surfaces | Crash? |
|---|---|---|
| Bug in polyplug's own `polyplug_runtime_create` / `_destroy` | null / no-op return + recorded `last_error` (the two C ABI exports are `catch_unwind`-wrapped) | **No** |
| ABI / contract version mismatch | bundle rejected at load with a clear `LoaderError` | **No** |
| Bundle tampered / unsigned under `SignaturePolicy::Required` | `LoaderError` at load (see [Trust Model § Bundle Signing](TRUST_MODEL.md#bundle-signing)) | **No** |
| Per-call arena exhausted | `ArenaOverflow` error returned to the caller | **No** |
| A guest panic / C++ exception / managed exception **escaping its generated glue** | **process abort** — `panic = "abort"` is intentionally never set, so each FFI boundary `catch_unwind`s, but an escaped unwind across `extern "C"` is UB and the boundary aborts deterministically instead | **Yes (by design)** |
| Plugin writes a wild pointer / smashes the stack / double-frees host memory | `SIGSEGV` / `SIGABRT` / `SIGBUS` | **Yes** |
| Using a cached raw interface pointer **after** its bundle was unloaded or reloaded without honoring the quiesce contract | use-after-free → likely `SIGSEGV` (documented UB) | **Yes** |

The two rows at the bottom are what this guide is about. Everything above them is a
typed error you handle in host code — if one of *those* manifests as a crash instead, you
have found a runtime bug worth [reporting](security-policy.md).

### Why a plugin crash aborts deterministically

polyplug never sets `panic = "abort"` globally; instead every FFI entry/exit point wraps
guest calls in `catch_unwind`. A well-behaved guest that returns an error code never
unwinds across the boundary. A guest that lets a panic/exception propagate *into* the
`extern "C"` frame triggers the boundary's abort path — a defined, immediate process exit
rather than undefined unwinding through foreign frames. So "my plugin crashed the host" is
almost always one of: (a) an escaped unwind, or (b) real memory unsafety in the plugin.
The rest of this guide tells the two apart.

---

## 2. Build for debuggability

A release binary with no symbols yields a backtrace of bare addresses. Fix that *before*
you need it — debuginfo costs nothing at runtime and can be shipped separately from the
binary.

Add debug symbols to your **release** profile (host application `Cargo.toml`):

```toml
[profile.release]
debug = 1            # line tables — enough for symbolicated backtraces
split-debuginfo = "packed"   # emit a separate .dwp / .dSYM you can archive per release
```

- `debug = 1` keeps optimizations on but emits line-number info. Use `debug = 2` for full
  variable info when you are actively chasing a bug.
- `split-debuginfo = "packed"` (Linux/macOS) keeps the shipped binary lean while letting
  you archive the symbol file alongside each release — load it later to symbolicate a core
  dump from the field.
- Always run the host with `RUST_BACKTRACE=1` (or `full`) in production. It is free until
  something panics.

For **C++ / C# / native guest bundles**, build the plugin's `cdylib` with debug info too
(`-g` for C++, `<DebugType>portable</DebugType>` for .NET) and archive the symbols next to
the bundle. A symbolicated host stack that dead-ends at `??` inside a plugin's `.so` means
*that* bundle was stripped.

---

## 3. Getting a backtrace from a live crash

### Rust panic / boundary abort

`RUST_BACKTRACE=full ./your-host` prints the unwinding stack at the point of a panic or a
boundary abort. The frame naming the `extern "C"` trampoline (`polyplug_*` or a generated
`*_dispatch`) is the boundary; the frames *above* it are inside the guest.

### Attaching a debugger

```bash
# Linux
gdb --args ./your-host <args>
(gdb) run
# ... crash ...
(gdb) bt full          # full backtrace with locals
(gdb) info sharedlibrary   # confirm the plugin .so is mapped + symbolicated

# macOS
lldb -- ./your-host <args>
(lldb) run
(lldb) bt all          # all threads — essential for the concurrent unload/reload paths
```

For a stack of bare addresses (stripped binary, archived symbols), symbolicate offline:

```bash
addr2line -e ./your-host-or-plugin.so -f -C 0x<address>
```

### Multi-threaded crashes

polyplug's read path is lock-free and its unload/reload paths run concurrently with
readers. A crash there is rarely on the faulting thread alone — always capture **all**
threads (`thread apply all bt` in gdb, `bt all` in lldb). A reader thread faulting on a
freed interface while another thread is mid-unload is the signature of a violated
quiesce-before-unload contract (see §6).

---

## 4. Core dumps

A core dump turns a one-off field crash into something you can open at your desk with the
archived symbols.

### Linux

```bash
ulimit -c unlimited                       # allow cores for this shell/service
cat /proc/sys/kernel/core_pattern         # where cores go (often piped to systemd-coredump)

# If systemd-coredump is active:
coredumpctl list
coredumpctl gdb <PID|exe>                 # opens the core in gdb with symbols resolved
```

Open a raw core manually:

```bash
gdb ./your-host /path/to/core
(gdb) bt full
```

For a service, set `LimitCORE=infinity` in the systemd unit so production crashes are
captured automatically.

### macOS

Cores land in `/cores` once enabled:

```bash
sudo sysctl kern.coredump=1
ulimit -c unlimited
lldb ./your-host -c /cores/core.<PID>
(lldb) bt all
```

(A System Integrity Protection–enabled Mac may also need an entitlement for the target.)

### Windows

The OS Windows Error Reporting path writes a `.dmp`. Configure it via the
`LocalDumps` registry key, then open the dump:

```
cdb -z C:\path\to\crash.dmp
0:000> !analyze -v
0:000> ~*k                 :: stacks for all threads
```

or load the `.dmp` in WinDbg / Visual Studio. Build the host and plugins with PDBs and
keep them in a symbol store so `!analyze` resolves frames.

---

## 5. Sanitizers and model checking

When a crash is intermittent — a data race, a use-after-free that only fires under load —
a single backtrace is not enough. Reach for the tool that targets the bug class. polyplug
runs all of these in CI (nightly), and you can run them against your own host + plugins.

| Tool | Catches | Run it |
|---|---|---|
| **AddressSanitizer** | use-after-free, buffer overflow, double-free across the unsafe boundary | `RUSTFLAGS=-Zsanitizer=address cargo +nightly test -Zbuild-std -p polyplug -p polyplug_abi -p polyplug_utils --target x86_64-unknown-linux-gnu --lib` |
| **ThreadSanitizer** | data races on the concurrent read / unload / reload paths | `RUSTFLAGS=-Zsanitizer=thread cargo +nightly test -Zbuild-std --target x86_64-unknown-linux-gnu` |
| **Miri** | UB in the ABI types & hashing (provenance, alignment, invalid values) | `cargo +nightly miri test -p polyplug_abi -p polyplug_utils` (`MIRIFLAGS=-Zmiri-disable-isolation`) |
| **loom** | the epoch publish / reclaim protocol behind lock-free reads + safe unload | `just loom` |

Notes from running these on polyplug:

- **Leak detection is intentionally off** under ASAN (`ASAN_OPTIONS=detect_leaks=0`). The
  loaders legitimately hold allocations to the end of the process: epoch-deferred
  reclamation frees a superseded interface and its library only once no reader is still
  pinned, and the CPython / .NET CLR once-per-process initializations are never torn down.
  LSAN would flag those as leaks even though each is reclaimed correctly or is a deliberate
  process-lifetime singleton. Use ASAN for *use-after-free*, not for leak hunting.
- **ASAN/TSAN require `-Zbuild-std`** so the standard library is instrumented too — the
  crash you are chasing crosses the FFI boundary into instrumented and uninstrumented code.
- Run your sanitizer build against the **same plugin bundles** you ship; a crash in a
  guest `.so` only shows up when that `.so` is loaded and exercised.

---

## 6. polyplug-specific crash classes

Most native crashes in a polyplug deployment fall into a handful of recognizable shapes.

### Use-after-unload / use-after-reload

The most common self-inflicted crash. The runtime's registry reads are lock-free: a
reader pins a crossbeam-epoch guard and serves from an immutable published snapshot, and
unload/reload reclaim the superseded interface and its library/VM only once no reader is
still pinned. The host-mediated lifecycle calls (`create_guest_instance` /
`destroy_guest_instance`) pin the epoch across their work, so they are always safe.

What is **not** safe — and is documented UB — is caching a *raw* interface pointer and
calling through it after the owning bundle was unloaded or hot-reloaded. The fix is built
into the generated callers: they cache the resolved interface but poll the runtime's
`revision_counter` (one acquire load) before each dispatch and re-resolve when it changes,
so a cached pointer never dangles. If you hand-rolled a caller and skipped that, you will
fault here. **Quiesce first:** stop issuing calls into a bundle before you unload it.

A crash with a reader thread deep in a generated dispatch while another thread is inside
an unload/reload is this class. Confirm with `bt all`.

### Guest panic / exception escaping the glue

A Rust `panic!`, a C++ `throw`, a C# exception, or a Lua/JS error that the generated glue
does not catch and that reaches the `extern "C"` frame aborts the process (§1). The frame
just below the boundary names the offending guest method. Fix it *in the guest*: return an
error through the contract instead of unwinding. This is a plugin defect, not a runtime
bug.

### Cross-boundary memory ownership

A plugin must never free memory it did not allocate, and cross-boundary data must use the
host allocator (`alloc` / `free` on `HostApi`). A double-free or a free of host-owned
memory shows up as a `SIGABRT` from the allocator (glibc "double free or corruption") or
an ASAN report. If you wrote a guest by hand rather than via `polyplugc`, audit every
buffer that crosses the boundary.

### Arena misuse

The per-call arena is a dispatch argument, not shared state. Writing past the arena's
capacity is reported as `ArenaOverflow` — *unless* you took a raw pointer into the arena
and wrote beyond what you reserved, which corrupts the heap and crashes later, far from
the cause. Treat the arena as opaque and only write through the provided accessors.

### "It crashed but the ABI versions matched"

If a bundle loads cleanly (versions matched) and then crashes during dispatch, suspect
calling-convention drift in a *hand-written* or out-of-date generated caller — a function
signature that no longer matches the ABI. Regenerate with the current `polyplugc` and
rebuild the bundle; the codegen derives every ABI signature from the frozen mirror, so
freshly generated code cannot drift (see [ABI Architecture](ABI_ARCHITECTURE.md)).

---

## 7. Per-language notes

The host is always native, but the faulting code may be inside a language VM. Each VM has
its own way to recover a *language-level* stack on top of the native one.

- **Native (Rust / C++ cdylib):** plain symbols. `gdb`/`lldb` + archived debuginfo is the
  whole story. This is the simplest case.
- **Python (CPython via the `polyplug_python` loader):** a crash in C extension code shows
  a native stack; recover the Python stack with the `python-gdb.py` extension
  (`py-bt` in gdb) or by enabling `faulthandler` in the guest so a fault prints the Python
  traceback before the process dies. Remember CPython is once-per-process — a Python crash
  is not runtime-isolated between `Runtime` instances.
- **.NET / C# (the `polyplug_dotnet` loader):** set `DOTNET_DbgEnableMiniDump=1` to have
  the CLR write a dump on an unhandled managed exception, then analyze with `dotnet-dump
  analyze` or `lldb` + the SOS plugin (`clrstack`, `dumpheap`). The CLR is also
  once-per-process.
- **Lua (LuaJIT via the `polyplug_lua` loader):** most LuaJIT crashes are `ffi.*`
  misuse — a bad cast or an out-of-bounds `ffi` write — and surface as a native `SIGSEGV`
  inside the LuaJIT VM. Get a Lua-level traceback with `debug.traceback` in the guest's
  error path; the native `bt` points at the VM, the Lua traceback points at the line.
- **JavaScript (QuickJS via the `polyplug_js` loader):** a JS-level error is a value, not
  a crash — it propagates back as an error. A *native* crash in the QuickJS path is a
  loader/bridge bug, not guest JS; capture it like any native crash and
  [report it](security-policy.md) if the guest JS was well-behaved.

---

## 8. Production deployment checklist

- [ ] Release profile sets `debug = 1` (or higher) and `split-debuginfo = "packed"`.
- [ ] Plugin bundles are built with debug info; symbols are **archived per release**
      alongside the host's.
- [ ] The host runs with `RUST_BACKTRACE=1` in production.
- [ ] Core dumps are enabled and routed (`LimitCORE=infinity` / `coredumpctl` on Linux,
      `kern.coredump` on macOS, `LocalDumps` on Windows).
- [ ] A staging tier exercises the real bundles under **ASAN and TSAN** before release.
- [ ] Hosts honor the **quiesce-before-unload** contract; no raw interface pointer is
      cached across an unload/reload (use the generated callers).
- [ ] Bundles are signed and, where authenticity matters, the runtime pins
      `trusted_keys` (see [Trust Model § Bundle Signing](TRUST_MODEL.md#bundle-signing)).

---

## See also

- [Trust Model](TRUST_MODEL.md) — the in-process trust boundary and the full threat model.
- [Unload Design](UNLOAD_DESIGN.md) and [Hot Reload](HOT_RELOAD_DESIGN.md) — the
  epoch-reclaim protocol and the quiesce contract in depth.
- [ABI Architecture](ABI_ARCHITECTURE.md) — why freshly generated callers cannot drift.
- [Profiling](PROFILING.md) — for performance investigation rather than crashes.
- [Security Policy](security-policy.md) — reporting a crash that *is* a runtime bug.
