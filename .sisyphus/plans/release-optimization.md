# Release Build Optimization (Epic 25)

## TL;DR

> **Quick Summary**: Maximize production release binary performance and minimize binary size for the
> `polyplug` workspace by adding an optimal `[profile.release]` to the workspace root, trimming
> `notify`'s platform backends, and adding `target-cpu` configuration for native deployment.
>
> **Deliverables**:
> - `Cargo.toml` (workspace root) — `[profile.release]` + `[profile.release.package.polyplugc]` sections added
> - `crates/polyplug/Cargo.toml` — `notify` dependency trimmed to used platform backends only
> - `.cargo/config.toml` — created with `rustflags` for `target-cpu`
> - Comment block at bottom of workspace `Cargo.toml` recording before/after binary sizes
>
> **Estimated Effort**: Short
> **Parallel Execution**: YES — 3 waves (Wave 1 has 3 parallel tasks)
> **Critical Path**: Wave 1 (3 parallel) → Task 4 (build + verify) → Task 5 (measure + record)

---

## Context

### Original Request
Optimize every release build of `polyplug` to be as fast as possible and as small as possible.
Production shipping builds ONLY. `cargo test` behavior is not a concern.

### Codebase Analysis

**Workspace layout:**
- Workspace root: `Cargo.toml` — no `[profile.*]` sections exist yet
- `crates/polyplug/` — core runtime, `crate-type = ["cdylib", "rlib"]`
- `crates/polyplugc/` — CLI codegen tool, `[[bin]]` only
- All adapter crates (`polyplug-dotnet`, `polyplug-python`, `polyplug-lua`, `polyplug-js`, `polyplug-js-deno`) — `crate-type = ["rlib"]`
- `guest-libs/rust/` and `host-libs/rust/` — no `crate-type` set (defaults to `rlib`)
- `showcase/host/` — declares its OWN `[workspace]`, is a **separate workspace**, out of scope

**Before sizes (measured from pre-existing artifacts):**
- `polyplugc`: **2.0M**
- `libpolyplug.so`: **1.1M**

**No `.cargo/config.toml` exists.** It must be created.

**No `[profile.*]` in any member crate.** Workspace root is the correct and only location.

### Dependency Audit (Complete)

| Dependency | Location | Current State | Action |
|---|---|---|---|
| `arc-swap` | workspace | No defaults | None needed |
| `thiserror` | workspace | Default `std` — needed | None |
| `libloading` | workspace | Default `std` — needed | None |
| `petgraph` | workspace | Already `default-features = false, features = ["stable_graph"]` | Already optimal |
| `clap` | workspace | All defaults needed for CLI UX | None |
| `serde` | workspace | Default `std` — needed | None |
| `toml` | workspace | `display` feature IS used (`toml::to_string` at `polyplugc/src/main.rs:267`) | None |
| `criterion` | dev-only | Dev dep — no release binary impact | None |
| `once_cell` | polyplug-dotnet | Default `std` — needed | None |
| `tempfile` | polyplug-dotnet | Default `getrandom` — needed | None |
| `tokio` | polyplug-js-deno | No defaults, already minimal | None |
| `notify` | polyplug (optional feature) | **Missing `default-features = false`** — pulls all platform backends | **FIX** |
| `mlua` | polyplug-lua | `["luajit", "vendored", "send"]` — PROTECTED | None |
| `netcorehost` | polyplug-dotnet | `["nethost", "net10_0"]` — PROTECTED | None |
| `pelite` | polyplug-dotnet | PROTECTED | None |
| `pyo3` | polyplug-python | `features = []` — PROTECTED | None |
| `rquickjs` | polyplug-js | `["parallel"]` — PROTECTED | None |
| `deno_core` | polyplug-js-deno | PROTECTED | None |

### Metis Review

**Critical Gap Found — `panic = "abort"` INCOMPATIBLE with this codebase:**

