//! End-to-end load/check proofs for the VM-targeted generators (`python`,
//! `lua`, `js-quickjs`).
//!
//! The Rust reference (`generate_e2e.rs`) proves the native generator output
//! *compiles* with zero hand edits. This file proves the equivalent for the
//! three VM languages, using each language's own toolchain exactly the way the
//! runtime loader and `examples/build_all.sh` wire it:
//!
//! 1. **Python** — every generated `.py` passes `python3 -m py_compile`, and the
//!    generated package *imports cleanly* with the in-tree SDK roots on
//!    `sys.path` (mirroring `polyplug_python`'s loader: bundle dir +
//!    `site-packages`, plus the `generated/` dir so `guest.types` resolves).
//!    Import-time failures (missing imports, wrong field names) are generator
//!    defects.
//!
//! 2. **Lua** — every generated `.lua` compiles under LuaJIT (`luajit -bl`,
//!    required because the generated code does `require("ffi")`), and each
//!    module `require`s cleanly with `package.path` pointing at the generated
//!    dir + `sdks/lua/guest` + `sdks/lua/abi` (mirroring `polyplug_lua`'s
//!    loader).
//!
//! 3. **JS (QuickJS target, TypeScript output)** — every generated `.ts`
//!    type-checks with `deno check`. Generated guest modules use extensionless
//!    ESM relative imports (the QuickJS/rolldown convention this repo ships), so
//!    the check runs with `--sloppy-imports` to resolve them the same way the
//!    bundler does. All generated imports are local (`./…`); none reference an
//!    external module, so no import map is needed.
//!
//! Toolchains (`python3`, `luajit`, `deno`) are present locally and in CI — the
//! same jobs that build the examples use all three. There is no skip-if-missing
//! logic: a missing toolchain is a hard failure, not a silent pass.
//!
//! Run with:
//!   cargo test --test generate_e2e_vm --package polyplugc

#![allow(clippy::expect_used)]

use std::ffi::OsStr;
use std::ffi::OsString;
use std::fs;
use std::fs::ReadDir;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Output;
use tempfile::TempDir;
use tempfile::tempdir;

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

/// Absolute path to the `transformer` guest bundle for `lang`. The transformer
/// exercises a `StringView`-in/`StringView`-out contract function plus the
/// shared host-contract callers (`host.logger`, incl. an enum parameter), so it
/// covers a broad slice of the VM generators' surface.
fn transformer_bundle_toml(lang_dir: &str) -> PathBuf {
    repo_root()
        .join("examples")
        .join("guests")
        .join(lang_dir)
        .join("transformer")
        .join("bundle.toml")
}

/// Run the `polyplugc` binary with `args`, returning the captured output.
fn run_polyplugc(args: &[&OsStr]) -> Output {
    let bin: &str = env!("CARGO_BIN_EXE_polyplugc");
    Command::new(bin)
        .args(args)
        .output()
        .expect("failed to spawn polyplugc binary")
}

