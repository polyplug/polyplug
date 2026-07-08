//! End-to-end proof for `Array<T>` returns (`polyplugc generate --lang rust`).
//!
//! `Array<Inner>` desugars to a generated `ArrayOf_Inner { items, len }` wrapper
//! struct (see `crates/polyplugc/src/ir.rs`). This test proves the whole native
//! return path round-trips byte-correct, not merely that the text compiles:
//!
//!   1. generate the guest glue for a contract returning `Array<Proc>` where
//!      `Proc` embeds a `StringView`,
//!   2. BUILD AND RUN a driver that implements the guest trait using the guest
//!      SDK's `ReturnArena` (`alloc_str` + `alloc_array`), calls it, and reads the
//!      returned wrapper back through the generated `ArrayOf_Proc::as_slice`,
//!   3. assert every element field — including the embedded string bytes — matches.
//!
//! This is the P1 de-risking proof for the CheatGear migration: an array of
//! structs with embedded strings is exactly the `enum_processes` /
//! `enum_modules` return shape.
//!
//! Run with:
//!   cargo test --test generate_e2e_array --package polyplugc

#![allow(clippy::expect_used)]

use std::ffi::OsStr;
use std::fs;
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

fn rust_guest_sdk_path() -> PathBuf {
    repo_root().join("sdks").join("rust").join("guest")
}

fn polyplug_abi_path() -> PathBuf {
    repo_root().join("crates").join("polyplug_abi")
}

fn polyplug_utils_path() -> PathBuf {
    repo_root().join("crates").join("polyplug_utils")
}

fn cpp_abi_include() -> PathBuf {
    repo_root().join("sdks").join("cpp").join("abi")
}

fn cpp_guest_include() -> PathBuf {
    repo_root().join("sdks").join("cpp").join("guest")
}

/// Run the `polyplugc` binary with `args`, returning the captured output.
fn run_polyplugc(args: &[&OsStr]) -> Output {
    let bin: &str = env!("CARGO_BIN_EXE_polyplugc");
    Command::new(bin)
        .args(args)
        .output()
        .expect("failed to spawn polyplugc binary")
}

/// Escape a path for embedding inside a double-quoted TOML/Rust string.
fn dep_path(p: PathBuf) -> String {
    p.display().to_string().replace('\\', "\\\\")
}

const API_TOML: &str = r#"
[[types]]
name = "Proc"
fields = [
    { name = "id", type = "u32" },
    { name = "name", type = "StringView" },
]

[[plugin_contract]]
name = "sys.Enumerator"
version = "1.0.0"

[[plugin_contract.functions]]
name = "enum_procs"
return = "Array<Proc>"
"#;

const BUNDLE_TOML: &str = r#"
[bundle]
name = "sys_enum"
version = "1.0.0"
api = "api.toml"
loader = "native"

[bundle.file]
linux.x86_64 = "libenumerator.so"

[[plugin]]
name = "enumerator"
implements = ["sys.Enumerator@1.0"]
"#;

/// The only hand-written source. It includes the generated guest glue verbatim,
/// implements the trait with the SDK's `ReturnArena`, then calls the method and
/// reads the returned array back through the generated `as_slice` — asserting
/// every field, so a layout or marshaling regression fails the run.
const MAIN_RS: &str = r##"#[path = "../gen/guest/mod.rs"]
mod generated;

use core::slice;
use core::str;
use std::sync::Mutex;

use generated::contracts::SysEnumeratorGuestContract;
use generated::types::{ArrayOf_Proc, Proc};
use polyplug_abi::StringView;
use polyplug_guest::{GuestError, HostContext, ReturnArena};

/// Plugin state: one reused return buffer per instance. The guest trait is
/// `Send + Sync`, and a per-instance return buffer needs interior mutability
/// (the method takes `&self`), so it lives behind a `Mutex` — which is `Sync`
/// because `ReturnArena: Send`.
struct Plugin {
    arena: Mutex<ReturnArena>,
}

impl SysEnumeratorGuestContract for Plugin {
    fn enum_procs(&self) -> Result<ArrayOf_Proc, GuestError> {
        let mut arena = self.arena.lock().expect("arena lock");
        // Reclaim the previous call's return before building this one.
        arena.reset();
        let procs = [
            Proc { id: 7, name: arena.alloc_str("cs2.exe") },
            Proc { id: 42, name: arena.alloc_str("game.exe") },
        ];
        let (items, len) = arena.alloc_array(&procs);
        Ok(ArrayOf_Proc { items, len })
    }
}

#[unsafe(no_mangle)]
pub fn polyplug_create_enumerator(host: HostContext) -> Box<dyn SysEnumeratorGuestContract> {
    Box::new(Plugin {
        arena: Mutex::new(ReturnArena::new(host, 4096)),
    })
}