The runtime uses `std::panic::catch_unwind()` at every FFI boundary. Specifically:
- **12 FFI entry points** in `crates/polyplug/src/ffi/mod.rs` use `catch_unwind`
- **The code generator** (`crates/polyplugc/src/generators/rust/mod.rs:565`) emits `catch_unwind` wrappers for every generated ABI function
- **`ABI_ERROR_PANIC = 3`** (`crates/polyplug/src/abi/mod.rs:22`) is the designated return code for caught panics
- **`tests/integration_panic/`** validates that plugin panics are caught and reported without aborting the host process

With `panic = "abort"`, `catch_unwind` becomes a no-op — any plugin panic aborts the entire host process, destroying the runtime's safety model. **`panic = "abort"` MUST NOT be used.**

**Other Gaps Addressed:**
- `target-cpu=native` portability: `.cargo/config.toml` will include a prominent comment warning about SIGILL on machines without the same CPU features, and will NOT be force-committed (users must opt in)
- `strip = "symbols"` tradeoff: Acceptable for size optimization, documented
- Build time warning: `deno_core` + LTO + `codegen-units = 1` will make first release build very slow

---

## Work Objectives

### Core Objective
Configure the workspace release profile for maximum optimization and minimum binary size, trim
unused platform backends from the `notify` dependency, and provide a `target-cpu` configuration
for native deployment scenarios.

### Concrete Deliverables
- `Cargo.toml` — `[profile.release]` + `[profile.release.package.polyplugc]` appended
- `crates/polyplug/Cargo.toml` — `notify` line updated with `default-features = false`
- `.cargo/config.toml` — created at workspace root
- `Cargo.toml` — comment block at bottom with before/after binary sizes

### Definition of Done
- [ ] `cargo build --release --workspace` exits with code 0
- [ ] `cargo build --release --bin polyplugc` exits with code 0
- [ ] `cargo clippy -- -D warnings` exits with code 0
- [ ] `cargo fmt --check` exits with code 0
- [ ] `ls -lh target/release/polyplugc` — size recorded
- [ ] `ls -lh target/release/libpolyplug.so` — size recorded
- [ ] Size comment block present at bottom of workspace `Cargo.toml`

### Must Have
- `[profile.release]` in workspace root `Cargo.toml` with exactly the settings listed in the task spec (minus `panic = "abort"` — see Must NOT Have)
- `[profile.release.package.polyplugc]` with `opt-level = "z"`
- `notify` trimmed with `default-features = false` and three platform features
- `.cargo/config.toml` with `rustflags` documented
- Before/after size comment block in workspace `Cargo.toml`

### Must NOT Have (Guardrails)

- **`panic = "abort"` ANYWHERE in the workspace** — `catch_unwind` at 12 FFI entry points requires
  unwinding semantics. `panic = "abort"` silently turns `catch_unwind` into a no-op, causing plugin
  panics to abort the host process instead of returning `ABI_ERROR_PANIC = 3`. This is a hard
  runtime safety guarantee violation.
- **`lto = "fat"`** — fat LTO + cdylib triggers LLVM crash rust-lang/rust#117220
- **`[profile.*]` in any member crate Cargo.toml** — silently ignored by Cargo; creates confusion
- **Any change to protected dependency feature flags** — `mlua`, `netcorehost`, `pelite`, `pyo3`,
  `rquickjs`, `deno_core` are load-bearing; touching them breaks build or runtime correctness
- **Changes to `showcase/host/Cargo.toml`** — separate workspace, out of scope
- **Adding `crate-type` to `guest-libs/rust` or `host-libs/rust`** — cosmetic, out of scope

---

## Verification Strategy

> **ZERO HUMAN INTERVENTION** — ALL verification is agent-executed.

### Test Decision
- **Infrastructure exists**: YES (test harness via `[[test]]` targets)
- **Automated tests for THIS task**: NO — we are only editing `Cargo.toml` and creating a config
  file. No Rust source changes, no new logic, no tests needed.
- **Verification**: Build commands + binary size measurement + clippy/fmt

### QA Policy
Bash-based verification for all tasks: build commands, `grep` assertions on file contents,
`ls -lh` size measurement. No Playwright needed (no UI). No tmux needed (simple commands).

---

## Execution Strategy

