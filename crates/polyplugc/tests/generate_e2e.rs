//! End-to-end tests for `polyplugc generate` and `polyplugc validate
//! --bundle-dir`.
//!
//! Two guarantees are proven here:
//!
//! 1. **generate output is immediately compilable.** We run the `polyplugc`
//!    binary `generate --bundle <toml> --lang rust --out <tmp>/gen`, then the
//!    TEST writes a minimal `Cargo.toml` (cdylib, path dep on the in-tree
//!    `sdks/rust/guest`) and `src/lib.rs` that simply includes the generated
//!    glue, and finally `cargo build`s it. A successful cdylib artifact proves
//!    the generated code compiles with zero hand edits to any emitted file.
//!
//! 2. **validate --bundle-dir catches assembly mistakes.** We assemble a bundle
//!    directory (generated `manifest.toml` + a dummy artifact with the declared
//!    name) and assert the CLI accepts a correct dir, rejects a missing
//!    artifact, and rejects a tampered `id`.
//!
//! Run with:
//!   cargo test --test generate_e2e --package polyplugc

#![allow(clippy::expect_used)]

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

/// Absolute path to the repository root, derived from this crate's manifest dir
/// (`<repo>/crates/polyplugc`).
fn repo_root() -> PathBuf {
    let manifest_dir: PathBuf = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .and_then(Path::parent)
        .expect("crate manifest dir must have a grandparent (the repo root)")
        .to_path_buf()
}

/// Absolute path to the in-tree Rust guest SDK crate.
fn rust_guest_sdk_path() -> PathBuf {
    repo_root().join("sdks").join("rust").join("guest")
}

/// Absolute path to the representative Rust bundle used by the e2e tests. The
/// decoder bundle declares a `[bundle.file]` platform table and one plugin
/// implementing `pipeline.Decoder`, exercising a broad slice of the codegen
/// surface (StringView params/returns).
fn example_bundle_toml() -> PathBuf {
    repo_root()
        .join("examples")
        .join("guests")
        .join("rust")
        .join("decoder")
        .join("bundle.toml")
}

/// Absolute path to the representative `api.toml`, which declares the
/// `host.logger` host contract whose `log(message: StringView)` thunk exercises
/// the host-side StringView param-unpack path.
fn example_api_toml() -> PathBuf {
    repo_root().join("examples").join("api.toml")
}

/// Absolute path to the in-tree ABI crate (a dependency the generated host
/// glue needs).
fn polyplug_abi_path() -> PathBuf {
    repo_root().join("crates").join("polyplug_abi")
}

/// Absolute path to the in-tree utils crate. The generated host glue imports
/// `polyplug_utils::HostContractId` directly (it never re-exports through the
/// ABI crate), so the driver must declare this dependency.
fn polyplug_utils_path() -> PathBuf {
    repo_root().join("crates").join("polyplug_utils")
}

/// Run the `polyplugc` binary with `args`, returning the captured output.
fn run_polyplugc(args: &[&std::ffi::OsStr]) -> std::process::Output {
    let bin: &str = env!("CARGO_BIN_EXE_polyplugc");
    Command::new(bin)
        .args(args)
        .output()
        .expect("failed to spawn polyplugc binary")
}