fn sv_to_string(s: StringView) -> String {
    if s.ptr.is_null() || s.len == 0 {
        return String::new();
    }
    // SAFETY: `s` was produced by `ReturnArena::alloc_str` from valid UTF-8 and
    // its buffer is still alive (no reset since), so the bytes are readable.
    let bytes: &[u8] = unsafe { slice::from_raw_parts(s.ptr, s.len) };
    str::from_utf8(bytes).expect("utf8").to_owned()
}

fn main() {
    // A null host is sound here: the 4 KiB primary buffer never overflows for
    // this return, so the host pointer (used only for overflow alloc/free) is
    // never dereferenced.
    // SAFETY: null host pointer, never dereferenced (no overflow).
    let host: HostContext = unsafe { HostContext::new(core::ptr::null()) };
    let plugin: Box<dyn SysEnumeratorGuestContract> = polyplug_create_enumerator(host);

    let wrapper: ArrayOf_Proc = plugin.enum_procs().expect("enum_procs must return Ok");
    // SAFETY: `items`/`len` came from `alloc_array` on the plugin's still-alive
    // arena and no reset has happened since, so the slice is valid.
    let got: &[Proc] = unsafe { wrapper.as_slice() };

    assert_eq!(wrapper.len, 2, "wrapper.len must be 2");
    assert_eq!(got.len(), 2, "as_slice must yield 2 elements");
    assert_eq!(got[0].id, 7, "proc[0].id");
    assert_eq!(got[1].id, 42, "proc[1].id");
    assert_eq!(sv_to_string(got[0].name), "cs2.exe", "proc[0].name bytes");
    assert_eq!(sv_to_string(got[1].name), "game.exe", "proc[1].name bytes");

    println!("OK: Array<Proc{{StringView}}> round-tripped byte-correct");
}
"##;

/// The only hand-written C++ source: implements the guest trait with the SDK's
/// `polyplug::ReturnArena`, then in `main` calls the method and reads the array
/// back through the generated `ArrayOf_Proc::elements()`, asserting every field.
const CPP_MAIN: &str = r##"#include "guest/init.hpp"
#include <polyplug/guest.hpp>
#include <cassert>
#include <cstdio>
#include <string>

namespace polyplug_plugin {
class EnumImpl : public SysEnumeratorGuestContract {
public:
    EnumImpl() : arena_(4096) {}
    polyplug_generated::ArrayOf_Proc enum_procs() override {
        arena_.reset();
        polyplug_generated::Proc procs[2];
        procs[0].id = 7;
        procs[0].name = arena_.alloc_str("cs2.exe");
        procs[1].id = 42;
        procs[1].name = arena_.alloc_str("game.exe");
        polyplug::ArrayRef ref = arena_.alloc_array(procs, 2);
        return polyplug_generated::ArrayOf_Proc{ref.items, ref.len};
    }
private:
    polyplug::ReturnArena arena_;
};
SysEnumeratorGuestContract* polyplug_create_enumerator(const HostApi*) { return new EnumImpl(); }
}  // namespace polyplug_plugin

static std::string sv_str(const StringView& s) {
    if (s.ptr == nullptr || s.len == 0) return std::string();
    return std::string(reinterpret_cast<const char*>(s.ptr), s.len);
}

int main() {
    polyplug_plugin::SysEnumeratorGuestContract* impl =
        polyplug_plugin::polyplug_create_enumerator(nullptr);
    polyplug_generated::ArrayOf_Proc arr = impl->enum_procs();
    assert(arr.len == 2);
    const polyplug_generated::Proc* els = arr.elements();
    assert(els[0].id == 7);
    assert(els[1].id == 42);
    assert(sv_str(els[0].name) == "cs2.exe");
    assert(sv_str(els[1].name) == "game.exe");
    std::printf("OK: Array<Proc{StringView}> round-tripped byte-correct\n");
    delete impl;
    return 0;
}
"##;