### Parallel Execution Waves

```
Wave 1 (Start Immediately — independent file edits):
├── Task 1: Add [profile.release] sections to workspace Cargo.toml [quick]
├── Task 2: Update notify dependency in crates/polyplug/Cargo.toml [quick]
└── Task 3: Create .cargo/config.toml [quick]

Wave 2 (After Wave 1 — build and verify):
└── Task 4: Build release artifacts, run clippy + fmt check [quick]

Wave 3 (After Wave 2 — measure and record):
└── Task 5: Measure binary sizes and record comment block [quick]

Critical Path: Wave 1 → Task 4 → Task 5
Parallel Speedup: ~60% faster than sequential (Wave 1 parallelism)
Max Concurrent: 3 (Wave 1)
```

### Dependency Matrix

- **1**: no deps — blocks 4, 5
- **2**: no deps — blocks 4, 5
- **3**: no deps — blocks 4, 5
- **4**: depends 1, 2, 3 — blocks 5
- **5**: depends 4 — blocks none

### Agent Dispatch Summary

- **Wave 1**: 3 × `quick` (T1, T2, T3) — trivial file edits
- **Wave 2**: 1 × `quick` (T4) — build commands
- **Wave 3**: 1 × `quick` (T5) — size measurement + file edit

---

## TODOs

---