/// Generate guest glue for `bundle` in `lang` into `out_dir` via the CLI,
/// asserting success and surfacing full stderr on failure.
fn generate_guest_glue(bundle: &Path, lang: &str, out_dir: &Path) {
    let output: Output = run_polyplugc(&[
        "generate".as_ref(),
        "--bundle".as_ref(),
        bundle.as_os_str(),
        "--lang".as_ref(),
        lang.as_ref(),
        "--out".as_ref(),
        out_dir.as_os_str(),
    ]);
    assert!(
        output.status.success(),
        "polyplugc generate --lang {lang} failed (status {:?})\n--- stdout ---\n{}\n--- stderr ---\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

/// Collect every file under `dir` (recursively) whose extension equals `ext`.
fn files_with_ext(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    let entries: ReadDir = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return out,
    };
    for entry in entries.filter_map(Result::ok) {
        let path: PathBuf = entry.path();
        if path.is_dir() {
            out.extend(files_with_ext(&path, ext));
        } else if path.extension().and_then(|s| s.to_str()) == Some(ext) {
            out.push(path);
        }
    }
    out
}

// ═══════════════════════════════════════════════════════════════════════════
// Python: generated .py files compile AND the generated package imports clean.
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn python_generated_glue_compiles_and_imports() {
    let tmp: TempDir = tempdir().expect("tempdir");
    let bundle_dir: PathBuf = tmp.path().join("bundle");
    let gen_dir: PathBuf = bundle_dir.join("generated");
    fs::create_dir_all(&bundle_dir).expect("create bundle dir");

    generate_guest_glue(&transformer_bundle_toml("python"), "python", &gen_dir);

    // py_compile every generated .py — a syntax/bytecode proof per file.
    let py_files: Vec<PathBuf> = files_with_ext(&gen_dir, "py");
    assert!(
        !py_files.is_empty(),
        "expected generated .py files under {}",
        gen_dir.display()
    );
    for file in &py_files {
        let output: Output = Command::new("python3")
            .arg("-m")
            .arg("py_compile")
            .arg(file)
            .output()
            .expect("failed to spawn python3 -m py_compile");
        assert!(
            output.status.success(),
            "python3 -m py_compile failed for {} (status {:?})\n--- stderr ---\n{}",
            file.display(),
            output.status.code(),
            String::from_utf8_lossy(&output.stderr),
        );
    }

    // Provision the in-tree SDK packages into <bundle>/site-packages, exactly as
    // `examples/build_all.sh` does and as the PythonLoader expects:
    //   polyplug_guest, polyplug_abi, and the polyplug.abi namespace shim.
    let site: PathBuf = bundle_dir.join("site-packages");
    fs::create_dir_all(site.join("polyplug").join("abi")).expect("create site polyplug/abi");
    let sdk_python: PathBuf = repo_root().join("sdks").join("python");
    copy_dir_all(
        &sdk_python.join("guest").join("polyplug_guest"),
        &site.join("polyplug_guest"),
    );
    copy_dir_all(
        &sdk_python.join("polyplug_abi").join("polyplug_abi"),
        &site.join("polyplug_abi"),
    );
    fs::copy(
        sdk_python.join("abi").join("abi.py"),
        site.join("polyplug").join("abi").join("abi.py"),
    )
    .expect("copy polyplug/abi/abi.py");
    fs::write(site.join("polyplug").join("__init__.py"), b"").expect("write polyplug init");
    fs::write(site.join("polyplug").join("abi").join("__init__.py"), b"")
        .expect("write polyplug.abi init");

    // Import every generated guest module. The loader prepends the bundle dir
    // (so `generated.guest.*` resolves) and site-packages; the generated
    // `host_contracts.py` also imports `from guest.types import …`, which needs
    // the generated dir itself on the path.
    let import_script: String = format!(
        "import sys\n\
         sys.path.insert(0, {site:?})\n\
         sys.path.insert(0, {bundle:?})\n\
         sys.path.insert(0, {gen:?})\n\
         import importlib\n\
         for m in ('generated.guest.types', 'generated.guest.contracts', \
         'generated.guest.host_contracts', 'generated.guest.init'):\n\
         \x20\x20\x20\x20importlib.import_module(m)\n",
        site = site.to_string_lossy(),
        bundle = bundle_dir.to_string_lossy(),
        gen = gen_dir.to_string_lossy(),
    );
    let output: Output = Command::new("python3")
        .arg("-c")
        .arg(&import_script)
        .output()
        .expect("failed to spawn python3 import check");
    assert!(
        output.status.success(),
        "importing generated python guest modules failed (status {:?})\n--- stderr ---\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr),
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Lua: generated .lua files compile under LuaJIT AND require cleanly.
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn lua_generated_glue_compiles_and_loads() {
    let tmp: TempDir = tempdir().expect("tempdir");
    let bundle_dir: PathBuf = tmp.path().join("bundle");
    let gen_dir: PathBuf = bundle_dir.join("generated");
    fs::create_dir_all(&bundle_dir).expect("create bundle dir");

    generate_guest_glue(&transformer_bundle_toml("lua"), "lua", &gen_dir);

    // Syntax proof: LuaJIT compiles each file to bytecode. `-bl` lists bytecode;
    // we discard the listing to /dev/null but still get a non-zero exit on any
    // syntax error. luajit (not stock lua) is required: generated code does
    // `require("ffi")`.
    let lua_files: Vec<PathBuf> = files_with_ext(&gen_dir, "lua");
    assert!(
        !lua_files.is_empty(),
        "expected generated .lua files under {}",
        gen_dir.display()
    );
    for file in &lua_files {
        let output: Output = Command::new("luajit")
            .arg("-bl")
            .arg(file)
            .arg(devnull())
            .output()
            .expect("failed to spawn luajit -bl");
        assert!(
            output.status.success(),
            "luajit failed to compile {} (status {:?})\n--- stderr ---\n{}",
            file.display(),
            output.status.code(),
            String::from_utf8_lossy(&output.stderr),
        );
    }

    // Load proof: require each generated module with package.path mirroring the
    // LuaLoader (bundle dir + sdks/lua/guest + sdks/lua/abi).
    let guest_dir: PathBuf = repo_root().join("sdks").join("lua").join("guest");
    let abi_dir: PathBuf = repo_root().join("sdks").join("lua").join("abi");
    let bundle_fwd: String = bundle_dir.to_string_lossy().replace('\\', "/");
    let guest_fwd: String = guest_dir.to_string_lossy().replace('\\', "/");
    let abi_fwd: String = abi_dir.to_string_lossy().replace('\\', "/");
    let load_script: String = format!(
        "package.path = \"{bundle_fwd}/?.lua;{bundle_fwd}/?.init.lua;{guest_fwd}/?.lua;{abi_fwd}/?.lua;\" .. package.path\n\
         local mods = {{\"generated.guest.types\", \"generated.guest.contracts\", \"generated.guest.host_contracts\"}}\n\
         for _, m in ipairs(mods) do\n\
         \x20\x20local ok, err = pcall(require, m)\n\
         \x20\x20if not ok then\n\
         \x20\x20\x20\x20io.stderr:write(\"LOAD FAIL \" .. m .. \": \" .. tostring(err) .. \"\\n\")\n\
         \x20\x20\x20\x20os.exit(1)\n\
         \x20\x20end\n\
         end\n"
    );
    let output: Output = Command::new("luajit")
        .arg("-e")
        .arg(&load_script)
        .output()
        .expect("failed to spawn luajit load check");
    assert!(
        output.status.success(),
        "requiring generated lua guest modules failed (status {:?})\n--- stderr ---\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr),
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// JS (QuickJS target): generated .ts files type-check under `deno check`.
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn js_quickjs_generated_glue_type_checks() {
    let tmp: TempDir = tempdir().expect("tempdir");
    let gen_dir: PathBuf = tmp.path().join("generated");

    generate_guest_glue(&transformer_bundle_toml("js"), "js-quickjs", &gen_dir);

    let ts_files: Vec<PathBuf> = files_with_ext(&gen_dir, "ts");
    assert!(
        !ts_files.is_empty(),
        "expected generated .ts files under {}",
        gen_dir.display()
    );

    // Type-check every generated .ts. `--sloppy-imports` lets Deno resolve the
    // extensionless relative imports the generator emits (the QuickJS/rolldown
    // convention this repo ships). All generated imports are local, so no import
    // map is needed.
    let mut args: Vec<OsString> = vec!["check".into(), "--sloppy-imports".into()];
    for file in &ts_files {
        args.push(file.clone().into_os_string());
    }
    let output: Output = Command::new("deno")
        .args(&args)
        .output()
        .expect("failed to spawn deno check");
    assert!(
        output.status.success(),
        "deno check of generated js-quickjs glue failed (status {:?})\n--- stdout ---\n{}\n--- stderr ---\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

// ─── helpers ────────────────────────────────────────────────────────────────

/// Recursively copy `src` directory into `dst` (created if absent).
fn copy_dir_all(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).expect("create dst dir");
    for entry in fs::read_dir(src)
        .expect("read src dir")
        .filter_map(Result::ok)
    {
        let path: PathBuf = entry.path();
        let target: PathBuf = dst.join(entry.file_name());
        if path.is_dir() {
            copy_dir_all(&path, &target);
        } else {
            fs::copy(&path, &target).expect("copy file");
        }
    }
}

/// The platform null-sink path used to discard `luajit -bl` listing output.
fn devnull() -> &'static str {
    if cfg!(windows) { "NUL" } else { "/dev/null" }
}