/// Write `api.toml` + `bundle.toml` into `project_dir` and generate the guest
/// glue for `lang` into `gen_dir`, asserting the CLI succeeds.
fn generate_array_bundle(project_dir: &Path, gen_dir: &Path, lang: &str) {
    fs::write(project_dir.join("api.toml"), API_TOML).expect("write api.toml");
    fs::write(project_dir.join("bundle.toml"), BUNDLE_TOML).expect("write bundle.toml");
    let output: Output = run_polyplugc(&[
        "generate".as_ref(),
        "--bundle".as_ref(),
        project_dir.join("bundle.toml").as_os_str(),
        "--lang".as_ref(),
        lang.as_ref(),
        "--out".as_ref(),
        gen_dir.as_os_str(),
    ]);
    assert!(
        output.status.success(),
        "polyplugc generate --lang {lang} failed (status {:?})\n--- stdout ---\n{}\n--- stderr ---\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// C++: generate → compile an executable that returns Array<Proc> and reads it
// back through the generated `elements()` accessor → run → assert byte-correct.
// `c++` is guaranteed by CI (examples/build_all.sh), so a missing toolchain is a
// hard failure, matching generate_e2e_native.rs.
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn cpp_array_of_struct_with_string_round_trips() {
    let tmp: TempDir = tempdir().expect("tempdir");
    let project_dir: PathBuf = tmp.path().join("plugin");
    let gen_dir: PathBuf = project_dir.join("gen");
    fs::create_dir_all(&project_dir).expect("create project dir");

    generate_array_bundle(&project_dir, &gen_dir, "cpp");
    assert!(
        gen_dir.join("guest/types.hpp").exists(),
        "generated guest/types.hpp must exist at {}",
        gen_dir.join("guest/types.hpp").display()
    );

    let main_cpp: PathBuf = project_dir.join("driver.cpp");
    fs::write(&main_cpp, CPP_MAIN).expect("write driver.cpp");
    let exe: PathBuf = project_dir.join("driver");

    let build: Output = Command::new("c++")
        .arg("-std=c++20")
        .arg("-O0")
        .arg("-I")
        .arg(&gen_dir)
        .arg("-I")
        .arg(cpp_abi_include())
        .arg("-I")
        .arg(cpp_guest_include())
        .arg(&main_cpp)
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("failed to spawn c++ compiler");
    assert!(
        build.status.success(),
        "c++ build of the array driver failed (status {:?})\n--- stdout ---\n{}\n--- stderr ---\n{}",
        build.status.code(),
        String::from_utf8_lossy(&build.stdout),
        String::from_utf8_lossy(&build.stderr),
    );

    let run: Output = Command::new(&exe)
        .output()
        .expect("failed to run the compiled array driver");
    assert!(
        run.status.success(),
        "C++ Array<Proc> driver failed (status {:?})\n--- stdout ---\n{}\n--- stderr ---\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr),
    );
    assert!(
        String::from_utf8_lossy(&run.stdout).contains("round-tripped byte-correct"),
        "C++ driver must report success, got:\n{}",
        String::from_utf8_lossy(&run.stdout),
    );
}

#[test]
fn array_of_struct_with_string_round_trips() {
    let tmp: TempDir = tempdir().expect("tempdir");
    let project_dir: PathBuf = tmp.path().join("driver");
    let gen_dir: PathBuf = project_dir.join("gen");
    fs::create_dir_all(project_dir.join("src")).expect("create src dir");

    // Write the contract + bundle the CLI consumes.
    fs::write(project_dir.join("api.toml"), API_TOML).expect("write api.toml");
    fs::write(project_dir.join("bundle.toml"), BUNDLE_TOML).expect("write bundle.toml");

    // Generate the guest glue (guest trait + ArrayOf_Proc + as_slice) into <driver>/gen.
    let output: Output = run_polyplugc(&[
        "generate".as_ref(),
        "--bundle".as_ref(),
        project_dir.join("bundle.toml").as_os_str(),
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
        gen_dir.join("guest/types.rs").exists(),
        "generated guest/types.rs must exist at {}",
        gen_dir.join("guest/types.rs").display()
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
         polyplug_guest = {{ path = \"{}\" }}\n\
         polyplug_utils = {{ path = \"{}\" }}\n",
        dep_path(polyplug_abi_path()),
        dep_path(rust_guest_sdk_path()),
        dep_path(polyplug_utils_path()),
    );
    fs::write(project_dir.join("Cargo.toml"), cargo_toml).expect("write Cargo.toml");
    fs::write(project_dir.join("src/main.rs"), MAIN_RS).expect("write src/main.rs");

    let target_dir: PathBuf = tmp.path().join("target");
    let run: Output = Command::new(env!("CARGO"))
        .arg("run")
        .arg("--manifest-path")
        .arg(project_dir.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", &target_dir)
        .output()
        .expect("failed to spawn cargo run for the array driver");
    assert!(
        run.status.success(),
        "Array<Proc> round-trip driver failed (status {:?})\n--- stdout ---\n{}\n--- stderr ---\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr),
    );
    assert!(
        String::from_utf8_lossy(&run.stdout).contains("round-tripped byte-correct"),
        "driver must report the array round-trip succeeded, got:\n{}",
        String::from_utf8_lossy(&run.stdout),
    );
}