// ═══════════════════════════════════════════════════════════════════════════
// Rust: generate → cargo build must succeed with zero hand edits.
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn rust_generated_glue_compiles() {
    let tmp: tempfile::TempDir = tempfile::tempdir().expect("tempdir");
    let project_dir: PathBuf = tmp.path().join("plugin");
    let gen_dir: PathBuf = project_dir.join("gen");
    std::fs::create_dir_all(project_dir.join("src")).expect("create src dir");

    // Generate the guest glue into <project>/gen via the CLI.
    let output: std::process::Output = run_polyplugc(&[
        "generate".as_ref(),
        "--bundle".as_ref(),
        example_bundle_toml().as_os_str(),
        "--lang".as_ref(),
        "rust".as_ref(),
        "--out".as_ref(),
        gen_dir.as_os_str(),
    ]);
    assert!(
        output.status.success(),
        "polyplugc generate failed (status {:?})\n--- stdout ---\n{}\n--- stderr ---\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        gen_dir.join("guest/mod.rs").exists(),
        "generated guest/mod.rs must exist at {}",
        gen_dir.join("guest/mod.rs").display()
    );

    // The TEST writes the project shell — none of the generated files are edited.
    let cargo_toml: String = format!(
        "[package]\n\
         name = \"plugin\"\n\
         version = \"0.1.0\"\n\
         edition = \"2024\"\n\n\
         [lib]\n\
         crate-type = [\"cdylib\"]\n\n\
         [dependencies]\n\
         polyplug_abi = {{ path = \"{}\" }}\n\
         polyplug_guest = {{ path = \"{}\" }}\n\
         polyplug_utils = {{ path = \"{}\" }}\n",
        polyplug_abi_path()
            .display()
            .to_string()
            .replace('\\', "\\\\"),
        rust_guest_sdk_path()
            .display()
            .to_string()
            .replace('\\', "\\\\"),
        polyplug_utils_path()
            .display()
            .to_string()
            .replace('\\', "\\\\")
    );
    std::fs::write(project_dir.join("Cargo.toml"), cargo_toml).expect("write Cargo.toml");

    // src/lib.rs includes the generated glue verbatim and provides the minimal
    // plugin entry points. This is the only hand-written file.
    // polyplug_user_init is the plugin author's mandatory entry point — the
    // generated polyplug_init declares it extern and calls it. Linux ld defers
    // unresolved cdylib symbols to load time, but macOS ld64 requires them at
    // link time, so the stub must exist for the build to succeed everywhere.
    let lib_rs: &str = "#[path = \"../gen/guest/mod.rs\"]\n\
         mod generated;\n\
         \n\
         #[unsafe(no_mangle)]\n\
         pub extern \"C\" fn polyplug_user_init() {}\n\
         \n\
         #[unsafe(no_mangle)]\n\
         pub extern \"C\" fn polyplug_abi_version() -> u32 {\n\
         polyplug_abi::POLYPLUG_ABI_VERSION\n\
         }\n";
    std::fs::write(project_dir.join("src/lib.rs"), lib_rs).expect("write src/lib.rs");

    // Per-test target dir inside the tempdir to avoid contending on the
    // workspace target lock while the outer test run holds it.
    let target_dir: PathBuf = tmp.path().join("target");
    let build: std::process::Output = Command::new(env!("CARGO"))
        .arg("build")
        .arg("--manifest-path")
        .arg(project_dir.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", &target_dir)
        .output()
        .expect("failed to spawn cargo build for generated project");
    assert!(
        build.status.success(),
        "cargo build of generated Rust project failed (status {:?})\n--- stdout ---\n{}\n--- stderr ---\n{}",
        build.status.code(),
        String::from_utf8_lossy(&build.stdout),
        String::from_utf8_lossy(&build.stderr),
    );

    let produced_cdylib: bool = std::fs::read_dir(target_dir.join("debug"))
        .expect("debug output dir must exist after a successful build")
        .filter_map(Result::ok)
        .any(|entry: std::fs::DirEntry| {
            let name: String = entry.file_name().to_string_lossy().into_owned();
            name.contains("plugin")
                && (name.ends_with(".so") || name.ends_with(".dylib") || name.ends_with(".dll"))
        });
    assert!(
        produced_cdylib,
        "build succeeded but no cdylib artifact named like the plugin was produced in {}",
        target_dir.join("debug").display(),
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// validate --bundle-dir: correct / missing-artifact / tampered-id.
//
// The decoder fixture's `[bundle.file]` table only declares linux.x86_64
// (`libdecoder.so`), so these cases run only on that platform.
// ═══════════════════════════════════════════════════════════════════════════

/// Generate the bundle manifest (and guest glue) for the decoder fixture into
/// `out_dir` via the CLI.
fn generate_manifest_into(out_dir: &Path) {
    let output: std::process::Output = run_polyplugc(&[
        "generate".as_ref(),
        "--bundle".as_ref(),
        example_bundle_toml().as_os_str(),
        "--lang".as_ref(),
        "rust".as_ref(),
        "--out".as_ref(),
        out_dir.as_os_str(),
    ]);
    assert!(
        output.status.success(),
        "polyplugc generate failed:\n{}",
        String::from_utf8_lossy(&output.stderr),
    );
}

/// The entry-file name the decoder manifest declares for linux.x86_64.
const DECLARED_ARTIFACT: &str = "libdecoder.so";

fn is_supported_platform() -> bool {
    cfg!(target_os = "linux") && cfg!(target_arch = "x86_64")
}

#[test]
fn validate_bundle_dir_accepts_correct_bundle() {
    if !is_supported_platform() {
        return;
    }
    let tmp: tempfile::TempDir = tempfile::tempdir().expect("tempdir");
    let dir: PathBuf = tmp.path().join("dist");
    std::fs::create_dir_all(&dir).expect("create dist dir");

    generate_manifest_into(&dir);
    std::fs::write(dir.join(DECLARED_ARTIFACT), b"dummy").expect("write artifact");

    let output: std::process::Output = run_polyplugc(&[
        "validate".as_ref(),
        "--bundle-dir".as_ref(),
        dir.as_os_str(),
    ]);
    assert!(
        output.status.success(),
        "validate --bundle-dir should accept a correct bundle, got status {:?}\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("OK:"),
        "expected OK on success, got:\n{}",
        String::from_utf8_lossy(&output.stdout),
    );
}

#[test]
fn validate_bundle_dir_rejects_missing_artifact() {
    if !is_supported_platform() {
        return;
    }
    let tmp: tempfile::TempDir = tempfile::tempdir().expect("tempdir");
    let dir: PathBuf = tmp.path().join("dist");
    std::fs::create_dir_all(&dir).expect("create dist dir");

    // Manifest only — the declared artifact is intentionally absent.
    generate_manifest_into(&dir);

    let output: std::process::Output = run_polyplugc(&[
        "validate".as_ref(),
        "--bundle-dir".as_ref(),
        dir.as_os_str(),
    ]);
    assert!(
        !output.status.success(),
        "validate --bundle-dir must fail when the entry artifact is missing",
    );
    let stderr: std::borrow::Cow<'_, str> = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("file") && stderr.contains("does not exist"),
        "error must name the missing `file`, got:\n{stderr}",
    );
}

#[test]
fn validate_bundle_dir_rejects_tampered_id() {
    if !is_supported_platform() {
        return;
    }
    let tmp: tempfile::TempDir = tempfile::tempdir().expect("tempdir");
    let dir: PathBuf = tmp.path().join("dist");
    std::fs::create_dir_all(&dir).expect("create dist dir");

    generate_manifest_into(&dir);
    std::fs::write(dir.join(DECLARED_ARTIFACT), b"dummy").expect("write artifact");

    // Tamper: rewrite the `id` line so it no longer equals fnv1a_64(name).
    let manifest_path: PathBuf = dir.join("manifest.toml");
    let original: String = std::fs::read_to_string(&manifest_path).expect("read manifest");
    let tampered: String = original
        .lines()
        .map(|line: &str| {
            if line.trim_start().starts_with("id =") {
                "id = 1".to_owned()
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<String>>()
        .join("\n");
    assert_ne!(original, tampered, "tamper step must change the manifest");
    std::fs::write(&manifest_path, tampered).expect("write tampered manifest");

    let output: std::process::Output = run_polyplugc(&[
        "validate".as_ref(),
        "--bundle-dir".as_ref(),
        dir.as_os_str(),
    ]);
    assert!(
        !output.status.success(),
        "validate --bundle-dir must fail when the manifest `id` is tampered",
    );
    let stderr: std::borrow::Cow<'_, str> = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("tamper") || stderr.contains("id") || stderr.contains("expected"),
        "error must signal an id/tamper mismatch, got:\n{stderr}",
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Empty-string round-trip: EXECUTION proof for the host-thunk StringView guard.
//
// The host-side thunk (interface_factories.rs) unpacks a `StringView` arg into a
// `&str` before calling the host-contract impl. A `StringView::null()` (ptr=null,
// len=0) is a legal ABI value, and building a slice from a null pointer is UB even
// at len==0. The generated thunk now routes through the null-safe `as_str()`.
//
// This test generates the host glue, then BUILDS AND RUNS a driver that:
//   1. constructs the generated `host.logger` interface around a tiny impl,
//   2. reaches the generated `log` thunk via the interface's native dispatch
//      table and calls it with a `StringView::null()` argument,
//   3. asserts the impl received the empty string and the thunk returned Ok.
// Under the pre-fix code this dispatch was undefined behaviour; passing here
// proves the guard executes, not merely that the text was emitted.
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn host_thunk_empty_stringview_round_trips() {
    let tmp: tempfile::TempDir = tempfile::tempdir().expect("tempdir");
    let project_dir: PathBuf = tmp.path().join("driver");
    let gen_dir: PathBuf = project_dir.join("gen");
    std::fs::create_dir_all(project_dir.join("src")).expect("create src dir");

    // Generate the HOST glue (includes host/interface_factories.rs with the thunk).
    let output: std::process::Output = run_polyplugc(&[
        "generate".as_ref(),
        "--api".as_ref(),
        example_api_toml().as_os_str(),
        "--lang".as_ref(),
        "rust".as_ref(),
        "--out".as_ref(),
        gen_dir.as_os_str(),
    ]);
    assert!(
        output.status.success(),
        "polyplugc generate --api --lang rust failed (status {:?})\n--- stdout ---\n{}\n--- stderr ---\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        gen_dir.join("host/interface_factories.rs").exists(),
        "generated host/interface_factories.rs must exist at {}",
        gen_dir.join("host/interface_factories.rs").display()
    );

    let cargo_toml: String = format!(
        "[package]\n\
         name = \"driver\"\n\
         version = \"0.1.0\"\n\
         edition = \"2024\"\n\n\
         [[bin]]\n\
         name = \"driver\"\n\
         path = \"src/main.rs\"\n\n\
         [dependencies]\n\
         polyplug_abi = {{ path = \"{}\" }}\n\
         polyplug_utils = {{ path = \"{}\" }}\n",
        polyplug_abi_path()
            .display()
            .to_string()
            .replace('\\', "\\\\"),
        polyplug_utils_path()
            .display()
            .to_string()
            .replace('\\', "\\\\")
    );
    std::fs::write(project_dir.join("Cargo.toml"), cargo_toml).expect("write Cargo.toml");

    // The only hand-written file. It includes the generated host glue verbatim,
    // builds the logger interface, fetches the `log` thunk from the native
    // dispatch table, and invokes it with a NULL StringView. The impl records
    // what it received; the driver exits non-zero on any mismatch so a build that
    // links the pre-fix (UB) thunk cannot silently pass.
    let main_rs: &str = "#[path = \"../gen/mod.rs\"]\n\
         mod generated;\n\
         \n\
         use core::ffi::c_void;\n\
         use core::sync::atomic::{AtomicBool, Ordering};\n\
         use generated::host::host_contracts::HostLogger;\n\
         use generated::host::interface_factories::create_host_logger_interface;\n\
         use polyplug_abi::{AbiError, AbiErrorCode, HostContractInterface, StringView};\n\
         \n\
         static GOT_EMPTY: AtomicBool = AtomicBool::new(false);\n\
         static GOT_CALL: AtomicBool = AtomicBool::new(false);\n\
         \n\
         struct RecordingLogger;\n\
         impl HostLogger for RecordingLogger {\n\
         fn log(&self, message: &str) {\n\
         GOT_CALL.store(true, Ordering::SeqCst);\n\
         GOT_EMPTY.store(message.is_empty(), Ordering::SeqCst);\n\
         }\n\
         fn log_with_level(&self, _level: &generated::host::types::LogLevel, _message: &str) {}\n\
         }\n\
         \n\
         fn main() {\n\
         let interface: &'static HostContractInterface =\n\
         create_host_logger_interface(Box::new(RecordingLogger));\n\
         // The thunk's first arg is the impl pointer the create_instance stub\n\
         // derives from user_data. Reach it the same way the host runtime would.\n\
         let impl_ptr: *const c_void = interface.user_data as *const c_void;\n\
         // SAFETY: native dispatch is active; functions[0] is the `log` thunk.\n\
         let log_thunk: unsafe extern \"C\" fn(*const c_void, *const (), *mut ()) -> AbiError = unsafe {\n\
         let fns: *const *const () = interface.dispatch.native.functions;\n\
         core::mem::transmute(*fns.add(0))\n\
         };\n\
         // The crux: pass a NULL StringView (ptr=null, len=0) as the `message` arg.\n\
         let arg: StringView = StringView::null();\n\
         // SAFETY: impl_ptr is the interface's impl; arg points to a valid (null) StringView.\n\
         let err: AbiError = unsafe {\n\
         log_thunk(\n\
         impl_ptr,\n\
         &arg as *const StringView as *const (),\n\
         core::ptr::null_mut(),\n\
         )\n\
         };\n\
         assert_eq!(err.code, AbiErrorCode::Ok as u32, \"thunk must return Ok for a null StringView\");\n\
         assert!(GOT_CALL.load(Ordering::SeqCst), \"impl.log must have been called\");\n\
         assert!(GOT_EMPTY.load(Ordering::SeqCst), \"null StringView must decode to an empty &str\");\n\
         println!(\"OK: null StringView round-tripped to empty &str\");\n\
         }\n";
    std::fs::write(project_dir.join("src/main.rs"), main_rs).expect("write src/main.rs");

    let target_dir: PathBuf = tmp.path().join("target");
    let run: std::process::Output = Command::new(env!("CARGO"))
        .arg("run")
        .arg("--manifest-path")
        .arg(project_dir.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", &target_dir)
        .output()
        .expect("failed to spawn cargo run for the host-thunk driver");
    assert!(
        run.status.success(),
        "host-thunk empty-StringView driver failed (status {:?})\n--- stdout ---\n{}\n--- stderr ---\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr),
    );
    assert!(
        String::from_utf8_lossy(&run.stdout).contains("OK: null StringView round-tripped"),
        "driver must report the empty round-trip succeeded, got:\n{}",
        String::from_utf8_lossy(&run.stdout),
    );
}