- [ ] 1. Add `[profile.release]` sections to workspace root `Cargo.toml`

  **What to do**:
  - Open `/mnt/data/Projects/Utils/polyplug/Cargo.toml`
  - Append the following block **exactly** after the last existing line (line 39). Do NOT modify any existing lines.
  - The workspace root currently ends with `unsafe_op_in_unsafe_fn = "warn"` on line 39.

  ```toml

  [profile.release]
  opt-level       = 3        # maximum optimization
  codegen-units   = 1        # single codegen unit — maximum cross-function inlining
  lto             = "thin"   # thin LTO — cross-crate inlining, safe for cdylib
                             # NOTE: fat LTO + cdylib triggers LLVM crash rust-lang/rust#117220
  strip           = "symbols" # remove all symbols — minimum binary size
  debug           = false    # no debug info
  incremental     = false    # deterministic builds — never use incremental in release
  overflow-checks = false    # disable integer overflow checks — zero-cost arithmetic
  # NOTE: panic = "abort" is intentionally ABSENT.
  # The polyplug runtime uses std::panic::catch_unwind at every FFI boundary
  # (12 entry points in ffi/mod.rs + every generated ABI function via codegen).
  # panic = "abort" turns catch_unwind into a no-op, causing any plugin panic to
  # abort the host process instead of returning ABI_ERROR_PANIC = 3.
  # This would destroy the runtime's plugin-isolation safety guarantee.

  # polyplugc is a build-time CLI tool — optimize for size over raw throughput
  [profile.release.package.polyplugc]
  opt-level = "z"            # "z" = smallest code size (vs "s" which is moderate)
  ```

  **Must NOT do**:
  - Do NOT add `panic = "abort"` — incompatible with `catch_unwind` FFI safety model (see above comment)
  - Do NOT use `lto = "fat"` — triggers LLVM crash with cdylib (rust-lang/rust#117220)
  - Do NOT add any `[profile.*]` to any other crate's `Cargo.toml`
  - Do NOT modify any existing lines in `Cargo.toml`

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Single file append, no logic, pure text edit
  - **Skills**: []
  - **Skills Evaluated but Omitted**:
    - `git-master`: No commit needed in this wave

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 2, 3)
  - **Blocks**: Tasks 4, 5
  - **Blocked By**: None (can start immediately)

  **References**:

  **Pattern References**:
  - `Cargo.toml:1-39` — current workspace root content; append after line 39

  **External References**:
  - [Cargo profile reference](https://doc.rust-lang.org/cargo/reference/profiles.html) — profile key documentation
  - [rust-lang/rust#117220](https://github.com/rust-lang/rust/issues/117220) — fat LTO + cdylib LLVM crash
  - `crates/polyplug/src/ffi/mod.rs` — verify `catch_unwind` usage before considering panic = abort (it's there, do NOT add panic = abort)

  **Acceptance Criteria**:

  ```
  Scenario: Profile sections are present and correct
    Tool: Bash
    Preconditions: File has been edited
    Steps:
      1. grep 'lto' Cargo.toml
         Expected: shows lto = "thin"
      2. grep 'panic' Cargo.toml
         Expected: ZERO lines containing panic = "abort" (only comment line is acceptable)
      3. grep 'opt-level.*z' Cargo.toml
         Expected: shows the polyplugc package override
      4. grep 'codegen-units.*1' Cargo.toml
         Expected: shows codegen-units = 1
      5. grep 'strip.*symbols' Cargo.toml
         Expected: shows strip = "symbols"
    Expected Result: All greps match. No panic = "abort" in any key (only in comment).
    Evidence: .sisyphus/evidence/task-1-profile-check.txt

  Scenario: No profile sections in member crates
    Tool: Bash
    Preconditions: Task 1 applied
    Steps:
      1. grep -r '\[profile\.' crates/ host-libs/ guest-libs/
         Expected: NO output (zero matches)
    Expected Result: grep exits non-zero (no matches = correct)
    Evidence: .sisyphus/evidence/task-1-no-member-profiles.txt
  ```

  **Evidence to Capture**:
  - [ ] `.sisyphus/evidence/task-1-profile-check.txt` — grep output confirming profile keys
  - [ ] `.sisyphus/evidence/task-1-no-member-profiles.txt` — grep output confirming no member profiles

  **Commit**: YES (groups with Tasks 2 and 3 in a single commit after Wave 1)
  - Message: `build(release): add release profile optimizations for production builds`
  - Files: `Cargo.toml`, `crates/polyplug/Cargo.toml`, `.cargo/config.toml`
  - Pre-commit: (none — source code unchanged, no tests needed)

---

- [ ] 2. Update `notify` dependency in `crates/polyplug/Cargo.toml`

  **What to do**:
  - Open `crates/polyplug/Cargo.toml`
  - Find line 19 (the `notify` dependency line):
    ```toml
    notify     = { version = "6", optional = true }
    ```
  - Replace it with:
    ```toml
    notify     = { version = "6", default-features = false, features = ["macos_fsevent", "inotify", "windows_ReadDirectoryChangesW"], optional = true }
    ```
  - This trims the kqueue and other unused platform backends from the `hot-reload` feature compilation.

  **Must NOT do**:
  - Do NOT change `optional = true` — the hot-reload feature flag must remain optional
  - Do NOT change the version `"6"`
  - Do NOT touch any other dependency in this file
  - Do NOT touch `mlua`, `thiserror`, `libloading`, `petgraph`, `serde`, `toml` in this file

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Single line replacement in one file
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 1, 3)
  - **Blocks**: Tasks 4, 5
  - **Blocked By**: None (can start immediately)

  **References**:

  **Pattern References**:
  - `crates/polyplug/Cargo.toml:19` — the exact line to replace

  **External References**:
  - [notify crate backends](https://docs.rs/notify/latest/notify/) — `macos_fsevent` (macOS), `inotify` (Linux), `windows_ReadDirectoryChangesW` (Windows) are the three supported platform backends

  **Acceptance Criteria**:

  ```
  Scenario: notify dependency has default-features = false
    Tool: Bash
    Preconditions: File has been edited
    Steps:
      1. grep 'notify' crates/polyplug/Cargo.toml
         Expected: shows default-features = false AND optional = true
      2. grep 'macos_fsevent' crates/polyplug/Cargo.toml
         Expected: feature is present in the notify line
    Expected Result: Both greps match
    Evidence: .sisyphus/evidence/task-2-notify-check.txt

  Scenario: Protected dependencies are unchanged
    Tool: Bash
    Preconditions: Task 2 applied
    Steps:
      1. grep 'mlua' crates/polyplug-lua/Cargo.toml
         Expected: still shows features = ["luajit", "vendored", "send"]
    Expected Result: mlua line unchanged
    Evidence: .sisyphus/evidence/task-2-protected-deps.txt
  ```

  **Evidence to Capture**:
  - [ ] `.sisyphus/evidence/task-2-notify-check.txt` — grep confirming notify update
  - [ ] `.sisyphus/evidence/task-2-protected-deps.txt` — grep confirming protected deps unchanged

  **Commit**: Groups with Tasks 1 and 3 (see Task 1 commit)

---

- [ ] 3. Create `.cargo/config.toml`

  **What to do**:
  - Create the file `/mnt/data/Projects/Utils/polyplug/.cargo/config.toml` with the following content:

  ```toml
  # .cargo/config.toml — polyplug workspace build configuration
  #
  # WARNING — target-cpu=native:
  #   This flag compiles for the current machine's exact CPU features (e.g., AVX-512).
  #   The resulting binary WILL CRASH WITH SIGILL on any machine that lacks those features.
  #
  #   ONLY use this when:
  #     1. You are building AND deploying on the same machine type, OR
  #     2. You know the deployment target has the same or newer CPU generation
  #
  #   If distributing this binary to other machines, REMOVE this flag or replace with
  #   a safe baseline such as:
  #     target-cpu=x86-64-v2  (requires: SSE4.2 — supported by nearly all x86-64 CPUs since ~2010)
  #     target-cpu=x86-64-v3  (requires: AVX2/BMI2 — supported by Intel Haswell+, AMD Zen+)
  #
  #   For CI systems with different hardware than deployment targets, REMOVE this entirely.

  [target.x86_64-unknown-linux-gnu]
  rustflags = [
    "-C", "target-cpu=native",   # use all CPU features available on the current build machine
  ]
  ```

  **Must NOT do**:
  - Do NOT set `target-cpu` without the warning comment — the portability risk must be documented
  - Do NOT add linker flags or other rustflags unless explicitly requested in a future task
  - Do NOT create `config.toml` without the `.cargo/` directory (create the directory first if it doesn't exist)

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: New file creation, pure text
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 1, 2)
  - **Blocks**: Tasks 4, 5
  - **Blocked By**: None (can start immediately)

  **References**:
  - [Cargo config reference](https://doc.rust-lang.org/cargo/reference/config.html) — `.cargo/config.toml` format
  - [rustc target-cpu](https://doc.rust-lang.org/rustc/codegen-options/index.html#target-cpu) — target-cpu documentation

  **Acceptance Criteria**:

  ```
  Scenario: .cargo/config.toml exists and is correctly formed
    Tool: Bash
    Preconditions: File created
    Steps:
      1. test -f .cargo/config.toml && echo "EXISTS" || echo "MISSING"
         Expected: "EXISTS"
      2. grep 'target-cpu' .cargo/config.toml
         Expected: shows "-C", "target-cpu=native"
      3. grep 'WARNING' .cargo/config.toml
         Expected: warning comment is present
    Expected Result: File exists, target-cpu present, warning documented
    Evidence: .sisyphus/evidence/task-3-config-check.txt
  ```

  **Evidence to Capture**:
  - [ ] `.sisyphus/evidence/task-3-config-check.txt` — existence and content verification

  **Commit**: Groups with Tasks 1 and 2 (see Task 1 commit)

---

- [ ] 4. Build release artifacts and run AGENTS.md enforcement checks

  **What to do**:
  - Run the following commands in sequence. ALL must exit with code 0.
  - Working directory: workspace root (`/mnt/data/Projects/Utils/polyplug`)

  ```bash
  # Build the entire workspace in release mode
  cargo build --release --workspace 2>&1

  # Build polyplugc explicitly (also covered by --workspace, but verify separately)
  cargo build --release --bin polyplugc 2>&1

  # AGENTS.md enforcement — zero warnings tolerated
  cargo clippy -- -D warnings 2>&1

  # AGENTS.md enforcement — formatting must be clean
  cargo fmt --check 2>&1
  ```

  - If `cargo build --release --workspace` fails, diagnose the failure and fix it. The most likely causes:
    1. `notify` feature flags don't match what the crate's code uses — check the feature names against `notify 6.x` docs
    2. `.cargo/config.toml` has a syntax error
  - If `cargo clippy` fails, fix warnings in the changed files (TOML edits shouldn't produce clippy warnings, but verify)
  - If `cargo fmt --check` fails, run `cargo fmt` and commit the formatting changes

  **NOTE on build time**: The first release build after adding `codegen-units = 1` and `lto = "thin"`
  with `deno_core` in the dependency tree will be **significantly slower** (potentially 10-20 minutes
  depending on hardware). This is expected. Subsequent builds will be faster due to cargo caching.

  **Must NOT do**:
  - Do NOT run `cargo test` — `cargo test` behavior is explicitly not a concern for this epic
  - Do NOT add `panic = "abort"` to fix any test failures — that setting is forbidden
  - Do NOT modify any Rust source files to fix clippy warnings introduced by this task (there should be none since we only edited TOML)

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Running build/check commands, no code authoring
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO — must run after Wave 1 completes
  - **Parallel Group**: Wave 2 (sequential, single task)
  - **Blocks**: Task 5
  - **Blocked By**: Tasks 1, 2, 3

  **References**:
  - `Cargo.toml:1-39` — verify the profile was added correctly if build fails
  - `crates/polyplug/Cargo.toml:19` — verify notify line if build fails with notify-related errors
  - `.cargo/config.toml` — verify syntax if build fails with config errors

  **Acceptance Criteria**:

  ```
  Scenario: Full workspace release build succeeds
    Tool: Bash
    Preconditions: Wave 1 tasks complete
    Steps:
      1. cargo build --release --workspace
         Expected: exits with code 0, no error messages
      2. cargo build --release --bin polyplugc
         Expected: exits with code 0
      3. ls target/release/polyplugc target/release/libpolyplug.so
         Expected: both files exist
    Expected Result: Both binaries present, build exits 0
    Failure Indicators: "error[E...]" in cargo output, non-zero exit code
    Evidence: .sisyphus/evidence/task-4-build-output.txt

  Scenario: AGENTS.md enforcement passes
    Tool: Bash
    Preconditions: Build succeeds
    Steps:
      1. cargo clippy -- -D warnings
         Expected: exits with code 0
      2. cargo fmt --check
         Expected: exits with code 0
    Expected Result: Both checks pass
    Failure Indicators: "warning:" or "error:" in clippy output; "diff" in fmt output
    Evidence: .sisyphus/evidence/task-4-lint-check.txt
  ```

  **Evidence to Capture**:
  - [ ] `.sisyphus/evidence/task-4-build-output.txt` — full cargo build output
  - [ ] `.sisyphus/evidence/task-4-lint-check.txt` — clippy + fmt output

  **Commit**: NO (build artifacts are not committed)

---

- [ ] 5. Measure binary sizes and record comment block

  **What to do**:
  - Run the following measurement commands:

  ```bash
  ls -lh target/release/polyplugc
  ls -lh target/release/libpolyplug.so
  ```

  - Record the results. If the `vtable_dispatch` bench target exists, also run:

  ```bash
  cargo bench --bench vtable_dispatch 2>&1 | tail -20
  ```

  - Append the following comment block to the **end** of the workspace root `Cargo.toml`
    (after the last existing content), substituting the actual measured sizes:

  ```toml

  # ─────────────────────────────────────────────────────────────────────────────
  # Epic 25 — Release optimization results (YYYY-MM-DD):
  # polyplugc:        BEFORE 2.0M → AFTER X.XM
  # libpolyplug.so:   BEFORE 1.1M → AFTER X.XM
  # vtable_dispatch:  X.XX ns/call (if bench ran) | N/A (if bench not present)
  # ─────────────────────────────────────────────────────────────────────────────
  ```

  - Replace `YYYY-MM-DD` with today's date, and `X.XM` / `X.XX` with the actual measured values.

  **Must NOT do**:
  - Do NOT omit the comment block — it is a required deliverable per the task spec
  - Do NOT use incorrect "before" values — before sizes are: `polyplugc = 2.0M`, `libpolyplug.so = 1.1M`
  - Do NOT run `cargo test` when checking the bench target

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: File measurement and comment append
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO — depends on Task 4 (needs built artifacts)
  - **Parallel Group**: Wave 3 (sequential, single task)
  - **Blocks**: None
  - **Blocked By**: Task 4

  **References**:
  - `Cargo.toml:39` — append after this line (the last line of existing content)
  - `target/release/polyplugc` — measure this file
  - `target/release/libpolyplug.so` — measure this file

  **Acceptance Criteria**:

  ```
  Scenario: Size comment block is present and correct
    Tool: Bash
    Preconditions: Task 4 complete, artifacts built
    Steps:
      1. grep 'Epic 25' Cargo.toml
         Expected: comment block exists
      2. grep 'BEFORE' Cargo.toml
         Expected: shows actual before sizes (2.0M and 1.1M)
      3. grep 'AFTER' Cargo.toml
         Expected: shows actual measured after sizes (not placeholder X.XM)
    Expected Result: Comment block present with real numbers
    Evidence: .sisyphus/evidence/task-5-size-record.txt

  Scenario: Sizes reduced from baseline
    Tool: Bash
    Preconditions: Artifacts built
    Steps:
      1. ls -lh target/release/polyplugc
         Expected: smaller than 2.0M OR at most equal (LTO + strip should reduce size)
      2. ls -lh target/release/libpolyplug.so
         Expected: smaller than 1.1M OR at most equal
    Expected Result: Binary sizes did not increase from pre-optimization baseline
    Evidence: .sisyphus/evidence/task-5-size-check.txt
  ```

  **Evidence to Capture**:
  - [ ] `.sisyphus/evidence/task-5-size-record.txt` — grep output confirming comment block
  - [ ] `.sisyphus/evidence/task-5-size-check.txt` — `ls -lh` output with actual sizes

  **Commit**: YES
  - Message: `build(release): record Epic 25 release optimization results`
  - Files: `Cargo.toml` (comment block only)
  - Pre-commit: (none)

---

## Final Verification Wave

> Run after ALL implementation tasks. Four agents in parallel. All must pass.

- [ ] F1. **Plan Compliance Audit** — `oracle`

  Read the plan end-to-end. Verify:
  - `[profile.release]` present in workspace `Cargo.toml` with all required keys
  - `panic = "abort"` is ABSENT from `[profile.release]` (only permitted in a comment)
  - `lto = "thin"` present (not "fat", not true)
  - `[profile.release.package.polyplugc]` with `opt-level = "z"` present
  - `notify` line in `crates/polyplug/Cargo.toml` has `default-features = false`
  - `.cargo/config.toml` exists and contains documented `target-cpu` rustflag
  - Comment block at bottom of workspace `Cargo.toml` with real size numbers
  - No `[profile.*]` in any member crate
  - Evidence files exist in `.sisyphus/evidence/`
  - Protected deps (`mlua`, `netcorehost`, `pelite`, `pyo3`, `rquickjs`, `deno_core`) unchanged

  Output: `Must Have [N/N] | Must NOT Have [N/N] | VERDICT: APPROVE/REJECT`

- [ ] F2. **Build & Lint Verification** — `quick`

  Run fresh:
  ```bash
  cargo build --release --workspace
  cargo clippy -- -D warnings
  cargo fmt --check
  ```
  All three must exit 0.

  Output: `Build [PASS/FAIL] | Clippy [PASS/FAIL] | Fmt [PASS/FAIL] | VERDICT`

- [ ] F3. **Constraint Verification** — `quick`

  ```bash
  # Must NOT have fat LTO
  grep 'lto.*fat' Cargo.toml && echo FAIL || echo OK

  # Must NOT have panic = abort (as a key, not in comment)
  grep -E '^panic\s*=' Cargo.toml && echo FAIL || echo OK

  # Must have thin LTO
  grep 'lto.*thin' Cargo.toml && echo OK || echo FAIL

  # Must have codegen-units = 1
  grep 'codegen-units.*1' Cargo.toml && echo OK || echo FAIL

  # notify must have default-features = false
  grep 'default-features.*false' crates/polyplug/Cargo.toml && echo OK || echo FAIL

  # mlua must be unchanged
  grep 'luajit.*vendored.*send\|luajit.*send.*vendored' crates/polyplug-lua/Cargo.toml && echo OK || echo FAIL
  ```

  Output: `[N/N constraints verified] | VERDICT: APPROVE/REJECT`

- [ ] F4. **Size Delta Verification** — `quick`

  ```bash
  ls -lh target/release/polyplugc
  ls -lh target/release/libpolyplug.so
  ```

  Verify the recorded "AFTER" sizes in the `Cargo.toml` comment block match the actual file sizes.
  Verify sizes are ≤ before sizes (2.0M and 1.1M respectively).

  Output: `polyplugc [size, ≤2.0M: PASS/FAIL] | libpolyplug.so [size, ≤1.1M: PASS/FAIL] | VERDICT`

---

## Commit Strategy

```
Commit 1 (after Wave 1):
  build(release): add release profile optimizations for production builds
  Files: Cargo.toml, crates/polyplug/Cargo.toml, .cargo/config.toml

Commit 2 (after Task 5):
  build(release): record Epic 25 release optimization results
  Files: Cargo.toml (comment block only)
```

---

## Success Criteria

### Verification Commands

```bash
# Profile is present and correct
grep 'lto.*thin' Cargo.toml                           # Expected: lto = "thin"
grep -c 'profile.release' Cargo.toml                 # Expected: >= 2 (release + polyplugc sections)
grep 'opt-level.*z' Cargo.toml                        # Expected: polyplugc override

# panic = abort is absent as a key (may exist in a comment — that's fine)
grep -E '^panic\s*=' Cargo.toml && echo FAIL || echo OK   # Expected: OK

# notify is trimmed
grep 'default-features.*false' crates/polyplug/Cargo.toml  # Expected: notify line

# .cargo/config.toml exists
test -f .cargo/config.toml && echo OK || echo FAIL    # Expected: OK

# Build succeeds
cargo build --release --workspace                      # Expected: exit 0
cargo clippy -- -D warnings                           # Expected: exit 0
cargo fmt --check                                     # Expected: exit 0

# Sizes recorded
grep 'Epic 25' Cargo.toml                             # Expected: comment block
ls -lh target/release/polyplugc                       # Expected: file exists, size ≤ 2.0M
ls -lh target/release/libpolyplug.so                  # Expected: file exists, size ≤ 1.1M
```

### Final Checklist

- [ ] `[profile.release]` present in workspace `Cargo.toml` — verified
- [ ] `panic = "abort"` absent as a profile key — verified
- [ ] `lto = "thin"` in `[profile.release]` — verified
- [ ] `codegen-units = 1` in `[profile.release]` — verified
- [ ] `strip = "symbols"` in `[profile.release]` — verified
- [ ] `overflow-checks = false` in `[profile.release]` — verified
- [ ] `[profile.release.package.polyplugc]` with `opt-level = "z"` — verified
- [ ] `notify` has `default-features = false` in `crates/polyplug/Cargo.toml` — verified
- [ ] `.cargo/config.toml` exists with documented `target-cpu` — verified
- [ ] No `[profile.*]` in any member crate — verified
- [ ] `lto = "fat"` appears nowhere — verified
- [ ] `mlua` features unchanged: `["luajit", "vendored", "send"]` — verified
- [ ] `netcorehost` features unchanged — verified
- [ ] `pelite` features unchanged — verified
- [ ] `pyo3` features unchanged: `[]` — verified
- [ ] `rquickjs` features unchanged: `["parallel"]` — verified
- [ ] `deno_core` features unchanged — verified
- [ ] `cargo build --release --workspace` exits 0 — verified
- [ ] `cargo clippy -- -D warnings` exits 0 — verified
- [ ] `cargo fmt --check` exits 0 — verified
- [ ] Before/after size comment block in workspace `Cargo.toml` — verified
- [ ] `target-cpu=native` documented with portability warning — verified
