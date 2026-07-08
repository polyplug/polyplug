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

use std::env;
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
    generate_bundle_with(project_dir, gen_dir, lang, API_TOML, BUNDLE_TOML);
}

/// Like [`generate_array_bundle`] but with caller-supplied contract + bundle
/// TOML, so a single harness can drive both the `Proc` and the all-widths
/// `Kitchen` contracts.
fn generate_bundle_with(
    project_dir: &Path,
    gen_dir: &Path,
    lang: &str,
    api_toml: &str,
    bundle_toml: &str,
) {
    fs::write(project_dir.join("api.toml"), api_toml).expect("write api.toml");
    fs::write(project_dir.join("bundle.toml"), bundle_toml).expect("write bundle.toml");
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

/// The LuaJIT driver body (package.path is prepended separately). Sets a factory
/// whose `enum_procs` returns an array of plain Lua tables, drives the generated
/// dispatch handler with a mock **align-1** arena allocator starting at an odd
/// offset (so the marshaler MUST realign for the struct elements), then reads the
/// arena-filled `ArrayOf_Proc` back and asserts every field/string.
const LUA_DRIVER_BODY: &str = r#"
local ffi = require("ffi")
require("polyplug_abi")
require("generated.guest.types")
local contracts = require("generated.guest.contracts")

-- Mock arena: an align-1 bump allocator over a Lua-owned buffer, starting at an
-- odd offset so a correct marshaler must realign before writing Proc elements.
local ARENA = ffi.new("uint8_t[?]", 1048576)
local cursor = 1
local function arena_alloc(size, _arena)
    local addr = ffi.cast("uintptr_t", ARENA) + cursor
    cursor = cursor + tonumber(size)
    return addr
end

contracts.set_enumerator_factory(function(_host)
    return {
        enum_procs = function(_self)
            return { { id = 7, name = "cs2.exe" }, { id = 42, name = "game.exe" } }
        end,
    }
end)

local regs = polyplug_init(1, 1)
local entry = regs["sys.Enumerator"]
assert(entry ~= nil, "sys.Enumerator must be registered")
local inst = entry.factory(0)
local out = ffi.new("ArrayOf_Proc[1]")
entry.functions[0](inst, 0, ffi.cast("uintptr_t", out), 0, arena_alloc)

assert(tonumber(out[0].len) == 2, "len must be 2")
local procs = ffi.cast("Proc*", out[0].items)
assert(procs[0].id == 7, "proc0 id")
assert(procs[1].id == 42, "proc1 id")
assert(ffi.string(procs[0].name.ptr, procs[0].name.len) == "cs2.exe", "proc0 name")
assert(ffi.string(procs[1].name.ptr, procs[1].name.len) == "game.exe", "proc1 name")
io.write("OK: Array<Proc{StringView}> round-tripped byte-correct\n")
"#;

// ═══════════════════════════════════════════════════════════════════════════
// Lua: generate → run under LuaJIT a driver that returns Array<Proc> from an
// ergonomic Lua array-of-tables, and assert the generated glue marshaled it into
// the (mock) arena byte-correct. luajit is guaranteed by CI.
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn lua_array_of_struct_with_string_round_trips() {
    let tmp: TempDir = tempdir().expect("tempdir");
    let project_dir: PathBuf = tmp.path().join("bundle");
    // Require path `generated.guest.*` → directory named `generated`.
    let gen_dir: PathBuf = project_dir.join("generated");
    fs::create_dir_all(&project_dir).expect("create project dir");

    generate_array_bundle(&project_dir, &gen_dir, "lua");
    assert!(
        gen_dir.join("guest/contracts.lua").exists(),
        "generated guest/contracts.lua must exist at {}",
        gen_dir.join("guest/contracts.lua").display()
    );

    let guest_dir: PathBuf = repo_root().join("sdks").join("lua").join("guest");
    let abi_dir: PathBuf = repo_root().join("sdks").join("lua").join("abi");
    let project_fwd: String = project_dir.to_string_lossy().replace('\\', "/");
    let guest_fwd: String = guest_dir.to_string_lossy().replace('\\', "/");
    let abi_fwd: String = abi_dir.to_string_lossy().replace('\\', "/");
    let driver: String = format!(
        "package.path = \"{project_fwd}/?.lua;{guest_fwd}/?.lua;{abi_fwd}/?.lua;\" .. package.path\n{LUA_DRIVER_BODY}"
    );
    let driver_path: PathBuf = project_dir.join("driver.lua");
    fs::write(&driver_path, driver).expect("write driver.lua");

    let run: Output = Command::new("luajit")
        .arg(&driver_path)
        .output()
        .expect("failed to spawn luajit");
    assert!(
        run.status.success(),
        "LuaJIT Array<Proc> driver failed (status {:?})\n--- stdout ---\n{}\n--- stderr ---\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr),
    );
    assert!(
        String::from_utf8_lossy(&run.stdout).contains("round-tripped byte-correct"),
        "LuaJIT driver must report success, got:\n{}",
        String::from_utf8_lossy(&run.stdout),
    );
}

/// Deno driver: mocks the QuickJS `bridge` over an ArrayBuffer, drives the
/// generated dispatch wrapper with a factory whose `fn0` returns an array of
/// plain JS objects, then reads the arena-filled `ArrayOf_Proc` back out of the
/// buffer and asserts each field/string. The mock arena is align-1 and starts at
/// an odd offset, so the generated marshaler MUST realign the element base.
const JS_DRIVER: &str = r#"import { ENUMERATOR_INTERFACE, setEnumeratorFactory } from "./generated/guest/contracts.ts";

const MEM = new ArrayBuffer(1 << 20);
const DV = new DataView(MEM);
let cursor = 4097; // odd, past the out slot → forces element realignment
const bridge = {
  writeU32: (p: number, v: number) => DV.setUint32(p, v >>> 0, true),
  writeI32: (p: number, v: number) => DV.setInt32(p, v | 0, true),
  writeF32: (p: number, v: number) => DV.setFloat32(p, v, true),
  writeF64: (p: number, v: number) => DV.setFloat64(p, v, true),
  writeByte: (p: number, v: number) => DV.setUint8(p, v & 0xff),
  readU32: (p: number) => DV.getUint32(p, true),
  arenaAlloc: (size: number, _arena: number) => {
    const a = cursor;
    cursor += Number(size);
    return [a % 4294967296, Math.floor(a / 4294967296)];
  },
};

setEnumeratorFactory(((_b: any, _lo: number, _hi: number) => ({
  fn0: () => [{ id: 7, name: "cs2.exe" }, { id: 42, name: "game.exe" }],
})) as any);

const impl = (ENUMERATOR_INTERFACE.factory as any)(bridge, 0, 0);
const OUT = 16;
(ENUMERATOR_INTERFACE.functions as any)[0](impl, 0, OUT, 0, bridge);

const items = DV.getUint32(OUT, true) + DV.getUint32(OUT + 4, true) * 4294967296;
const len = DV.getUint32(OUT + 8, true);
if (len !== 2) throw new Error("len must be 2, got " + len);
function readProc(base: number) {
  const id = DV.getUint32(base, true);
  const ptr = DV.getUint32(base + 8, true) + DV.getUint32(base + 12, true) * 4294967296;
  const nlen = DV.getUint32(base + 16, true);
  let s = "";
  for (let i = 0; i < nlen; i++) s += String.fromCharCode(DV.getUint8(ptr + i));
  return { id, name: s };
}
const p0 = readProc(items);
const p1 = readProc(items + 24);
if (p0.id !== 7 || p0.name !== "cs2.exe") throw new Error("proc0 wrong: " + JSON.stringify(p0));
if (p1.id !== 42 || p1.name !== "game.exe") throw new Error("proc1 wrong: " + JSON.stringify(p1));
console.log("OK: Array<Proc{StringView}> round-tripped byte-correct");
"#;

// ═══════════════════════════════════════════════════════════════════════════
// JS (QuickJS target): generate → run under Deno a driver that returns
// Array<Proc> from an ergonomic JS array-of-objects through a mock bridge, and
// assert the generated glue marshaled it into the (mock) arena byte-correct.
// deno is guaranteed by CI.
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn js_array_of_struct_with_string_round_trips() {
    let tmp: TempDir = tempdir().expect("tempdir");
    let project_dir: PathBuf = tmp.path().join("bundle");
    let gen_dir: PathBuf = project_dir.join("generated");
    fs::create_dir_all(&project_dir).expect("create project dir");

    generate_array_bundle(&project_dir, &gen_dir, "js-quickjs");
    assert!(
        gen_dir.join("guest/contracts.ts").exists(),
        "generated guest/contracts.ts must exist at {}",
        gen_dir.join("guest/contracts.ts").display()
    );

    let driver_path: PathBuf = project_dir.join("driver.ts");
    fs::write(&driver_path, JS_DRIVER).expect("write driver.ts");

    let run: Output = Command::new("deno")
        .arg("run")
        .arg("--no-lock")
        .arg(&driver_path)
        .output()
        .expect("failed to spawn deno run");
    assert!(
        run.status.success(),
        "Deno Array<Proc> driver failed (status {:?})\n--- stdout ---\n{}\n--- stderr ---\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr),
    );
    assert!(
        String::from_utf8_lossy(&run.stdout).contains("round-tripped byte-correct"),
        "Deno driver must report success, got:\n{}",
        String::from_utf8_lossy(&run.stdout),
    );
}

/// Python driver: mocks the loader's `arena_alloc` over a ctypes buffer (align-1,
/// odd start → forces the marshaler's realign), calls the generated dispatch
/// callable with an impl whose `enum_procs` returns a list of ergonomic objects,
/// then reads the arena-filled `ArrayOf_Proc` back and asserts each field/string.
const PY_DRIVER: &str = r#"import ctypes
from types import SimpleNamespace
from guest.types import ArrayOf_Proc, Proc
from guest.contracts import enumerator_enum_procs_abi

BUF = ctypes.create_string_buffer(1 << 20)
BASE = ctypes.addressof(BUF)
_cursor = [1]  # odd start → forces element realignment


def arena_alloc(size, _arena):
    a = BASE + _cursor[0]
    _cursor[0] += int(size)
    return a


impl = SimpleNamespace(enum_procs=lambda: [
    SimpleNamespace(id=7, name="cs2.exe"),
    SimpleNamespace(id=42, name="game.exe"),
])

out = ArrayOf_Proc()
enumerator_enum_procs_abi(impl, 0, ctypes.addressof(out), 0, arena_alloc)

assert out.len == 2, "len=%d" % out.len
esize = ctypes.sizeof(Proc)


def read_proc(i):
    p = Proc.from_address(out.items + i * esize)
    return (p.id, ctypes.string_at(p.name.ptr, p.name.len).decode())


assert read_proc(0) == (7, "cs2.exe"), read_proc(0)
assert read_proc(1) == (42, "game.exe"), read_proc(1)
print("OK: Array<Proc{StringView}> round-tripped byte-correct")
"#;

// ═══════════════════════════════════════════════════════════════════════════
// Python: generate → run under python3 a driver that returns Array<Proc> from an
// ergonomic list of objects, and assert the generated ctypes glue marshaled it
// into the (mock) arena byte-correct. python3 is guaranteed by CI.
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn python_array_of_struct_with_string_round_trips() {
    let tmp: TempDir = tempdir().expect("tempdir");
    let project_dir: PathBuf = tmp.path().join("bundle");
    let gen_dir: PathBuf = project_dir.join("gen");
    fs::create_dir_all(&project_dir).expect("create project dir");

    generate_array_bundle(&project_dir, &gen_dir, "python");
    assert!(
        gen_dir.join("guest/contracts.py").exists(),
        "generated guest/contracts.py must exist at {}",
        gen_dir.join("guest/contracts.py").display()
    );

    let driver_path: PathBuf = project_dir.join("driver.py");
    fs::write(&driver_path, PY_DRIVER).expect("write driver.py");

    // `polyplug_abi.abi` re-exports `polyplug.abi.abi`, so provision that shim
    // package (as examples/build_all.sh and the PythonLoader do).
    let sdk: PathBuf = repo_root().join("sdks").join("python");
    let shim: PathBuf = project_dir.join("shim");
    fs::create_dir_all(shim.join("polyplug").join("abi")).expect("create shim polyplug/abi");
    fs::write(shim.join("polyplug").join("__init__.py"), b"").expect("write polyplug init");
    fs::write(shim.join("polyplug").join("abi").join("__init__.py"), b"")
        .expect("write polyplug.abi init");
    fs::copy(
        sdk.join("abi").join("abi.py"),
        shim.join("polyplug").join("abi").join("abi.py"),
    )
    .expect("copy polyplug/abi/abi.py");

    // `polyplug_abi` / `polyplug_guest` resolve from the in-tree SDK source dirs;
    // `guest.*` from the generated dir; `polyplug.abi` from the shim.
    let pythonpath: String = env::join_paths([
        gen_dir.clone(),
        sdk.join("guest"),
        sdk.join("polyplug_abi"),
        shim,
    ])
    .expect("join PYTHONPATH")
    .to_string_lossy()
    .into_owned();

    let run: Output = Command::new("python3")
        .arg(&driver_path)
        .env("PYTHONPATH", &pythonpath)
        .output()
        .expect("failed to spawn python3");
    assert!(
        run.status.success(),
        "python3 Array<Proc> driver failed (status {:?})\n--- stdout ---\n{}\n--- stderr ---\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr),
    );
    assert!(
        String::from_utf8_lossy(&run.stdout).contains("round-tripped byte-correct"),
        "python3 driver must report success, got:\n{}",
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

// ═══════════════════════════════════════════════════════════════════════════
// Kitchen-sink coverage: `Array<Kitchen>` where `Kitchen` carries one field of
// every primitive width/signedness/float plus two embedded `StringView`s. Each
// language marshals the SAME boundary values (u8::MAX, i16/i32 MIN & MAX,
// u64::MAX, a unicode string, an EMPTY string) so every distinct field-writer
// branch and the realign/stride math are exercised, not just u32+StringView.
//
// Every guest contract also exposes `empty()` returning a zero-length array, so
// the len==0 path (no element alloc, no string loop) is proven not to crash or
// mis-marshal in every language.
//
// Field order pins a known C layout (declared order, natural alignment):
//   flag bool@0  b8 u8@1  i16v i16@2  i32v i32@4  u64v u64@8
//   f64v f64@16  f32v f32@24  name SV@32  tag SV@48   → sizeof 64, stride 64.
// ═══════════════════════════════════════════════════════════════════════════

const KITCHEN_API_TOML: &str = r#"
[[types]]
name = "Kitchen"
fields = [
    { name = "flag", type = "bool" },
    { name = "b8",   type = "u8" },
    { name = "i16v", type = "i16" },
    { name = "i32v", type = "i32" },
    { name = "u64v", type = "u64" },
    { name = "f64v", type = "f64" },
    { name = "f32v", type = "f32" },
    { name = "name", type = "StringView" },
    { name = "tag",  type = "StringView" },
]

[[plugin_contract]]
name = "sys.Kitchen"
version = "1.0.0"

[[plugin_contract.functions]]
name = "make"
return = "Array<Kitchen>"

[[plugin_contract.functions]]
name = "empty"
return = "Array<Kitchen>"
"#;

const KITCHEN_BUNDLE_TOML: &str = r#"
[bundle]
name = "sys_kitchen"
version = "1.0.0"
api = "api.toml"
loader = "native"

[bundle.file]
linux.x86_64 = "libkitchen.so"

[[plugin]]
name = "kitchen"
implements = ["sys.Kitchen@1.0"]
"#;

/// Rust driver for the kitchen-sink contract. Builds two `Kitchen` elements with
/// boundary values (incl. a unicode `name` and an empty `tag`) into the SDK's
/// `ReturnArena`, reads them back through the generated `ArrayOf_Kitchen::as_slice`,
/// and asserts every field byte-for-byte. Also proves `empty()` yields len==0.
const KITCHEN_MAIN_RS: &str = r##"#[path = "../gen/guest/mod.rs"]
mod generated;

use core::slice;
use core::str;
use std::sync::Mutex;

use generated::contracts::SysKitchenGuestContract;
use generated::types::{ArrayOf_Kitchen, Kitchen};
use polyplug_abi::StringView;
use polyplug_guest::{GuestError, HostContext, ReturnArena};

struct Plugin {
    arena: Mutex<ReturnArena>,
}

impl SysKitchenGuestContract for Plugin {
    fn make(&self) -> Result<ArrayOf_Kitchen, GuestError> {
        let mut arena = self.arena.lock().expect("arena lock");
        arena.reset();
        let rows = [
            Kitchen {
                flag: true,
                b8: u8::MAX,
                i16v: i16::MIN,
                i32v: i32::MIN,
                u64v: u64::MAX,
                f64v: 3.141592653589793_f64,
                f32v: 1.5_f32,
                name: arena.alloc_str("café.exe"),
                tag: arena.alloc_str(""),
            },
            Kitchen {
                flag: false,
                b8: 0,
                i16v: i16::MAX,
                i32v: i32::MAX,
                u64v: 0,
                f64v: -2.5_f64,
                f32v: -0.25_f32,
                name: arena.alloc_str("x"),
                tag: arena.alloc_str("tag2"),
            },
        ];
        let (items, len) = arena.alloc_array(&rows);
        Ok(ArrayOf_Kitchen { items, len })
    }

    fn empty(&self) -> Result<ArrayOf_Kitchen, GuestError> {
        let mut arena = self.arena.lock().expect("arena lock");
        arena.reset();
        let rows: [Kitchen; 0] = [];
        let (items, len) = arena.alloc_array(&rows);
        Ok(ArrayOf_Kitchen { items, len })
    }
}

#[unsafe(no_mangle)]
pub fn polyplug_create_kitchen(host: HostContext) -> Box<dyn SysKitchenGuestContract> {
    Box::new(Plugin {
        arena: Mutex::new(ReturnArena::new(host, 8192)),
    })
}

fn sv_to_string(s: StringView) -> String {
    if s.ptr.is_null() || s.len == 0 {
        return String::new();
    }
    // SAFETY: `s` came from `ReturnArena::alloc_str` (valid UTF-8, still alive).
    let bytes: &[u8] = unsafe { slice::from_raw_parts(s.ptr, s.len) };
    str::from_utf8(bytes).expect("utf8").to_owned()
}

fn main() {
    // SAFETY: null host, never dereferenced (8 KiB buffer never overflows here).
    let host: HostContext = unsafe { HostContext::new(core::ptr::null()) };
    let plugin: Box<dyn SysKitchenGuestContract> = polyplug_create_kitchen(host);

    let wrapper: ArrayOf_Kitchen = plugin.make().expect("make must return Ok");
    // SAFETY: items/len from `alloc_array` on the still-alive arena (no reset since).
    let got: &[Kitchen] = unsafe { wrapper.as_slice() };
    assert_eq!(wrapper.len, 2, "wrapper.len");
    assert_eq!(got.len(), 2, "as_slice len");

    assert!(got[0].flag, "row0.flag");
    assert_eq!(got[0].b8, u8::MAX, "row0.b8");
    assert_eq!(got[0].i16v, i16::MIN, "row0.i16v");
    assert_eq!(got[0].i32v, i32::MIN, "row0.i32v");
    assert_eq!(got[0].u64v, u64::MAX, "row0.u64v");
    assert_eq!(got[0].f64v, 3.141592653589793_f64, "row0.f64v");
    assert_eq!(got[0].f32v, 1.5_f32, "row0.f32v");
    assert_eq!(sv_to_string(got[0].name), "café.exe", "row0.name");
    assert_eq!(sv_to_string(got[0].tag), "", "row0.tag (empty)");

    assert!(!got[1].flag, "row1.flag");
    assert_eq!(got[1].b8, 0, "row1.b8");
    assert_eq!(got[1].i16v, i16::MAX, "row1.i16v");
    assert_eq!(got[1].i32v, i32::MAX, "row1.i32v");
    assert_eq!(got[1].u64v, 0, "row1.u64v");
    assert_eq!(got[1].f64v, -2.5_f64, "row1.f64v");
    assert_eq!(got[1].f32v, -0.25_f32, "row1.f32v");
    assert_eq!(sv_to_string(got[1].name), "x", "row1.name");
    assert_eq!(sv_to_string(got[1].tag), "tag2", "row1.tag");

    let empty: ArrayOf_Kitchen = plugin.empty().expect("empty must return Ok");
    assert_eq!(empty.len, 0, "empty().len must be 0");

    println!("OK: Array<Kitchen{{all-widths}}> round-tripped byte-correct");
}
"##;

#[test]
fn rust_kitchen_all_widths_round_trips() {
    let tmp: TempDir = tempdir().expect("tempdir");
    let project_dir: PathBuf = tmp.path().join("driver");
    let gen_dir: PathBuf = project_dir.join("gen");
    fs::create_dir_all(project_dir.join("src")).expect("create src dir");

    generate_bundle_with(
        &project_dir,
        &gen_dir,
        "rust",
        KITCHEN_API_TOML,
        KITCHEN_BUNDLE_TOML,
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
    fs::write(project_dir.join("src/main.rs"), KITCHEN_MAIN_RS).expect("write src/main.rs");

    let target_dir: PathBuf = tmp.path().join("target");
    let run: Output = Command::new(env!("CARGO"))
        .arg("run")
        .arg("--manifest-path")
        .arg(project_dir.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", &target_dir)
        .output()
        .expect("failed to spawn cargo run for the kitchen driver");
    assert!(
        run.status.success(),
        "Kitchen all-widths driver failed (status {:?})\n--- stdout ---\n{}\n--- stderr ---\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr),
    );
    assert!(
        String::from_utf8_lossy(&run.stdout).contains("round-tripped byte-correct"),
        "driver must report success, got:\n{}",
        String::from_utf8_lossy(&run.stdout),
    );
}

/// LuaJIT kitchen-sink driver body. Returns arrays of ergonomic Lua tables
/// (u64::MAX passed as a `ULL` cdata so it survives exactly), drives both
/// `make` (fn 0) and `empty` (fn 1) through a mock align-1 arena at an odd
/// offset, then reads each `Kitchen` back via `ffi.cast("Kitchen*")`.
const LUA_KITCHEN_BODY: &str = r#"
local ffi = require("ffi")
require("polyplug_abi")
require("generated.guest.types")
local contracts = require("generated.guest.contracts")

local ARENA = ffi.new("uint8_t[?]", 1048576)
local cursor = 1
local function arena_alloc(size, _arena)
    local addr = ffi.cast("uintptr_t", ARENA) + cursor
    cursor = cursor + tonumber(size)
    return addr
end

contracts.set_kitchen_factory(function(_host)
    return {
        make = function(_self)
            return {
                { flag = true,  b8 = 255, i16v = -32768, i32v = -2147483648,
                  u64v = 18446744073709551615ULL, f64v = 3.141592653589793,
                  f32v = 1.5, name = "café.exe", tag = "" },
                { flag = false, b8 = 0,   i16v = 32767,  i32v = 2147483647,
                  u64v = 0ULL, f64v = -2.5, f32v = -0.25, name = "x", tag = "tag2" },
            }
        end,
        empty = function(_self) return {} end,
    }
end)

local regs = polyplug_init(1, 1)
local entry = regs["sys.Kitchen"]
assert(entry ~= nil, "sys.Kitchen must be registered")
local inst = entry.factory(0)

local out = ffi.new("ArrayOf_Kitchen[1]")
entry.functions[0](inst, 0, ffi.cast("uintptr_t", out), 0, arena_alloc)
assert(tonumber(out[0].len) == 2, "make len must be 2")
local rows = ffi.cast("Kitchen*", out[0].items)

assert(rows[0].flag == true, "row0.flag")
assert(rows[0].b8 == 255, "row0.b8")
assert(rows[0].i16v == -32768, "row0.i16v")
assert(rows[0].i32v == -2147483648, "row0.i32v")
assert(rows[0].u64v == 18446744073709551615ULL, "row0.u64v")
assert(rows[0].f64v == 3.141592653589793, "row0.f64v")
assert(rows[0].f32v == 1.5, "row0.f32v")
assert(ffi.string(rows[0].name.ptr, rows[0].name.len) == "café.exe", "row0.name")
assert(tonumber(rows[0].tag.len) == 0, "row0.tag empty")

assert(rows[1].flag == false, "row1.flag")
assert(rows[1].b8 == 0, "row1.b8")
assert(rows[1].i16v == 32767, "row1.i16v")
assert(rows[1].i32v == 2147483647, "row1.i32v")
assert(rows[1].u64v == 0ULL, "row1.u64v")
assert(rows[1].f64v == -2.5, "row1.f64v")
assert(rows[1].f32v == -0.25, "row1.f32v")
assert(ffi.string(rows[1].name.ptr, rows[1].name.len) == "x", "row1.name")
assert(ffi.string(rows[1].tag.ptr, rows[1].tag.len) == "tag2", "row1.tag")

local out2 = ffi.new("ArrayOf_Kitchen[1]")
entry.functions[1](inst, 0, ffi.cast("uintptr_t", out2), 0, arena_alloc)
assert(tonumber(out2[0].len) == 0, "empty len must be 0")

io.write("OK: Array<Kitchen{all-widths}> round-tripped byte-correct\n")
"#;

#[test]
fn lua_kitchen_all_widths_round_trips() {
    let tmp: TempDir = tempdir().expect("tempdir");
    let project_dir: PathBuf = tmp.path().join("bundle");
    let gen_dir: PathBuf = project_dir.join("generated");
    fs::create_dir_all(&project_dir).expect("create project dir");

    generate_bundle_with(
        &project_dir,
        &gen_dir,
        "lua",
        KITCHEN_API_TOML,
        KITCHEN_BUNDLE_TOML,
    );
    assert!(
        gen_dir.join("guest/contracts.lua").exists(),
        "generated guest/contracts.lua must exist"
    );

    let guest_dir: PathBuf = repo_root().join("sdks").join("lua").join("guest");
    let abi_dir: PathBuf = repo_root().join("sdks").join("lua").join("abi");
    let project_fwd: String = project_dir.to_string_lossy().replace('\\', "/");
    let guest_fwd: String = guest_dir.to_string_lossy().replace('\\', "/");
    let abi_fwd: String = abi_dir.to_string_lossy().replace('\\', "/");
    let driver: String = format!(
        "package.path = \"{project_fwd}/?.lua;{guest_fwd}/?.lua;{abi_fwd}/?.lua;\" .. package.path\n{LUA_KITCHEN_BODY}"
    );
    let driver_path: PathBuf = project_dir.join("driver.lua");
    fs::write(&driver_path, driver).expect("write driver.lua");

    let run: Output = Command::new("luajit")
        .arg(&driver_path)
        .output()
        .expect("failed to spawn luajit");
    assert!(
        run.status.success(),
        "LuaJIT Kitchen driver failed (status {:?})\n--- stdout ---\n{}\n--- stderr ---\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr),
    );
    assert!(
        String::from_utf8_lossy(&run.stdout).contains("round-tripped byte-correct"),
        "LuaJIT driver must report success, got:\n{}",
        String::from_utf8_lossy(&run.stdout),
    );
}

/// C++ kitchen-sink driver: builds two `Kitchen` rows with boundary values into
/// the SDK `ReturnArena`, reads them back through the generated `elements()`.
const CPP_KITCHEN_MAIN: &str = r##"#include "guest/init.hpp"
#include <polyplug/guest.hpp>
#include <cassert>
#include <cstdint>
#include <cstdio>
#include <string>

namespace polyplug_plugin {
class KitchenImpl : public SysKitchenGuestContract {
public:
    KitchenImpl() : arena_(8192) {}
    polyplug_generated::ArrayOf_Kitchen make() override {
        arena_.reset();
        polyplug_generated::Kitchen rows[2];
        rows[0].flag = true;
        rows[0].b8 = 255;
        rows[0].i16v = INT16_MIN;
        rows[0].i32v = INT32_MIN;
        rows[0].u64v = UINT64_MAX;
        rows[0].f64v = 3.141592653589793;
        rows[0].f32v = 1.5f;
        rows[0].name = arena_.alloc_str("café.exe");
        rows[0].tag = arena_.alloc_str("");
        rows[1].flag = false;
        rows[1].b8 = 0;
        rows[1].i16v = INT16_MAX;
        rows[1].i32v = INT32_MAX;
        rows[1].u64v = 0;
        rows[1].f64v = -2.5;
        rows[1].f32v = -0.25f;
        rows[1].name = arena_.alloc_str("x");
        rows[1].tag = arena_.alloc_str("tag2");
        polyplug::ArrayRef ref = arena_.alloc_array(rows, 2);
        return polyplug_generated::ArrayOf_Kitchen{ref.items, ref.len};
    }
    polyplug_generated::ArrayOf_Kitchen empty() override {
        arena_.reset();
        polyplug::ArrayRef ref = arena_.alloc_array((polyplug_generated::Kitchen*)nullptr, 0);
        return polyplug_generated::ArrayOf_Kitchen{ref.items, ref.len};
    }
private:
    polyplug::ReturnArena arena_;
};
SysKitchenGuestContract* polyplug_create_kitchen(const HostApi*) { return new KitchenImpl(); }
}  // namespace polyplug_plugin

static std::string sv_str(const StringView& s) {
    if (s.ptr == nullptr || s.len == 0) return std::string();
    return std::string(reinterpret_cast<const char*>(s.ptr), s.len);
}

int main() {
    polyplug_plugin::SysKitchenGuestContract* impl =
        polyplug_plugin::polyplug_create_kitchen(nullptr);
    polyplug_generated::ArrayOf_Kitchen arr = impl->make();
    assert(arr.len == 2);
    const polyplug_generated::Kitchen* r = arr.elements();
    assert(r[0].flag == true);
    assert(r[0].b8 == 255);
    assert(r[0].i16v == INT16_MIN);
    assert(r[0].i32v == INT32_MIN);
    assert(r[0].u64v == UINT64_MAX);
    assert(r[0].f64v == 3.141592653589793);
    assert(r[0].f32v == 1.5f);
    assert(sv_str(r[0].name) == "café.exe");
    assert(r[0].tag.len == 0);
    assert(r[1].flag == false);
    assert(r[1].b8 == 0);
    assert(r[1].i16v == INT16_MAX);
    assert(r[1].i32v == INT32_MAX);
    assert(r[1].u64v == 0);
    assert(r[1].f64v == -2.5);
    assert(r[1].f32v == -0.25f);
    assert(sv_str(r[1].name) == "x");
    assert(sv_str(r[1].tag) == "tag2");
    polyplug_generated::ArrayOf_Kitchen e = impl->empty();
    assert(e.len == 0);
    std::printf("OK: Array<Kitchen{all-widths}> round-tripped byte-correct\n");
    delete impl;
    return 0;
}
"##;

#[test]
fn cpp_kitchen_all_widths_round_trips() {
    let tmp: TempDir = tempdir().expect("tempdir");
    let project_dir: PathBuf = tmp.path().join("plugin");
    let gen_dir: PathBuf = project_dir.join("gen");
    fs::create_dir_all(&project_dir).expect("create project dir");

    generate_bundle_with(
        &project_dir,
        &gen_dir,
        "cpp",
        KITCHEN_API_TOML,
        KITCHEN_BUNDLE_TOML,
    );

    let main_cpp: PathBuf = project_dir.join("driver.cpp");
    fs::write(&main_cpp, CPP_KITCHEN_MAIN).expect("write driver.cpp");
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
        "c++ build of the kitchen driver failed (status {:?})\n--- stdout ---\n{}\n--- stderr ---\n{}",
        build.status.code(),
        String::from_utf8_lossy(&build.stdout),
        String::from_utf8_lossy(&build.stderr),
    );

    let run: Output = Command::new(&exe)
        .output()
        .expect("failed to run kitchen driver");
    assert!(
        run.status.success(),
        "C++ Kitchen driver failed (status {:?})\n--- stdout ---\n{}\n--- stderr ---\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr),
    );
    assert!(
        String::from_utf8_lossy(&run.stdout).contains("round-tripped byte-correct"),
        "C++ Kitchen driver must report success, got:\n{}",
        String::from_utf8_lossy(&run.stdout),
    );
}

/// Deno kitchen-sink driver: mocks the QuickJS `bridge` over an ArrayBuffer,
/// returns array-of-objects with boundary values (u64 as `{lo,hi}`), drives
/// `make`/`empty`, and hand-reads each `Kitchen` at its known C offset.
const JS_KITCHEN_DRIVER: &str = r#"import { KITCHEN_INTERFACE, setKitchenFactory } from "./generated/guest/contracts.ts";

const MEM = new ArrayBuffer(1 << 20);
const DV = new DataView(MEM);
let cursor = 4097; // odd → forces element realignment
const bridge = {
  writeU32: (p: number, v: number) => DV.setUint32(p, v >>> 0, true),
  writeI32: (p: number, v: number) => DV.setInt32(p, v | 0, true),
  writeF32: (p: number, v: number) => DV.setFloat32(p, v, true),
  writeF64: (p: number, v: number) => DV.setFloat64(p, v, true),
  writeByte: (p: number, v: number) => DV.setUint8(p, v & 0xff),
  readU32: (p: number) => DV.getUint32(p, true),
  arenaAlloc: (size: number, _arena: number) => {
    const a = cursor;
    cursor += Number(size);
    return [a % 4294967296, Math.floor(a / 4294967296)];
  },
};

setKitchenFactory(((_b: any, _lo: number, _hi: number) => ({
  fn0: () => [
    { flag: true, b8: 255, i16v: -32768, i32v: -2147483648,
      u64v: { lo: 0xffffffff, hi: 0xffffffff }, f64v: 3.141592653589793,
      f32v: 1.5, name: "café.exe", tag: "" },
    { flag: false, b8: 0, i16v: 32767, i32v: 2147483647,
      u64v: { lo: 0, hi: 0 }, f64v: -2.5, f32v: -0.25, name: "x", tag: "tag2" },
  ],
  fn1: () => [],
})) as any);

const impl = (KITCHEN_INTERFACE.factory as any)(bridge, 0, 0);
const OUT = 16;
(KITCHEN_INTERFACE.functions as any)[0](impl, 0, OUT, 0, bridge);
const items = DV.getUint32(OUT, true) + DV.getUint32(OUT + 4, true) * 4294967296;
const len = DV.getUint32(OUT + 8, true);
if (len !== 2) throw new Error("make len must be 2, got " + len);

function readStr(base: number, off: number): string {
  const ptr = DV.getUint32(base + off, true) + DV.getUint32(base + off + 4, true) * 4294967296;
  const nlen = DV.getUint32(base + off + 8, true);
  const bytes = new Uint8Array(nlen);
  for (let i = 0; i < nlen; i++) bytes[i] = DV.getUint8(ptr + i);
  return new TextDecoder().decode(bytes);
}
function readKitchen(base: number) {
  return {
    flag: DV.getUint8(base) !== 0,
    b8: DV.getUint8(base + 1),
    i16v: DV.getInt16(base + 2, true),
    i32v: DV.getInt32(base + 4, true),
    u64lo: DV.getUint32(base + 8, true),
    u64hi: DV.getUint32(base + 12, true),
    f64v: DV.getFloat64(base + 16, true),
    f32v: DV.getFloat32(base + 24, true),
    name: readStr(base, 32),
    tag: readStr(base, 48),
  };
}
const r0 = readKitchen(items);
const r1 = readKitchen(items + 64);
function eq(a: unknown, b: unknown, msg: string) {
  if (a !== b) throw new Error(msg + ": " + JSON.stringify(a) + " !== " + JSON.stringify(b));
}
eq(r0.flag, true, "r0.flag");
eq(r0.b8, 255, "r0.b8");
eq(r0.i16v, -32768, "r0.i16v");
eq(r0.i32v, -2147483648, "r0.i32v");
eq(r0.u64lo, 4294967295, "r0.u64lo");
eq(r0.u64hi, 4294967295, "r0.u64hi");
eq(r0.f64v, 3.141592653589793, "r0.f64v");
eq(r0.f32v, 1.5, "r0.f32v");
eq(r0.name, "café.exe", "r0.name");
eq(r0.tag, "", "r0.tag");
eq(r1.flag, false, "r1.flag");
eq(r1.b8, 0, "r1.b8");
eq(r1.i16v, 32767, "r1.i16v");
eq(r1.i32v, 2147483647, "r1.i32v");
eq(r1.u64lo, 0, "r1.u64lo");
eq(r1.u64hi, 0, "r1.u64hi");
eq(r1.f64v, -2.5, "r1.f64v");
eq(r1.f32v, -0.25, "r1.f32v");
eq(r1.name, "x", "r1.name");
eq(r1.tag, "tag2", "r1.tag");

const OUT2 = 96;
(KITCHEN_INTERFACE.functions as any)[1](impl, 0, OUT2, 0, bridge);
if (DV.getUint32(OUT2 + 8, true) !== 0) throw new Error("empty len must be 0");

console.log("OK: Array<Kitchen{all-widths}> round-tripped byte-correct");
"#;

#[test]
fn js_kitchen_all_widths_round_trips() {
    let tmp: TempDir = tempdir().expect("tempdir");
    let project_dir: PathBuf = tmp.path().join("bundle");
    let gen_dir: PathBuf = project_dir.join("generated");
    fs::create_dir_all(&project_dir).expect("create project dir");

    generate_bundle_with(
        &project_dir,
        &gen_dir,
        "js-quickjs",
        KITCHEN_API_TOML,
        KITCHEN_BUNDLE_TOML,
    );

    let driver_path: PathBuf = project_dir.join("driver.ts");
    fs::write(&driver_path, JS_KITCHEN_DRIVER).expect("write driver.ts");

    let run: Output = Command::new("deno")
        .arg("run")
        .arg("--no-lock")
        .arg(&driver_path)
        .output()
        .expect("failed to spawn deno run");
    assert!(
        run.status.success(),
        "Deno Kitchen driver failed (status {:?})\n--- stdout ---\n{}\n--- stderr ---\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr),
    );
    assert!(
        String::from_utf8_lossy(&run.stdout).contains("round-tripped byte-correct"),
        "Deno Kitchen driver must report success, got:\n{}",
        String::from_utf8_lossy(&run.stdout),
    );
}

/// Python kitchen-sink driver: mocks the loader arena over a ctypes buffer,
/// returns ergonomic objects with boundary values, and reads each `Kitchen`
/// back through the generated ctypes `Kitchen.from_address`.
const PY_KITCHEN_DRIVER: &str = r#"import ctypes
from types import SimpleNamespace
from guest.types import ArrayOf_Kitchen, Kitchen
from guest.contracts import kitchen_make_abi, kitchen_empty_abi

BUF = ctypes.create_string_buffer(1 << 20)
BASE = ctypes.addressof(BUF)
_cursor = [1]  # odd start → forces element realignment


def arena_alloc(size, _arena):
    a = BASE + _cursor[0]
    _cursor[0] += int(size)
    return a


impl = SimpleNamespace(
    make=lambda: [
        SimpleNamespace(flag=True, b8=255, i16v=-32768, i32v=-2147483648,
                        u64v=0xFFFFFFFFFFFFFFFF, f64v=3.141592653589793,
                        f32v=1.5, name="café.exe", tag=""),
        SimpleNamespace(flag=False, b8=0, i16v=32767, i32v=2147483647,
                        u64v=0, f64v=-2.5, f32v=-0.25, name="x", tag="tag2"),
    ],
    empty=lambda: [],
)

out = ArrayOf_Kitchen()
kitchen_make_abi(impl, 0, ctypes.addressof(out), 0, arena_alloc)
assert out.len == 2, "make len=%d" % out.len
esize = ctypes.sizeof(Kitchen)


def read(i):
    k = Kitchen.from_address(out.items + i * esize)
    return SimpleNamespace(
        flag=bool(k.flag), b8=k.b8, i16v=k.i16v, i32v=k.i32v, u64v=k.u64v,
        f64v=k.f64v, f32v=k.f32v,
        name=ctypes.string_at(k.name.ptr, k.name.len).decode(),
        tag_len=int(k.tag.len),
        tag=ctypes.string_at(k.tag.ptr, k.tag.len).decode() if k.tag.len else "",
    )


r0 = read(0)
assert (r0.flag, r0.b8, r0.i16v, r0.i32v, r0.u64v) == (True, 255, -32768, -2147483648, 0xFFFFFFFFFFFFFFFF), r0
assert r0.f64v == 3.141592653589793 and r0.f32v == 1.5, r0
assert r0.name == "café.exe" and r0.tag_len == 0, r0
r1 = read(1)
assert (r1.flag, r1.b8, r1.i16v, r1.i32v, r1.u64v) == (False, 0, 32767, 2147483647, 0), r1
assert r1.f64v == -2.5 and r1.f32v == -0.25, r1
assert r1.name == "x" and r1.tag == "tag2", r1

out2 = ArrayOf_Kitchen()
kitchen_empty_abi(impl, 0, ctypes.addressof(out2), 0, arena_alloc)
assert out2.len == 0, "empty len=%d" % out2.len

print("OK: Array<Kitchen{all-widths}> round-tripped byte-correct")
"#;

#[test]
fn python_kitchen_all_widths_round_trips() {
    let tmp: TempDir = tempdir().expect("tempdir");
    let project_dir: PathBuf = tmp.path().join("bundle");
    let gen_dir: PathBuf = project_dir.join("gen");
    fs::create_dir_all(&project_dir).expect("create project dir");

    generate_bundle_with(
        &project_dir,
        &gen_dir,
        "python",
        KITCHEN_API_TOML,
        KITCHEN_BUNDLE_TOML,
    );

    let driver_path: PathBuf = project_dir.join("driver.py");
    fs::write(&driver_path, PY_KITCHEN_DRIVER).expect("write driver.py");

    let sdk: PathBuf = repo_root().join("sdks").join("python");
    let shim: PathBuf = project_dir.join("shim");
    fs::create_dir_all(shim.join("polyplug").join("abi")).expect("create shim polyplug/abi");
    fs::write(shim.join("polyplug").join("__init__.py"), b"").expect("write polyplug init");
    fs::write(shim.join("polyplug").join("abi").join("__init__.py"), b"")
        .expect("write polyplug.abi init");
    fs::copy(
        sdk.join("abi").join("abi.py"),
        shim.join("polyplug").join("abi").join("abi.py"),
    )
    .expect("copy polyplug/abi/abi.py");

    let pythonpath: String = env::join_paths([
        gen_dir.clone(),
        sdk.join("guest"),
        sdk.join("polyplug_abi"),
        shim,
    ])
    .expect("join PYTHONPATH")
    .to_string_lossy()
    .into_owned();

    let run: Output = Command::new("python3")
        .arg(&driver_path)
        .env("PYTHONPATH", &pythonpath)
        .output()
        .expect("failed to spawn python3");
    assert!(
        run.status.success(),
        "python3 Kitchen driver failed (status {:?})\n--- stdout ---\n{}\n--- stderr ---\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr),
    );
    assert!(
        String::from_utf8_lossy(&run.stdout).contains("round-tripped byte-correct"),
        "python3 Kitchen driver must report success, got:\n{}",
        String::from_utf8_lossy(&run.stdout),
    );
}

/// C# driver for the kitchen-sink contract: implements the generated
/// `ISysKitchenGuestContract` using the guest SDK's `ReturnArena`
/// (`AllocString` + `AllocArray`), calls `Make()`, reads the returned
/// `ArrayOf_Kitchen` back through the raw `items` pointer, and asserts every
/// field byte-for-byte (incl. a unicode `name` and an empty `tag`). Also proves
/// `Empty()` yields len==0. Mirrors `KITCHEN_MAIN_RS` / `PY_KITCHEN_DRIVER`.
const CS_KITCHEN_DRIVER: &str = r####"using System.Text;
using Polyplug.Abi;
using Polyplug.Guest;

static unsafe string Sv(StringView s) =>
    s.Ptr == IntPtr.Zero || s.Len == 0 ? "" : Encoding.UTF8.GetString((byte*)s.Ptr, (int)s.Len);
static void Check(bool cond, string what) { if (!cond) throw new Exception("mismatch: " + what); }

var plugin = new KitchenPlugin();
unsafe
{
    ArrayOf_Kitchen arr = plugin.Make();
    Check(arr.len == 2, "len");
    Kitchen* p = (Kitchen*)(void*)(nuint)arr.items;

    Check(p[0].flag == 1, "row0.flag");
    Check(p[0].b8 == 255, "row0.b8");
    Check(p[0].i16v == short.MinValue, "row0.i16v");
    Check(p[0].i32v == int.MinValue, "row0.i32v");
    Check(p[0].u64v == ulong.MaxValue, "row0.u64v");
    Check(p[0].f64v == 3.141592653589793, "row0.f64v");
    Check(p[0].f32v == 1.5f, "row0.f32v");
    Check(Sv(p[0].name) == "café.exe", "row0.name");
    Check(Sv(p[0].tag) == "", "row0.tag");

    Check(p[1].flag == 0, "row1.flag");
    Check(p[1].b8 == 0, "row1.b8");
    Check(p[1].i16v == short.MaxValue, "row1.i16v");
    Check(p[1].i32v == int.MaxValue, "row1.i32v");
    Check(p[1].u64v == 0, "row1.u64v");
    Check(p[1].f64v == -2.5, "row1.f64v");
    Check(p[1].f32v == -0.25f, "row1.f32v");
    Check(Sv(p[1].name) == "x", "row1.name");
    Check(Sv(p[1].tag) == "tag2", "row1.tag");

    ArrayOf_Kitchen empty = plugin.Empty();
    Check(empty.len == 0, "empty.len");
}
Console.WriteLine("OK: Array<Kitchen{all-widths}> round-tripped byte-correct");

sealed class KitchenPlugin : ISysKitchenGuestContract
{
    private readonly ReturnArena _arena = new(8192);
    public ArrayOf_Kitchen Make()
    {
        _arena.Reset();
        Kitchen[] rows =
        {
            new() { flag = 1, b8 = 255, i16v = short.MinValue, i32v = int.MinValue, u64v = ulong.MaxValue, f64v = 3.141592653589793, f32v = 1.5f, name = _arena.AllocString("café.exe"), tag = _arena.AllocString("") },
            new() { flag = 0, b8 = 0, i16v = short.MaxValue, i32v = int.MaxValue, u64v = 0, f64v = -2.5, f32v = -0.25f, name = _arena.AllocString("x"), tag = _arena.AllocString("tag2") },
        };
        var (items, len) = _arena.AllocArray<Kitchen>(rows);
        return new ArrayOf_Kitchen { items = items, len = len };
    }
    public ArrayOf_Kitchen Empty()
    {
        _arena.Reset();
        var (items, len) = _arena.AllocArray<Kitchen>(ReadOnlySpan<Kitchen>.Empty);
        return new ArrayOf_Kitchen { items = items, len = len };
    }
}
"####;

#[test]
fn csharp_kitchen_all_widths_round_trips() {
    let tmp: TempDir = tempdir().expect("tempdir");
    let project_dir: PathBuf = tmp.path().join("bundle");
    let gen_dir: PathBuf = project_dir.join("gen");
    fs::create_dir_all(&project_dir).expect("create project dir");

    generate_bundle_with(
        &project_dir,
        &gen_dir,
        "csharp",
        KITCHEN_API_TOML,
        KITCHEN_BUNDLE_TOML,
    );

    // The .NET SDK globs every `.cs` under the project directory, so the generated
    // `gen/guest/*.cs` and this driver compile together with no explicit includes.
    fs::write(project_dir.join("Program.cs"), CS_KITCHEN_DRIVER).expect("write Program.cs");

    let abi_csproj: PathBuf = repo_root().join("sdks/csharp/abi/Polyplug.Abi.csproj");
    let guest_csproj: PathBuf = repo_root().join("sdks/csharp/guest/Polyplug.Guest.csproj");
    let csproj: String = format!(
        "<Project Sdk=\"Microsoft.NET.Sdk\">\n  \
         <PropertyGroup>\n    \
         <OutputType>Exe</OutputType>\n    \
         <TargetFramework>net10.0</TargetFramework>\n    \
         <Nullable>enable</Nullable>\n    \
         <AllowUnsafeBlocks>true</AllowUnsafeBlocks>\n    \
         <ImplicitUsings>enable</ImplicitUsings>\n  \
         </PropertyGroup>\n  \
         <ItemGroup>\n    \
         <ProjectReference Include=\"{abi}\" />\n    \
         <ProjectReference Include=\"{guest}\" />\n  \
         </ItemGroup>\n\
         </Project>\n",
        abi = abi_csproj.display(),
        guest = guest_csproj.display(),
    );
    let csproj_path: PathBuf = project_dir.join("kitchen.csproj");
    fs::write(&csproj_path, csproj).expect("write kitchen.csproj");

    let run: Output = Command::new("dotnet")
        .arg("run")
        .arg("-c")
        .arg("Release")
        .arg("--project")
        .arg(&csproj_path)
        .output()
        .expect("failed to spawn dotnet");
    assert!(
        run.status.success(),
        "dotnet Kitchen driver failed (status {:?})\n--- stdout ---\n{}\n--- stderr ---\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr),
    );
    assert!(
        String::from_utf8_lossy(&run.stdout).contains("round-tripped byte-correct"),
        "dotnet Kitchen driver must report success, got:\n{}",
        String::from_utf8_lossy(&run.stdout),
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Non-struct array elements: `Array<u32>` (scalar), `Array<StringView>` (direct
// string), and `Array<Status>` (enum). These are the element kinds the marshaler
// must handle DIRECTLY (not only as struct fields). They were silently broken in
// the VM generators before this suite: the lua array marshaler emitted
// `ffi.sizeof("u32")` / `ffi.sizeof("Status")` (no such cdef'd C type) and copied
// a Lua string straight into a `StringView` cdata; the python marshaler referenced
// the undefined `StringView` / `Status` ctypes names. Fixed in lua.rs
// (`emit_lua_marshal_array_into` resolves the element to its LuaJIT C type;
// `emit_lua_marshal_into` gained a StringView branch) and python.rs
// (`emit_py_marshal_array` uses the enum repr ctype; contracts.py imports
// StringView for a StringView-array return).
//
// The element COUNTS also probe the loop/stride math the struct tests miss:
// `nums` returns 257 elements (large-N, index 256 exercises stride at scale),
// `names` returns exactly 1 (off-by-one), `codes` returns 3.
// ═══════════════════════════════════════════════════════════════════════════

const GAPS_API_TOML: &str = r#"
[[enum]]
name = "Status"
repr = "u32"
[[enum.variants]]
name = "Idle"
value = "0"
[[enum.variants]]
name = "Busy"
value = "7"

[[plugin_contract]]
name = "sys.Gaps"
version = "1.0.0"

[[plugin_contract.functions]]
name = "nums"
return = "Array<u32>"

[[plugin_contract.functions]]
name = "names"
return = "Array<StringView>"

[[plugin_contract.functions]]
name = "codes"
return = "Array<Status>"
"#;

const GAPS_BUNDLE_TOML: &str = r#"
[bundle]
name = "sys_gaps"
version = "1.0.0"
api = "api.toml"
loader = "native"

[bundle.file]
linux.x86_64 = "libgaps.so"

[[plugin]]
name = "gaps"
implements = ["sys.Gaps@1.0"]
"#;

/// Rust driver: builds a 257-element `u32` array, a 1-element `StringView` array,
/// and a 3-element `Status` array into the SDK `ReturnArena`, then reads each back
/// through the generated `as_slice` and asserts values.
const GAPS_MAIN_RS: &str = r##"#[path = "../gen/guest/mod.rs"]
mod generated;

use core::slice;
use core::str;
use std::sync::Mutex;

use generated::contracts::SysGapsGuestContract;
use generated::types::{ArrayOf_Status, ArrayOf_StringView, ArrayOf_u32, Status};
use polyplug_abi::StringView;
use polyplug_guest::{GuestError, HostContext, ReturnArena};

struct Plugin {
    arena: Mutex<ReturnArena>,
}

impl SysGapsGuestContract for Plugin {
    fn nums(&self) -> Result<ArrayOf_u32, GuestError> {
        let mut arena = self.arena.lock().expect("arena lock");
        arena.reset();
        let v: Vec<u32> = (0..257u32).collect();
        let (items, len) = arena.alloc_array(v.as_slice());
        Ok(ArrayOf_u32 { items, len })
    }
    fn names(&self) -> Result<ArrayOf_StringView, GuestError> {
        let mut arena = self.arena.lock().expect("arena lock");
        arena.reset();
        let solo: [StringView; 1] = [arena.alloc_str("solo")];
        let (items, len) = arena.alloc_array(&solo);
        Ok(ArrayOf_StringView { items, len })
    }
    fn codes(&self) -> Result<ArrayOf_Status, GuestError> {
        let mut arena = self.arena.lock().expect("arena lock");
        arena.reset();
        let codes: [Status; 3] = [Status::Busy, Status::Idle, Status::Busy];
        let (items, len) = arena.alloc_array(&codes);
        Ok(ArrayOf_Status { items, len })
    }
}

#[unsafe(no_mangle)]
pub fn polyplug_create_gaps(host: HostContext) -> Box<dyn SysGapsGuestContract> {
    Box::new(Plugin {
        arena: Mutex::new(ReturnArena::new(host, 8192)),
    })
}

fn sv_to_string(s: StringView) -> String {
    if s.ptr.is_null() || s.len == 0 {
        return String::new();
    }
    // SAFETY: `s` came from `alloc_str` (valid UTF-8, still alive; no reset since).
    let bytes: &[u8] = unsafe { slice::from_raw_parts(s.ptr, s.len) };
    str::from_utf8(bytes).expect("utf8").to_owned()
}

fn main() {
    // SAFETY: null host, never dereferenced (8 KiB buffer never overflows here).
    let host: HostContext = unsafe { HostContext::new(core::ptr::null()) };
    let plugin: Box<dyn SysGapsGuestContract> = polyplug_create_gaps(host);

    let n: ArrayOf_u32 = plugin.nums().expect("nums must return Ok");
    // SAFETY: items/len from `alloc_array` on the still-alive arena (no reset since).
    let nums: &[u32] = unsafe { n.as_slice() };
    assert_eq!(n.len, 257, "nums len");
    assert_eq!(nums[0], 0, "nums[0]");
    assert_eq!(nums[128], 128, "nums[128]");
    assert_eq!(nums[256], 256, "nums[256] (large-N stride)");

    let m: ArrayOf_StringView = plugin.names().expect("names must return Ok");
    // SAFETY: as above.
    let names: &[StringView] = unsafe { m.as_slice() };
    assert_eq!(m.len, 1, "names len (N=1)");
    assert_eq!(sv_to_string(names[0]), "solo", "names[0] bytes");

    let c: ArrayOf_Status = plugin.codes().expect("codes must return Ok");
    // SAFETY: as above.
    let codes: &[Status] = unsafe { c.as_slice() };
    assert_eq!(c.len, 3, "codes len");
    assert_eq!(codes[0], Status::Busy, "codes[0]");
    assert_eq!(codes[1], Status::Idle, "codes[1]");
    assert_eq!(codes[2], Status::Busy, "codes[2]");

    println!("OK: Array<u32|StringView|enum> round-tripped byte-correct");
}
"##;

#[test]
fn rust_scalar_string_enum_arrays_round_trip() {
    let tmp: TempDir = tempdir().expect("tempdir");
    let project_dir: PathBuf = tmp.path().join("driver");
    let gen_dir: PathBuf = project_dir.join("gen");
    fs::create_dir_all(project_dir.join("src")).expect("create src dir");

    generate_bundle_with(
        &project_dir,
        &gen_dir,
        "rust",
        GAPS_API_TOML,
        GAPS_BUNDLE_TOML,
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
    fs::write(project_dir.join("src/main.rs"), GAPS_MAIN_RS).expect("write src/main.rs");

    let target_dir: PathBuf = tmp.path().join("target");
    let run: Output = Command::new(env!("CARGO"))
        .arg("run")
        .arg("--manifest-path")
        .arg(project_dir.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", &target_dir)
        .output()
        .expect("failed to spawn cargo run for the gaps driver");
    assert!(
        run.status.success(),
        "gaps driver failed (status {:?})\n--- stdout ---\n{}\n--- stderr ---\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr),
    );
    assert!(
        String::from_utf8_lossy(&run.stdout).contains("round-tripped byte-correct"),
        "driver must report success, got:\n{}",
        String::from_utf8_lossy(&run.stdout),
    );
}

/// C++ driver for the non-struct-element arrays.
const CPP_GAPS_MAIN: &str = r##"#include "guest/init.hpp"
#include <polyplug/guest.hpp>
#include <cassert>
#include <cstdint>
#include <cstdio>
#include <string>

namespace polyplug_plugin {
class GapsImpl : public SysGapsGuestContract {
public:
    GapsImpl() : arena_(8192) {}
    polyplug_generated::ArrayOf_u32 nums() override {
        arena_.reset();
        uint32_t v[257];
        for (uint32_t i = 0; i < 257; i++) v[i] = i;
        polyplug::ArrayRef ref = arena_.alloc_array(v, 257);
        return polyplug_generated::ArrayOf_u32{ref.items, ref.len};
    }
    polyplug_generated::ArrayOf_StringView names() override {
        arena_.reset();
        StringView solo[1];
        solo[0] = arena_.alloc_str("solo");
        polyplug::ArrayRef ref = arena_.alloc_array(solo, 1);
        return polyplug_generated::ArrayOf_StringView{ref.items, ref.len};
    }
    polyplug_generated::ArrayOf_Status codes() override {
        arena_.reset();
        polyplug_generated::Status c[3] = {
            polyplug_generated::Status::Busy,
            polyplug_generated::Status::Idle,
            polyplug_generated::Status::Busy,
        };
        polyplug::ArrayRef ref = arena_.alloc_array(c, 3);
        return polyplug_generated::ArrayOf_Status{ref.items, ref.len};
    }
private:
    polyplug::ReturnArena arena_;
};
SysGapsGuestContract* polyplug_create_gaps(const HostApi*) { return new GapsImpl(); }
}  // namespace polyplug_plugin

static std::string sv_str(const StringView& s) {
    if (s.ptr == nullptr || s.len == 0) return std::string();
    return std::string(reinterpret_cast<const char*>(s.ptr), s.len);
}

int main() {
    polyplug_plugin::SysGapsGuestContract* impl =
        polyplug_plugin::polyplug_create_gaps(nullptr);
    polyplug_generated::ArrayOf_u32 n = impl->nums();
    assert(n.len == 257);
    const uint32_t* nums = n.elements();
    assert(nums[0] == 0 && nums[128] == 128 && nums[256] == 256);
    polyplug_generated::ArrayOf_StringView m = impl->names();
    assert(m.len == 1);
    assert(sv_str(m.elements()[0]) == "solo");
    polyplug_generated::ArrayOf_Status c = impl->codes();
    assert(c.len == 3);
    const polyplug_generated::Status* codes = c.elements();
    assert(codes[0] == polyplug_generated::Status::Busy);
    assert(codes[1] == polyplug_generated::Status::Idle);
    assert(codes[2] == polyplug_generated::Status::Busy);
    std::printf("OK: Array<u32|StringView|enum> round-tripped byte-correct\n");
    delete impl;
    return 0;
}
"##;

#[test]
fn cpp_scalar_string_enum_arrays_round_trip() {
    let tmp: TempDir = tempdir().expect("tempdir");
    let project_dir: PathBuf = tmp.path().join("plugin");
    let gen_dir: PathBuf = project_dir.join("gen");
    fs::create_dir_all(&project_dir).expect("create project dir");

    generate_bundle_with(
        &project_dir,
        &gen_dir,
        "cpp",
        GAPS_API_TOML,
        GAPS_BUNDLE_TOML,
    );

    let main_cpp: PathBuf = project_dir.join("driver.cpp");
    fs::write(&main_cpp, CPP_GAPS_MAIN).expect("write driver.cpp");
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
        "c++ build of the gaps driver failed (status {:?})\n--- stdout ---\n{}\n--- stderr ---\n{}",
        build.status.code(),
        String::from_utf8_lossy(&build.stdout),
        String::from_utf8_lossy(&build.stderr),
    );

    let run: Output = Command::new(&exe)
        .output()
        .expect("failed to run gaps driver");
    assert!(
        run.status.success(),
        "C++ gaps driver failed (status {:?})\n--- stdout ---\n{}\n--- stderr ---\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr),
    );
    assert!(
        String::from_utf8_lossy(&run.stdout).contains("round-tripped byte-correct"),
        "C++ gaps driver must report success, got:\n{}",
        String::from_utf8_lossy(&run.stdout),
    );
}

/// LuaJIT driver: the three functions return ergonomic Lua tables (a 257-int
/// array, a 1-string array, a 3-int enum-value array); the generated glue must
/// marshal each into the (mock align-1) arena with the correct element C type.
const LUA_GAPS_BODY: &str = r#"
local ffi = require("ffi")
require("polyplug_abi")
require("generated.guest.types")
local contracts = require("generated.guest.contracts")

local ARENA = ffi.new("uint8_t[?]", 1048576)
local cursor = 1
local function arena_alloc(size, _arena)
    local addr = ffi.cast("uintptr_t", ARENA) + cursor
    cursor = cursor + tonumber(size)
    return addr
end

contracts.set_gaps_factory(function(_host)
    return {
        nums = function(_self)
            local t = {}
            for i = 0, 256 do t[#t + 1] = i end
            return t
        end,
        names = function(_self) return { "solo" } end,
        codes = function(_self) return { 7, 0, 7 } end,
    }
end)

local regs = polyplug_init(1, 1)
local entry = regs["sys.Gaps"]
assert(entry ~= nil, "sys.Gaps must be registered")
local inst = entry.factory(0)

local out0 = ffi.new("ArrayOf_u32[1]")
entry.functions[0](inst, 0, ffi.cast("uintptr_t", out0), 0, arena_alloc)
assert(tonumber(out0[0].len) == 257, "nums len")
local nums = ffi.cast("uint32_t*", out0[0].items)
assert(nums[0] == 0 and nums[128] == 128 and nums[256] == 256, "nums vals")

local out1 = ffi.new("ArrayOf_StringView[1]")
entry.functions[1](inst, 0, ffi.cast("uintptr_t", out1), 0, arena_alloc)
assert(tonumber(out1[0].len) == 1, "names len")
local names = ffi.cast("StringView*", out1[0].items)
assert(ffi.string(names[0].ptr, names[0].len) == "solo", "names0")

local out2 = ffi.new("ArrayOf_Status[1]")
entry.functions[2](inst, 0, ffi.cast("uintptr_t", out2), 0, arena_alloc)
assert(tonumber(out2[0].len) == 3, "codes len")
local codes = ffi.cast("uint32_t*", out2[0].items)
assert(codes[0] == 7 and codes[1] == 0 and codes[2] == 7, "codes vals")

io.write("OK: Array<u32|StringView|enum> round-tripped byte-correct\n")
"#;

#[test]
fn lua_scalar_string_enum_arrays_round_trip() {
    let tmp: TempDir = tempdir().expect("tempdir");
    let project_dir: PathBuf = tmp.path().join("bundle");
    let gen_dir: PathBuf = project_dir.join("generated");
    fs::create_dir_all(&project_dir).expect("create project dir");

    generate_bundle_with(
        &project_dir,
        &gen_dir,
        "lua",
        GAPS_API_TOML,
        GAPS_BUNDLE_TOML,
    );

    let guest_dir: PathBuf = repo_root().join("sdks").join("lua").join("guest");
    let abi_dir: PathBuf = repo_root().join("sdks").join("lua").join("abi");
    let project_fwd: String = project_dir.to_string_lossy().replace('\\', "/");
    let guest_fwd: String = guest_dir.to_string_lossy().replace('\\', "/");
    let abi_fwd: String = abi_dir.to_string_lossy().replace('\\', "/");
    let driver: String = format!(
        "package.path = \"{project_fwd}/?.lua;{guest_fwd}/?.lua;{abi_fwd}/?.lua;\" .. package.path\n{LUA_GAPS_BODY}"
    );
    let driver_path: PathBuf = project_dir.join("driver.lua");
    fs::write(&driver_path, driver).expect("write driver.lua");

    let run: Output = Command::new("luajit")
        .arg(&driver_path)
        .output()
        .expect("failed to spawn luajit");
    assert!(
        run.status.success(),
        "LuaJIT gaps driver failed (status {:?})\n--- stdout ---\n{}\n--- stderr ---\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr),
    );
    assert!(
        String::from_utf8_lossy(&run.stdout).contains("round-tripped byte-correct"),
        "LuaJIT gaps driver must report success, got:\n{}",
        String::from_utf8_lossy(&run.stdout),
    );
}

/// Deno driver: the three functions return plain JS arrays through the mock
/// bridge; the generated glue must marshal scalar / string / enum-value elements
/// into the (mock align-1) arena.
const JS_GAPS_DRIVER: &str = r#"import { GAPS_INTERFACE, setGapsFactory } from "./generated/guest/contracts.ts";

const MEM = new ArrayBuffer(1 << 20);
const DV = new DataView(MEM);
let cursor = 4097;
const bridge = {
  writeU32: (p: number, v: number) => DV.setUint32(p, v >>> 0, true),
  writeI32: (p: number, v: number) => DV.setInt32(p, v | 0, true),
  writeF32: (p: number, v: number) => DV.setFloat32(p, v, true),
  writeF64: (p: number, v: number) => DV.setFloat64(p, v, true),
  writeByte: (p: number, v: number) => DV.setUint8(p, v & 0xff),
  readU32: (p: number) => DV.getUint32(p, true),
  arenaAlloc: (size: number, _arena: number) => {
    const a = cursor;
    cursor += Number(size);
    return [a % 4294967296, Math.floor(a / 4294967296)];
  },
};

setGapsFactory(((_b: any, _lo: number, _hi: number) => ({
  fn0: () => Array.from({ length: 257 }, (_, i) => i),
  fn1: () => ["solo"],
  fn2: () => [7, 0, 7],
})) as any);

const impl = (GAPS_INTERFACE.factory as any)(bridge, 0, 0);
function call(idx: number, out: number) {
  (GAPS_INTERFACE.functions as any)[idx](impl, 0, out, 0, bridge);
  const items = DV.getUint32(out, true) + DV.getUint32(out + 4, true) * 4294967296;
  const len = DV.getUint32(out + 8, true);
  return { items, len };
}

const n = call(0, 16);
if (n.len !== 257) throw new Error("nums len " + n.len);
if (
  DV.getUint32(n.items, true) !== 0 ||
  DV.getUint32(n.items + 128 * 4, true) !== 128 ||
  DV.getUint32(n.items + 256 * 4, true) !== 256
) throw new Error("nums vals");

const m = call(1, 64);
if (m.len !== 1) throw new Error("names len " + m.len);
const ptr = DV.getUint32(m.items, true) + DV.getUint32(m.items + 4, true) * 4294967296;
const slen = DV.getUint32(m.items + 8, true);
let s = "";
for (let i = 0; i < slen; i++) s += String.fromCharCode(DV.getUint8(ptr + i));
if (s !== "solo") throw new Error("names0 " + s);

const c = call(2, 112);
if (c.len !== 3) throw new Error("codes len " + c.len);
if (
  DV.getUint32(c.items, true) !== 7 ||
  DV.getUint32(c.items + 4, true) !== 0 ||
  DV.getUint32(c.items + 8, true) !== 7
) throw new Error("codes vals");

console.log("OK: Array<u32|StringView|enum> round-tripped byte-correct");
"#;

#[test]
fn js_scalar_string_enum_arrays_round_trip() {
    let tmp: TempDir = tempdir().expect("tempdir");
    let project_dir: PathBuf = tmp.path().join("bundle");
    let gen_dir: PathBuf = project_dir.join("generated");
    fs::create_dir_all(&project_dir).expect("create project dir");

    generate_bundle_with(
        &project_dir,
        &gen_dir,
        "js-quickjs",
        GAPS_API_TOML,
        GAPS_BUNDLE_TOML,
    );

    let driver_path: PathBuf = project_dir.join("driver.ts");
    fs::write(&driver_path, JS_GAPS_DRIVER).expect("write driver.ts");

    let run: Output = Command::new("deno")
        .arg("run")
        .arg("--no-lock")
        .arg(&driver_path)
        .output()
        .expect("failed to spawn deno run");
    assert!(
        run.status.success(),
        "Deno gaps driver failed (status {:?})\n--- stdout ---\n{}\n--- stderr ---\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr),
    );
    assert!(
        String::from_utf8_lossy(&run.stdout).contains("round-tripped byte-correct"),
        "Deno gaps driver must report success, got:\n{}",
        String::from_utf8_lossy(&run.stdout),
    );
}

/// Python driver: the three functions return ergonomic lists; the generated
/// ctypes glue must size/overlay scalar / StringView / enum-repr elements.
const PY_GAPS_DRIVER: &str = r#"import ctypes
from types import SimpleNamespace
from guest.types import ArrayOf_u32, ArrayOf_StringView, ArrayOf_Status
from guest.contracts import gaps_nums_abi, gaps_names_abi, gaps_codes_abi
from polyplug_abi import StringView

BUF = ctypes.create_string_buffer(1 << 20)
BASE = ctypes.addressof(BUF)
_cursor = [1]


def arena_alloc(size, _arena):
    a = BASE + _cursor[0]
    _cursor[0] += int(size)
    return a


impl = SimpleNamespace(
    nums=lambda: list(range(257)),
    names=lambda: ["solo"],
    codes=lambda: [7, 0, 7],
)

out0 = ArrayOf_u32()
gaps_nums_abi(impl, 0, ctypes.addressof(out0), 0, arena_alloc)
assert out0.len == 257, "nums len=%d" % out0.len
nums = (ctypes.c_uint32 * 257).from_address(out0.items)
assert nums[0] == 0 and nums[128] == 128 and nums[256] == 256, "nums vals"

out1 = ArrayOf_StringView()
gaps_names_abi(impl, 0, ctypes.addressof(out1), 0, arena_alloc)
assert out1.len == 1, "names len=%d" % out1.len
s0 = StringView.from_address(out1.items)
assert ctypes.string_at(s0.ptr, s0.len).decode() == "solo", "names0"

out2 = ArrayOf_Status()
gaps_codes_abi(impl, 0, ctypes.addressof(out2), 0, arena_alloc)
assert out2.len == 3, "codes len=%d" % out2.len
codes = (ctypes.c_uint32 * 3).from_address(out2.items)
assert codes[0] == 7 and codes[1] == 0 and codes[2] == 7, "codes vals"

print("OK: Array<u32|StringView|enum> round-tripped byte-correct")
"#;

#[test]
fn python_scalar_string_enum_arrays_round_trip() {
    let tmp: TempDir = tempdir().expect("tempdir");
    let project_dir: PathBuf = tmp.path().join("bundle");
    let gen_dir: PathBuf = project_dir.join("gen");
    fs::create_dir_all(&project_dir).expect("create project dir");

    generate_bundle_with(
        &project_dir,
        &gen_dir,
        "python",
        GAPS_API_TOML,
        GAPS_BUNDLE_TOML,
    );

    let driver_path: PathBuf = project_dir.join("driver.py");
    fs::write(&driver_path, PY_GAPS_DRIVER).expect("write driver.py");

    let sdk: PathBuf = repo_root().join("sdks").join("python");
    let shim: PathBuf = project_dir.join("shim");
    fs::create_dir_all(shim.join("polyplug").join("abi")).expect("create shim polyplug/abi");
    fs::write(shim.join("polyplug").join("__init__.py"), b"").expect("write polyplug init");
    fs::write(shim.join("polyplug").join("abi").join("__init__.py"), b"")
        .expect("write polyplug.abi init");
    fs::copy(
        sdk.join("abi").join("abi.py"),
        shim.join("polyplug").join("abi").join("abi.py"),
    )
    .expect("copy polyplug/abi/abi.py");

    let pythonpath: String = env::join_paths([
        gen_dir.clone(),
        sdk.join("guest"),
        sdk.join("polyplug_abi"),
        shim,
    ])
    .expect("join PYTHONPATH")
    .to_string_lossy()
    .into_owned();

    let run: Output = Command::new("python3")
        .arg(&driver_path)
        .env("PYTHONPATH", &pythonpath)
        .output()
        .expect("failed to spawn python3");
    assert!(
        run.status.success(),
        "python3 gaps driver failed (status {:?})\n--- stdout ---\n{}\n--- stderr ---\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr),
    );
    assert!(
        String::from_utf8_lossy(&run.stdout).contains("round-tripped byte-correct"),
        "python3 gaps driver must report success, got:\n{}",
        String::from_utf8_lossy(&run.stdout),
    );
}

/// C# driver: implements `ISysGapsGuestContract` with the guest SDK `ReturnArena`
/// (`AllocArray<uint>` / `AllocString`+`AllocArray<StringView>` /
/// `AllocArray<Status>`), reads each returned wrapper back through the raw `items`
/// pointer, and asserts values.
const CS_GAPS_DRIVER: &str = r####"using System.Text;
using Polyplug.Abi;
using Polyplug.Guest;

static unsafe string Sv(StringView s) =>
    s.Ptr == IntPtr.Zero || s.Len == 0 ? "" : Encoding.UTF8.GetString((byte*)s.Ptr, (int)s.Len);
static void Check(bool cond, string what) { if (!cond) throw new Exception("mismatch: " + what); }

var plugin = new GapsPlugin();
unsafe
{
    ArrayOf_u32 n = plugin.Nums();
    Check(n.len == 257, "nums.len");
    uint* np = (uint*)(void*)(nuint)n.items;
    Check(np[0] == 0 && np[128] == 128 && np[256] == 256, "nums vals");

    ArrayOf_StringView m = plugin.Names();
    Check(m.len == 1, "names.len");
    StringView* mp = (StringView*)(void*)(nuint)m.items;
    Check(Sv(mp[0]) == "solo", "names[0]");

    ArrayOf_Status c = plugin.Codes();
    Check(c.len == 3, "codes.len");
    Status* cp = (Status*)(void*)(nuint)c.items;
    Check(cp[0] == Status.Busy && cp[1] == Status.Idle && cp[2] == Status.Busy, "codes vals");
}
Console.WriteLine("OK: Array<u32|StringView|enum> round-tripped byte-correct");

sealed class GapsPlugin : ISysGapsGuestContract
{
    private readonly ReturnArena _arena = new(8192);
    public ArrayOf_u32 Nums()
    {
        _arena.Reset();
        uint[] v = new uint[257];
        for (int i = 0; i < 257; i++) v[i] = (uint)i;
        var (items, len) = _arena.AllocArray<uint>(v);
        return new ArrayOf_u32 { items = items, len = len };
    }
    public ArrayOf_StringView Names()
    {
        _arena.Reset();
        StringView[] solo = { _arena.AllocString("solo") };
        var (items, len) = _arena.AllocArray<StringView>(solo);
        return new ArrayOf_StringView { items = items, len = len };
    }
    public ArrayOf_Status Codes()
    {
        _arena.Reset();
        Status[] codes = { Status.Busy, Status.Idle, Status.Busy };
        var (items, len) = _arena.AllocArray<Status>(codes);
        return new ArrayOf_Status { items = items, len = len };
    }
}
"####;

#[test]
fn csharp_scalar_string_enum_arrays_round_trip() {
    let tmp: TempDir = tempdir().expect("tempdir");
    let project_dir: PathBuf = tmp.path().join("bundle");
    let gen_dir: PathBuf = project_dir.join("gen");
    fs::create_dir_all(&project_dir).expect("create project dir");

    generate_bundle_with(
        &project_dir,
        &gen_dir,
        "csharp",
        GAPS_API_TOML,
        GAPS_BUNDLE_TOML,
    );

    fs::write(project_dir.join("Program.cs"), CS_GAPS_DRIVER).expect("write Program.cs");

    let abi_csproj: PathBuf = repo_root().join("sdks/csharp/abi/Polyplug.Abi.csproj");
    let guest_csproj: PathBuf = repo_root().join("sdks/csharp/guest/Polyplug.Guest.csproj");
    let csproj: String = format!(
        "<Project Sdk=\"Microsoft.NET.Sdk\">\n  \
         <PropertyGroup>\n    \
         <OutputType>Exe</OutputType>\n    \
         <TargetFramework>net10.0</TargetFramework>\n    \
         <Nullable>enable</Nullable>\n    \
         <AllowUnsafeBlocks>true</AllowUnsafeBlocks>\n    \
         <ImplicitUsings>enable</ImplicitUsings>\n  \
         </PropertyGroup>\n  \
         <ItemGroup>\n    \
         <ProjectReference Include=\"{abi}\" />\n    \
         <ProjectReference Include=\"{guest}\" />\n  \
         </ItemGroup>\n\
         </Project>\n",
        abi = abi_csproj.display(),
        guest = guest_csproj.display(),
    );
    let csproj_path: PathBuf = project_dir.join("gaps.csproj");
    fs::write(&csproj_path, csproj).expect("write gaps.csproj");

    let run: Output = Command::new("dotnet")
        .arg("run")
        .arg("-c")
        .arg("Release")
        .arg("--project")
        .arg(&csproj_path)
        .output()
        .expect("failed to spawn dotnet");
    assert!(
        run.status.success(),
        "dotnet gaps driver failed (status {:?})\n--- stdout ---\n{}\n--- stderr ---\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr),
    );
    assert!(
        String::from_utf8_lossy(&run.stdout).contains("round-tripped byte-correct"),
        "dotnet gaps driver must report success, got:\n{}",
        String::from_utf8_lossy(&run.stdout),
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Struct with an `Array<T>` FIELD, returned as `Array<Group>`. Two things are
// under test: (1) type-emission ORDERING — the synthesized `ArrayOf_StringView`
// wrapper must be declared before `Group`, which embeds it, or the C-family type
// headers (Lua `ffi.cdef`, C++, Python `ctypes`) fail to load/compile (fixed by
// the topological sort in parser.rs); (2) the nested marshaling recursion —
// array-of-struct-that-contains-an-array-of-string, incl. an EMPTY nested array.
// ═══════════════════════════════════════════════════════════════════════════

const GROUPS_API_TOML: &str = r#"
[[types]]
name = "Group"
fields = [
    { name = "id",   type = "u32" },
    { name = "tags", type = "Array<StringView>" },
]

[[plugin_contract]]
name = "sys.Groups"
version = "1.0.0"

[[plugin_contract.functions]]
name = "groups"
return = "Array<Group>"
"#;

const GROUPS_BUNDLE_TOML: &str = r#"
[bundle]
name = "sys_groups"
version = "1.0.0"
api = "api.toml"
loader = "native"

[bundle.file]
linux.x86_64 = "libgroups.so"

[[plugin]]
name = "groups"
implements = ["sys.Groups@1.0"]
"#;

/// Rust driver: builds each group's nested `tags` array into the arena first,
/// then the `Group` array, then reads it all back through `as_slice`.
const GROUPS_MAIN_RS: &str = r##"#[path = "../gen/guest/mod.rs"]
mod generated;

use core::slice;
use core::str;
use std::sync::Mutex;

use generated::contracts::SysGroupsGuestContract;
use generated::types::{ArrayOf_Group, ArrayOf_StringView, Group};
use polyplug_abi::StringView;
use polyplug_guest::{GuestError, HostContext, ReturnArena};

struct Plugin {
    arena: Mutex<ReturnArena>,
}

impl SysGroupsGuestContract for Plugin {
    fn groups(&self) -> Result<ArrayOf_Group, GuestError> {
        let mut arena = self.arena.lock().expect("arena lock");
        arena.reset();
        let g0_tags: [StringView; 2] = [arena.alloc_str("alpha"), arena.alloc_str("beta")];
        let (t0_items, t0_len) = arena.alloc_array(&g0_tags);
        let empty: [StringView; 0] = [];
        let (t1_items, t1_len) = arena.alloc_array(&empty);
        let groups: [Group; 2] = [
            Group {
                id: 10,
                tags: ArrayOf_StringView { items: t0_items, len: t0_len },
            },
            Group {
                id: 20,
                tags: ArrayOf_StringView { items: t1_items, len: t1_len },
            },
        ];
        let (items, len) = arena.alloc_array(&groups);
        Ok(ArrayOf_Group { items, len })
    }
}

#[unsafe(no_mangle)]
pub fn polyplug_create_groups(host: HostContext) -> Box<dyn SysGroupsGuestContract> {
    Box::new(Plugin {
        arena: Mutex::new(ReturnArena::new(host, 8192)),
    })
}

fn sv_to_string(s: StringView) -> String {
    if s.ptr.is_null() || s.len == 0 {
        return String::new();
    }
    // SAFETY: `s` came from `alloc_str` (valid UTF-8, still alive; no reset since).
    let bytes: &[u8] = unsafe { slice::from_raw_parts(s.ptr, s.len) };
    str::from_utf8(bytes).expect("utf8").to_owned()
}

fn main() {
    // SAFETY: null host, never dereferenced (8 KiB buffer never overflows here).
    let host: HostContext = unsafe { HostContext::new(core::ptr::null()) };
    let plugin: Box<dyn SysGroupsGuestContract> = polyplug_create_groups(host);

    let w: ArrayOf_Group = plugin.groups().expect("groups must return Ok");
    // SAFETY: items/len from `alloc_array` on the still-alive arena (no reset since).
    let gs: &[Group] = unsafe { w.as_slice() };
    assert_eq!(w.len, 2, "groups len");
    assert_eq!(gs[0].id, 10, "g0.id");
    assert_eq!(gs[0].tags.len, 2, "g0.tags len");
    // SAFETY: nested tags array lives in the same arena, still valid.
    let t0: &[StringView] = unsafe { gs[0].tags.as_slice() };
    assert_eq!(sv_to_string(t0[0]), "alpha", "g0.tags[0]");
    assert_eq!(sv_to_string(t0[1]), "beta", "g0.tags[1]");
    assert_eq!(gs[1].id, 20, "g1.id");
    assert_eq!(gs[1].tags.len, 0, "g1.tags empty");

    println!("OK: Array<Group{{Array<StringView>}}> round-tripped byte-correct");
}
"##;

#[test]
fn rust_struct_with_array_field_round_trips() {
    let tmp: TempDir = tempdir().expect("tempdir");
    let project_dir: PathBuf = tmp.path().join("driver");
    let gen_dir: PathBuf = project_dir.join("gen");
    fs::create_dir_all(project_dir.join("src")).expect("create src dir");

    generate_bundle_with(
        &project_dir,
        &gen_dir,
        "rust",
        GROUPS_API_TOML,
        GROUPS_BUNDLE_TOML,
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
    fs::write(project_dir.join("src/main.rs"), GROUPS_MAIN_RS).expect("write src/main.rs");

    let target_dir: PathBuf = tmp.path().join("target");
    let run: Output = Command::new(env!("CARGO"))
        .arg("run")
        .arg("--manifest-path")
        .arg(project_dir.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", &target_dir)
        .output()
        .expect("failed to spawn cargo run for the groups driver");
    assert!(
        run.status.success(),
        "groups driver failed (status {:?})\n--- stdout ---\n{}\n--- stderr ---\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr),
    );
    assert!(
        String::from_utf8_lossy(&run.stdout).contains("round-tripped byte-correct"),
        "driver must report success, got:\n{}",
        String::from_utf8_lossy(&run.stdout),
    );
}

/// C++ driver for struct-with-array-field.
const CPP_GROUPS_MAIN: &str = r##"#include "guest/init.hpp"
#include <polyplug/guest.hpp>
#include <cassert>
#include <cstdint>
#include <cstdio>
#include <string>

namespace polyplug_plugin {
class GroupsImpl : public SysGroupsGuestContract {
public:
    GroupsImpl() : arena_(8192) {}
    polyplug_generated::ArrayOf_Group groups() override {
        arena_.reset();
        StringView t0[2];
        t0[0] = arena_.alloc_str("alpha");
        t0[1] = arena_.alloc_str("beta");
        polyplug::ArrayRef r0 = arena_.alloc_array(t0, 2);
        polyplug::ArrayRef r1 = arena_.alloc_array((StringView*)nullptr, 0);
        polyplug_generated::Group gs[2];
        gs[0].id = 10;
        gs[0].tags = polyplug_generated::ArrayOf_StringView{r0.items, r0.len};
        gs[1].id = 20;
        gs[1].tags = polyplug_generated::ArrayOf_StringView{r1.items, r1.len};
        polyplug::ArrayRef ref = arena_.alloc_array(gs, 2);
        return polyplug_generated::ArrayOf_Group{ref.items, ref.len};
    }
private:
    polyplug::ReturnArena arena_;
};
SysGroupsGuestContract* polyplug_create_groups(const HostApi*) { return new GroupsImpl(); }
}  // namespace polyplug_plugin

static std::string sv_str(const StringView& s) {
    if (s.ptr == nullptr || s.len == 0) return std::string();
    return std::string(reinterpret_cast<const char*>(s.ptr), s.len);
}

int main() {
    polyplug_plugin::SysGroupsGuestContract* impl =
        polyplug_plugin::polyplug_create_groups(nullptr);
    polyplug_generated::ArrayOf_Group w = impl->groups();
    assert(w.len == 2);
    const polyplug_generated::Group* gs = w.elements();
    assert(gs[0].id == 10);
    assert(gs[0].tags.len == 2);
    const StringView* t0 = gs[0].tags.elements();
    assert(sv_str(t0[0]) == "alpha");
    assert(sv_str(t0[1]) == "beta");
    assert(gs[1].id == 20);
    assert(gs[1].tags.len == 0);
    std::printf("OK: Array<Group{Array<StringView>}> round-tripped byte-correct\n");
    delete impl;
    return 0;
}
"##;

#[test]
fn cpp_struct_with_array_field_round_trips() {
    let tmp: TempDir = tempdir().expect("tempdir");
    let project_dir: PathBuf = tmp.path().join("plugin");
    let gen_dir: PathBuf = project_dir.join("gen");
    fs::create_dir_all(&project_dir).expect("create project dir");

    generate_bundle_with(
        &project_dir,
        &gen_dir,
        "cpp",
        GROUPS_API_TOML,
        GROUPS_BUNDLE_TOML,
    );

    let main_cpp: PathBuf = project_dir.join("driver.cpp");
    fs::write(&main_cpp, CPP_GROUPS_MAIN).expect("write driver.cpp");
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
        "c++ build of the groups driver failed (status {:?})\n--- stdout ---\n{}\n--- stderr ---\n{}",
        build.status.code(),
        String::from_utf8_lossy(&build.stdout),
        String::from_utf8_lossy(&build.stderr),
    );

    let run: Output = Command::new(&exe)
        .output()
        .expect("failed to run groups driver");
    assert!(
        run.status.success(),
        "C++ groups driver failed (status {:?})\n--- stdout ---\n{}\n--- stderr ---\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr),
    );
    assert!(
        String::from_utf8_lossy(&run.stdout).contains("round-tripped byte-correct"),
        "C++ groups driver must report success, got:\n{}",
        String::from_utf8_lossy(&run.stdout),
    );
}

/// LuaJIT driver: the impl returns ergonomic nested tables; the generated glue
/// recurses (array → struct field → string array), and the type cdef must load
/// (ordering) before any of it runs.
const LUA_GROUPS_BODY: &str = r#"
local ffi = require("ffi")
require("polyplug_abi")
require("generated.guest.types")
local contracts = require("generated.guest.contracts")

local ARENA = ffi.new("uint8_t[?]", 1048576)
local cursor = 1
local function arena_alloc(size, _arena)
    local addr = ffi.cast("uintptr_t", ARENA) + cursor
    cursor = cursor + tonumber(size)
    return addr
end

contracts.set_groups_factory(function(_host)
    return {
        groups = function(_self)
            return {
                { id = 10, tags = { "alpha", "beta" } },
                { id = 20, tags = {} },
            }
        end,
    }
end)

local regs = polyplug_init(1, 1)
local entry = regs["sys.Groups"]
assert(entry ~= nil, "sys.Groups must be registered")
local inst = entry.factory(0)

local out = ffi.new("ArrayOf_Group[1]")
entry.functions[0](inst, 0, ffi.cast("uintptr_t", out), 0, arena_alloc)
assert(tonumber(out[0].len) == 2, "groups len")
local gs = ffi.cast("Group*", out[0].items)
assert(gs[0].id == 10, "g0.id")
assert(tonumber(gs[0].tags.len) == 2, "g0.tags len")
local t0 = ffi.cast("StringView*", gs[0].tags.items)
assert(ffi.string(t0[0].ptr, t0[0].len) == "alpha", "g0.tags0")
assert(ffi.string(t0[1].ptr, t0[1].len) == "beta", "g0.tags1")
assert(gs[1].id == 20, "g1.id")
assert(tonumber(gs[1].tags.len) == 0, "g1.tags empty")

io.write("OK: Array<Group{Array<StringView>}> round-tripped byte-correct\n")
"#;

#[test]
fn lua_struct_with_array_field_round_trips() {
    let tmp: TempDir = tempdir().expect("tempdir");
    let project_dir: PathBuf = tmp.path().join("bundle");
    let gen_dir: PathBuf = project_dir.join("generated");
    fs::create_dir_all(&project_dir).expect("create project dir");

    generate_bundle_with(
        &project_dir,
        &gen_dir,
        "lua",
        GROUPS_API_TOML,
        GROUPS_BUNDLE_TOML,
    );

    let guest_dir: PathBuf = repo_root().join("sdks").join("lua").join("guest");
    let abi_dir: PathBuf = repo_root().join("sdks").join("lua").join("abi");
    let project_fwd: String = project_dir.to_string_lossy().replace('\\', "/");
    let guest_fwd: String = guest_dir.to_string_lossy().replace('\\', "/");
    let abi_fwd: String = abi_dir.to_string_lossy().replace('\\', "/");
    let driver: String = format!(
        "package.path = \"{project_fwd}/?.lua;{guest_fwd}/?.lua;{abi_fwd}/?.lua;\" .. package.path\n{LUA_GROUPS_BODY}"
    );
    let driver_path: PathBuf = project_dir.join("driver.lua");
    fs::write(&driver_path, driver).expect("write driver.lua");

    let run: Output = Command::new("luajit")
        .arg(&driver_path)
        .output()
        .expect("failed to spawn luajit");
    assert!(
        run.status.success(),
        "LuaJIT groups driver failed (status {:?})\n--- stdout ---\n{}\n--- stderr ---\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr),
    );
    assert!(
        String::from_utf8_lossy(&run.stdout).contains("round-tripped byte-correct"),
        "LuaJIT groups driver must report success, got:\n{}",
        String::from_utf8_lossy(&run.stdout),
    );
}

/// Deno driver: nested JS objects through the mock bridge; reads `Group` at its C
/// offsets (id@0, tags.items@8, tags.len@16; sizeof 24) and each tag StringView.
const JS_GROUPS_DRIVER: &str = r#"import { GROUPS_INTERFACE, setGroupsFactory } from "./generated/guest/contracts.ts";

const MEM = new ArrayBuffer(1 << 20);
const DV = new DataView(MEM);
let cursor = 4097;
const bridge = {
  writeU32: (p: number, v: number) => DV.setUint32(p, v >>> 0, true),
  writeI32: (p: number, v: number) => DV.setInt32(p, v | 0, true),
  writeF32: (p: number, v: number) => DV.setFloat32(p, v, true),
  writeF64: (p: number, v: number) => DV.setFloat64(p, v, true),
  writeByte: (p: number, v: number) => DV.setUint8(p, v & 0xff),
  readU32: (p: number) => DV.getUint32(p, true),
  arenaAlloc: (size: number, _arena: number) => {
    const a = cursor;
    cursor += Number(size);
    return [a % 4294967296, Math.floor(a / 4294967296)];
  },
};

setGroupsFactory(((_b: any, _lo: number, _hi: number) => ({
  fn0: () => [
    { id: 10, tags: ["alpha", "beta"] },
    { id: 20, tags: [] },
  ],
})) as any);

const impl = (GROUPS_INTERFACE.factory as any)(bridge, 0, 0);
const OUT = 16;
(GROUPS_INTERFACE.functions as any)[0](impl, 0, OUT, 0, bridge);
const items = DV.getUint32(OUT, true) + DV.getUint32(OUT + 4, true) * 4294967296;
const len = DV.getUint32(OUT + 8, true);
if (len !== 2) throw new Error("groups len " + len);

function u64(p: number): number {
  return DV.getUint32(p, true) + DV.getUint32(p + 4, true) * 4294967296;
}
function tag(base: number, i: number): string {
  const svp = base + i * 16;
  const ptr = u64(svp);
  const slen = DV.getUint32(svp + 8, true);
  let s = "";
  for (let j = 0; j < slen; j++) s += String.fromCharCode(DV.getUint8(ptr + j));
  return s;
}

const g0 = items; // sizeof(Group) == 24
if (DV.getUint32(g0, true) !== 10) throw new Error("g0.id");
const g0tItems = u64(g0 + 8);
const g0tLen = DV.getUint32(g0 + 16, true);
if (g0tLen !== 2) throw new Error("g0.tags len " + g0tLen);
if (tag(g0tItems, 0) !== "alpha" || tag(g0tItems, 1) !== "beta") throw new Error("g0.tags vals");

const g1 = items + 24;
if (DV.getUint32(g1, true) !== 20) throw new Error("g1.id");
if (DV.getUint32(g1 + 16, true) !== 0) throw new Error("g1.tags not empty");

console.log("OK: Array<Group{Array<StringView>}> round-tripped byte-correct");
"#;

#[test]
fn js_struct_with_array_field_round_trips() {
    let tmp: TempDir = tempdir().expect("tempdir");
    let project_dir: PathBuf = tmp.path().join("bundle");
    let gen_dir: PathBuf = project_dir.join("generated");
    fs::create_dir_all(&project_dir).expect("create project dir");

    generate_bundle_with(
        &project_dir,
        &gen_dir,
        "js-quickjs",
        GROUPS_API_TOML,
        GROUPS_BUNDLE_TOML,
    );

    let driver_path: PathBuf = project_dir.join("driver.ts");
    fs::write(&driver_path, JS_GROUPS_DRIVER).expect("write driver.ts");

    let run: Output = Command::new("deno")
        .arg("run")
        .arg("--no-lock")
        .arg(&driver_path)
        .output()
        .expect("failed to spawn deno run");
    assert!(
        run.status.success(),
        "Deno groups driver failed (status {:?})\n--- stdout ---\n{}\n--- stderr ---\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr),
    );
    assert!(
        String::from_utf8_lossy(&run.stdout).contains("round-tripped byte-correct"),
        "Deno groups driver must report success, got:\n{}",
        String::from_utf8_lossy(&run.stdout),
    );
}

/// Python driver: nested ergonomic objects; the generated ctypes glue recurses
/// and the type module must import cleanly (ordering) first.
const PY_GROUPS_DRIVER: &str = r#"import ctypes
from types import SimpleNamespace
from guest.types import ArrayOf_Group, Group
from guest.contracts import groups_groups_abi
from polyplug_abi import StringView

BUF = ctypes.create_string_buffer(1 << 20)
BASE = ctypes.addressof(BUF)
_cursor = [1]


def arena_alloc(size, _arena):
    a = BASE + _cursor[0]
    _cursor[0] += int(size)
    return a


impl = SimpleNamespace(groups=lambda: [
    SimpleNamespace(id=10, tags=["alpha", "beta"]),
    SimpleNamespace(id=20, tags=[]),
])

out = ArrayOf_Group()
groups_groups_abi(impl, 0, ctypes.addressof(out), 0, arena_alloc)
assert out.len == 2, "groups len=%d" % out.len
gsize = ctypes.sizeof(Group)
svsize = ctypes.sizeof(StringView)


def group(i):
    return Group.from_address(out.items + i * gsize)


def tag(items, i):
    s = StringView.from_address(items + i * svsize)
    return ctypes.string_at(s.ptr, s.len).decode()


g0 = group(0)
assert g0.id == 10, "g0.id"
assert g0.tags.len == 2, "g0.tags len"
assert tag(g0.tags.items, 0) == "alpha" and tag(g0.tags.items, 1) == "beta", "g0.tags"
g1 = group(1)
assert g1.id == 20 and g1.tags.len == 0, "g1"

print("OK: Array<Group{Array<StringView>}> round-tripped byte-correct")
"#;

#[test]
fn python_struct_with_array_field_round_trips() {
    let tmp: TempDir = tempdir().expect("tempdir");
    let project_dir: PathBuf = tmp.path().join("bundle");
    let gen_dir: PathBuf = project_dir.join("gen");
    fs::create_dir_all(&project_dir).expect("create project dir");

    generate_bundle_with(
        &project_dir,
        &gen_dir,
        "python",
        GROUPS_API_TOML,
        GROUPS_BUNDLE_TOML,
    );

    let driver_path: PathBuf = project_dir.join("driver.py");
    fs::write(&driver_path, PY_GROUPS_DRIVER).expect("write driver.py");

    let sdk: PathBuf = repo_root().join("sdks").join("python");
    let shim: PathBuf = project_dir.join("shim");
    fs::create_dir_all(shim.join("polyplug").join("abi")).expect("create shim polyplug/abi");
    fs::write(shim.join("polyplug").join("__init__.py"), b"").expect("write polyplug init");
    fs::write(shim.join("polyplug").join("abi").join("__init__.py"), b"")
        .expect("write polyplug.abi init");
    fs::copy(
        sdk.join("abi").join("abi.py"),
        shim.join("polyplug").join("abi").join("abi.py"),
    )
    .expect("copy polyplug/abi/abi.py");

    let pythonpath: String = env::join_paths([
        gen_dir.clone(),
        sdk.join("guest"),
        sdk.join("polyplug_abi"),
        shim,
    ])
    .expect("join PYTHONPATH")
    .to_string_lossy()
    .into_owned();

    let run: Output = Command::new("python3")
        .arg(&driver_path)
        .env("PYTHONPATH", &pythonpath)
        .output()
        .expect("failed to spawn python3");
    assert!(
        run.status.success(),
        "python3 groups driver failed (status {:?})\n--- stdout ---\n{}\n--- stderr ---\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr),
    );
    assert!(
        String::from_utf8_lossy(&run.stdout).contains("round-tripped byte-correct"),
        "python3 groups driver must report success, got:\n{}",
        String::from_utf8_lossy(&run.stdout),
    );
}

/// C# driver: builds nested arrays with `ReturnArena`, reads back through raw
/// pointers. Also proves the generated `ArrayOf_StringView`/`Group` structs order
/// correctly (C# is order-independent, but the round trip still exercises the
/// nested wrapper layout).
const CS_GROUPS_DRIVER: &str = r####"using System.Text;
using Polyplug.Abi;
using Polyplug.Guest;

static unsafe string Sv(StringView s) =>
    s.Ptr == IntPtr.Zero || s.Len == 0 ? "" : Encoding.UTF8.GetString((byte*)s.Ptr, (int)s.Len);
static void Check(bool cond, string what) { if (!cond) throw new Exception("mismatch: " + what); }

var plugin = new GroupsPlugin();
unsafe
{
    ArrayOf_Group w = plugin.Groups();
    Check(w.len == 2, "groups.len");
    Group* gs = (Group*)(void*)(nuint)w.items;
    Check(gs[0].id == 10, "g0.id");
    Check(gs[0].tags.len == 2, "g0.tags.len");
    StringView* t0 = (StringView*)(void*)(nuint)gs[0].tags.items;
    Check(Sv(t0[0]) == "alpha" && Sv(t0[1]) == "beta", "g0.tags");
    Check(gs[1].id == 20 && gs[1].tags.len == 0, "g1");
}
Console.WriteLine("OK: Array<Group{Array<StringView>}> round-tripped byte-correct");

sealed class GroupsPlugin : ISysGroupsGuestContract
{
    private readonly ReturnArena _arena = new(8192);
    public ArrayOf_Group Groups()
    {
        _arena.Reset();
        StringView[] t0 = { _arena.AllocString("alpha"), _arena.AllocString("beta") };
        var (t0Items, t0Len) = _arena.AllocArray<StringView>(t0);
        var (t1Items, t1Len) = _arena.AllocArray<StringView>(ReadOnlySpan<StringView>.Empty);
        Group[] gs =
        {
            new() { id = 10, tags = new ArrayOf_StringView { items = t0Items, len = t0Len } },
            new() { id = 20, tags = new ArrayOf_StringView { items = t1Items, len = t1Len } },
        };
        var (items, len) = _arena.AllocArray<Group>(gs);
        return new ArrayOf_Group { items = items, len = len };
    }
}
"####;

#[test]
fn csharp_struct_with_array_field_round_trips() {
    let tmp: TempDir = tempdir().expect("tempdir");
    let project_dir: PathBuf = tmp.path().join("bundle");
    let gen_dir: PathBuf = project_dir.join("gen");
    fs::create_dir_all(&project_dir).expect("create project dir");

    generate_bundle_with(
        &project_dir,
        &gen_dir,
        "csharp",
        GROUPS_API_TOML,
        GROUPS_BUNDLE_TOML,
    );

    fs::write(project_dir.join("Program.cs"), CS_GROUPS_DRIVER).expect("write Program.cs");

    let abi_csproj: PathBuf = repo_root().join("sdks/csharp/abi/Polyplug.Abi.csproj");
    let guest_csproj: PathBuf = repo_root().join("sdks/csharp/guest/Polyplug.Guest.csproj");
    let csproj: String = format!(
        "<Project Sdk=\"Microsoft.NET.Sdk\">\n  \
         <PropertyGroup>\n    \
         <OutputType>Exe</OutputType>\n    \
         <TargetFramework>net10.0</TargetFramework>\n    \
         <Nullable>enable</Nullable>\n    \
         <AllowUnsafeBlocks>true</AllowUnsafeBlocks>\n    \
         <ImplicitUsings>enable</ImplicitUsings>\n  \
         </PropertyGroup>\n  \
         <ItemGroup>\n    \
         <ProjectReference Include=\"{abi}\" />\n    \
         <ProjectReference Include=\"{guest}\" />\n  \
         </ItemGroup>\n\
         </Project>\n",
        abi = abi_csproj.display(),
        guest = guest_csproj.display(),
    );
    let csproj_path: PathBuf = project_dir.join("groups.csproj");
    fs::write(&csproj_path, csproj).expect("write groups.csproj");

    let run: Output = Command::new("dotnet")
        .arg("run")
        .arg("-c")
        .arg("Release")
        .arg("--project")
        .arg(&csproj_path)
        .output()
        .expect("failed to spawn dotnet");
    assert!(
        run.status.success(),
        "dotnet groups driver failed (status {:?})\n--- stdout ---\n{}\n--- stderr ---\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr),
    );
    assert!(
        String::from_utf8_lossy(&run.stdout).contains("round-tripped byte-correct"),
        "dotnet groups driver must report success, got:\n{}",
        String::from_utf8_lossy(&run.stdout),
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Struct with an ENUM field, returned as `Array<Rec>`. An enum has no ctypes
// class (generated enums are `enum.IntEnum`) and no cdef'd C type, so the type
// declaration must use the enum's repr integer for the field. Python got this
// wrong: `class Rec(ctypes.Structure)` emitted `("flag", Status)`, which raised
// `TypeError: this type has no size` the first time `Rec` was instantiated —
// fixed in `generate_python_user_type`. lua (`uint32_t`) and cpp (`enum class`)
// were already correct. This also exercises the enum struct-FIELD marshaling
// path in the VM generators (distinct from the enum ARRAY-ELEMENT path above).
// ═══════════════════════════════════════════════════════════════════════════

const RECS_API_TOML: &str = r#"
[[enum]]
name = "Status"
repr = "u32"
[[enum.variants]]
name = "Idle"
value = "0"
[[enum.variants]]
name = "Busy"
value = "7"

[[types]]
name = "Rec"
fields = [
    { name = "flag", type = "Status" },
    { name = "n",    type = "u32" },
]

[[plugin_contract]]
name = "sys.Recs"
version = "1.0.0"

[[plugin_contract.functions]]
name = "recs"
return = "Array<Rec>"
"#;

const RECS_BUNDLE_TOML: &str = r#"
[bundle]
name = "sys_recs"
version = "1.0.0"
api = "api.toml"
loader = "native"

[bundle.file]
linux.x86_64 = "librecs.so"

[[plugin]]
name = "recs"
implements = ["sys.Recs@1.0"]
"#;

const RECS_MAIN_RS: &str = r##"#[path = "../gen/guest/mod.rs"]
mod generated;

use std::sync::Mutex;

use generated::contracts::SysRecsGuestContract;
use generated::types::{ArrayOf_Rec, Rec, Status};
use polyplug_guest::{GuestError, HostContext, ReturnArena};

struct Plugin {
    arena: Mutex<ReturnArena>,
}

impl SysRecsGuestContract for Plugin {
    fn recs(&self) -> Result<ArrayOf_Rec, GuestError> {
        let mut arena = self.arena.lock().expect("arena lock");
        arena.reset();
        let rows: [Rec; 2] = [
            Rec { flag: Status::Busy, n: 1 },
            Rec { flag: Status::Idle, n: 2 },
        ];
        let (items, len) = arena.alloc_array(&rows);
        Ok(ArrayOf_Rec { items, len })
    }
}

#[unsafe(no_mangle)]
pub fn polyplug_create_recs(host: HostContext) -> Box<dyn SysRecsGuestContract> {
    Box::new(Plugin {
        arena: Mutex::new(ReturnArena::new(host, 4096)),
    })
}

fn main() {
    // SAFETY: null host, never dereferenced (4 KiB buffer never overflows here).
    let host: HostContext = unsafe { HostContext::new(core::ptr::null()) };
    let plugin: Box<dyn SysRecsGuestContract> = polyplug_create_recs(host);
    let w: ArrayOf_Rec = plugin.recs().expect("recs must return Ok");
    // SAFETY: items/len from `alloc_array` on the still-alive arena (no reset since).
    let rs: &[Rec] = unsafe { w.as_slice() };
    assert_eq!(w.len, 2, "recs len");
    assert_eq!(rs[0].flag, Status::Busy, "r0.flag");
    assert_eq!(rs[0].n, 1, "r0.n");
    assert_eq!(rs[1].flag, Status::Idle, "r1.flag");
    assert_eq!(rs[1].n, 2, "r1.n");
    println!("OK: Array<Rec{{enum field}}> round-tripped byte-correct");
}
"##;

#[test]
fn rust_struct_with_enum_field_round_trips() {
    let tmp: TempDir = tempdir().expect("tempdir");
    let project_dir: PathBuf = tmp.path().join("driver");
    let gen_dir: PathBuf = project_dir.join("gen");
    fs::create_dir_all(project_dir.join("src")).expect("create src dir");

    generate_bundle_with(
        &project_dir,
        &gen_dir,
        "rust",
        RECS_API_TOML,
        RECS_BUNDLE_TOML,
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
    fs::write(project_dir.join("src/main.rs"), RECS_MAIN_RS).expect("write src/main.rs");

    let target_dir: PathBuf = tmp.path().join("target");
    let run: Output = Command::new(env!("CARGO"))
        .arg("run")
        .arg("--manifest-path")
        .arg(project_dir.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", &target_dir)
        .output()
        .expect("failed to spawn cargo run for the recs driver");
    assert!(
        run.status.success(),
        "recs driver failed (status {:?})\n--- stdout ---\n{}\n--- stderr ---\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr),
    );
    assert!(
        String::from_utf8_lossy(&run.stdout).contains("round-tripped byte-correct"),
        "driver must report success, got:\n{}",
        String::from_utf8_lossy(&run.stdout),
    );
}

const CPP_RECS_MAIN: &str = r##"#include "guest/init.hpp"
#include <polyplug/guest.hpp>
#include <cassert>
#include <cstdio>

namespace polyplug_plugin {
class RecsImpl : public SysRecsGuestContract {
public:
    RecsImpl() : arena_(4096) {}
    polyplug_generated::ArrayOf_Rec recs() override {
        arena_.reset();
        polyplug_generated::Rec rows[2];
        rows[0].flag = polyplug_generated::Status::Busy;
        rows[0].n = 1;
        rows[1].flag = polyplug_generated::Status::Idle;
        rows[1].n = 2;
        polyplug::ArrayRef ref = arena_.alloc_array(rows, 2);
        return polyplug_generated::ArrayOf_Rec{ref.items, ref.len};
    }
private:
    polyplug::ReturnArena arena_;
};
SysRecsGuestContract* polyplug_create_recs(const HostApi*) { return new RecsImpl(); }
}  // namespace polyplug_plugin

int main() {
    polyplug_plugin::SysRecsGuestContract* impl =
        polyplug_plugin::polyplug_create_recs(nullptr);
    polyplug_generated::ArrayOf_Rec w = impl->recs();
    assert(w.len == 2);
    const polyplug_generated::Rec* rs = w.elements();
    assert(rs[0].flag == polyplug_generated::Status::Busy);
    assert(rs[0].n == 1);
    assert(rs[1].flag == polyplug_generated::Status::Idle);
    assert(rs[1].n == 2);
    std::printf("OK: Array<Rec{enum field}> round-tripped byte-correct\n");
    delete impl;
    return 0;
}
"##;

#[test]
fn cpp_struct_with_enum_field_round_trips() {
    let tmp: TempDir = tempdir().expect("tempdir");
    let project_dir: PathBuf = tmp.path().join("plugin");
    let gen_dir: PathBuf = project_dir.join("gen");
    fs::create_dir_all(&project_dir).expect("create project dir");

    generate_bundle_with(
        &project_dir,
        &gen_dir,
        "cpp",
        RECS_API_TOML,
        RECS_BUNDLE_TOML,
    );

    let main_cpp: PathBuf = project_dir.join("driver.cpp");
    fs::write(&main_cpp, CPP_RECS_MAIN).expect("write driver.cpp");
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
        "c++ build of the recs driver failed (status {:?})\n--- stdout ---\n{}\n--- stderr ---\n{}",
        build.status.code(),
        String::from_utf8_lossy(&build.stdout),
        String::from_utf8_lossy(&build.stderr),
    );

    let run: Output = Command::new(&exe)
        .output()
        .expect("failed to run recs driver");
    assert!(
        run.status.success(),
        "C++ recs driver failed (status {:?})\n--- stdout ---\n{}\n--- stderr ---\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr),
    );
    assert!(
        String::from_utf8_lossy(&run.stdout).contains("round-tripped byte-correct"),
        "C++ recs driver must report success, got:\n{}",
        String::from_utf8_lossy(&run.stdout),
    );
}

const LUA_RECS_BODY: &str = r#"
local ffi = require("ffi")
require("polyplug_abi")
require("generated.guest.types")
local contracts = require("generated.guest.contracts")

local ARENA = ffi.new("uint8_t[?]", 65536)
local cursor = 1
local function arena_alloc(size, _arena)
    local addr = ffi.cast("uintptr_t", ARENA) + cursor
    cursor = cursor + tonumber(size)
    return addr
end

contracts.set_recs_factory(function(_host)
    return {
        recs = function(_self)
            return { { flag = 7, n = 1 }, { flag = 0, n = 2 } }
        end,
    }
end)

local regs = polyplug_init(1, 1)
local entry = regs["sys.Recs"]
assert(entry ~= nil, "sys.Recs must be registered")
local inst = entry.factory(0)

local out = ffi.new("ArrayOf_Rec[1]")
entry.functions[0](inst, 0, ffi.cast("uintptr_t", out), 0, arena_alloc)
assert(tonumber(out[0].len) == 2, "recs len")
local rs = ffi.cast("Rec*", out[0].items)
assert(rs[0].flag == 7, "r0.flag")
assert(rs[0].n == 1, "r0.n")
assert(rs[1].flag == 0, "r1.flag")
assert(rs[1].n == 2, "r1.n")

io.write("OK: Array<Rec{enum field}> round-tripped byte-correct\n")
"#;

#[test]
fn lua_struct_with_enum_field_round_trips() {
    let tmp: TempDir = tempdir().expect("tempdir");
    let project_dir: PathBuf = tmp.path().join("bundle");
    let gen_dir: PathBuf = project_dir.join("generated");
    fs::create_dir_all(&project_dir).expect("create project dir");

    generate_bundle_with(
        &project_dir,
        &gen_dir,
        "lua",
        RECS_API_TOML,
        RECS_BUNDLE_TOML,
    );

    let guest_dir: PathBuf = repo_root().join("sdks").join("lua").join("guest");
    let abi_dir: PathBuf = repo_root().join("sdks").join("lua").join("abi");
    let project_fwd: String = project_dir.to_string_lossy().replace('\\', "/");
    let guest_fwd: String = guest_dir.to_string_lossy().replace('\\', "/");
    let abi_fwd: String = abi_dir.to_string_lossy().replace('\\', "/");
    let driver: String = format!(
        "package.path = \"{project_fwd}/?.lua;{guest_fwd}/?.lua;{abi_fwd}/?.lua;\" .. package.path\n{LUA_RECS_BODY}"
    );
    let driver_path: PathBuf = project_dir.join("driver.lua");
    fs::write(&driver_path, driver).expect("write driver.lua");

    let run: Output = Command::new("luajit")
        .arg(&driver_path)
        .output()
        .expect("failed to spawn luajit");
    assert!(
        run.status.success(),
        "LuaJIT recs driver failed (status {:?})\n--- stdout ---\n{}\n--- stderr ---\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr),
    );
    assert!(
        String::from_utf8_lossy(&run.stdout).contains("round-tripped byte-correct"),
        "LuaJIT recs driver must report success, got:\n{}",
        String::from_utf8_lossy(&run.stdout),
    );
}

const JS_RECS_DRIVER: &str = r#"import { RECS_INTERFACE, setRecsFactory } from "./generated/guest/contracts.ts";

const MEM = new ArrayBuffer(1 << 16);
const DV = new DataView(MEM);
let cursor = 4097;
const bridge = {
  writeU32: (p: number, v: number) => DV.setUint32(p, v >>> 0, true),
  writeI32: (p: number, v: number) => DV.setInt32(p, v | 0, true),
  writeF32: (p: number, v: number) => DV.setFloat32(p, v, true),
  writeF64: (p: number, v: number) => DV.setFloat64(p, v, true),
  writeByte: (p: number, v: number) => DV.setUint8(p, v & 0xff),
  readU32: (p: number) => DV.getUint32(p, true),
  arenaAlloc: (size: number, _arena: number) => {
    const a = cursor;
    cursor += Number(size);
    return [a % 4294967296, Math.floor(a / 4294967296)];
  },
};

setRecsFactory(((_b: any, _lo: number, _hi: number) => ({
  fn0: () => [{ flag: 7, n: 1 }, { flag: 0, n: 2 }],
})) as any);

const impl = (RECS_INTERFACE.factory as any)(bridge, 0, 0);
const OUT = 16;
(RECS_INTERFACE.functions as any)[0](impl, 0, OUT, 0, bridge);
const items = DV.getUint32(OUT, true) + DV.getUint32(OUT + 4, true) * 4294967296;
const len = DV.getUint32(OUT + 8, true);
if (len !== 2) throw new Error("recs len " + len);
// Rec { flag: u32@0, n: u32@4 } -> sizeof 8.
if (DV.getUint32(items, true) !== 7 || DV.getUint32(items + 4, true) !== 1) throw new Error("r0");
if (DV.getUint32(items + 8, true) !== 0 || DV.getUint32(items + 12, true) !== 2) throw new Error("r1");

console.log("OK: Array<Rec{enum field}> round-tripped byte-correct");
"#;

#[test]
fn js_struct_with_enum_field_round_trips() {
    let tmp: TempDir = tempdir().expect("tempdir");
    let project_dir: PathBuf = tmp.path().join("bundle");
    let gen_dir: PathBuf = project_dir.join("generated");
    fs::create_dir_all(&project_dir).expect("create project dir");

    generate_bundle_with(
        &project_dir,
        &gen_dir,
        "js-quickjs",
        RECS_API_TOML,
        RECS_BUNDLE_TOML,
    );

    let driver_path: PathBuf = project_dir.join("driver.ts");
    fs::write(&driver_path, JS_RECS_DRIVER).expect("write driver.ts");

    let run: Output = Command::new("deno")
        .arg("run")
        .arg("--no-lock")
        .arg(&driver_path)
        .output()
        .expect("failed to spawn deno run");
    assert!(
        run.status.success(),
        "Deno recs driver failed (status {:?})\n--- stdout ---\n{}\n--- stderr ---\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr),
    );
    assert!(
        String::from_utf8_lossy(&run.stdout).contains("round-tripped byte-correct"),
        "Deno recs driver must report success, got:\n{}",
        String::from_utf8_lossy(&run.stdout),
    );
}

/// Python driver: this is the direct regression for the enum-struct-field bug —
/// before the fix, importing `Rec` raised `TypeError: this type has no size`.
const PY_RECS_DRIVER: &str = r#"import ctypes
from types import SimpleNamespace
from guest.types import ArrayOf_Rec, Rec
from guest.contracts import recs_recs_abi

BUF = ctypes.create_string_buffer(1 << 16)
BASE = ctypes.addressof(BUF)
_cursor = [1]


def arena_alloc(size, _arena):
    a = BASE + _cursor[0]
    _cursor[0] += int(size)
    return a


impl = SimpleNamespace(recs=lambda: [
    SimpleNamespace(flag=7, n=1),
    SimpleNamespace(flag=0, n=2),
])

out = ArrayOf_Rec()
recs_recs_abi(impl, 0, ctypes.addressof(out), 0, arena_alloc)
assert out.len == 2, "recs len=%d" % out.len
esize = ctypes.sizeof(Rec)


def read(i):
    r = Rec.from_address(out.items + i * esize)
    return (int(r.flag), int(r.n))


assert read(0) == (7, 1), read(0)
assert read(1) == (0, 2), read(1)
print("OK: Array<Rec{enum field}> round-tripped byte-correct")
"#;

#[test]
fn python_struct_with_enum_field_round_trips() {
    let tmp: TempDir = tempdir().expect("tempdir");
    let project_dir: PathBuf = tmp.path().join("bundle");
    let gen_dir: PathBuf = project_dir.join("gen");
    fs::create_dir_all(&project_dir).expect("create project dir");

    generate_bundle_with(
        &project_dir,
        &gen_dir,
        "python",
        RECS_API_TOML,
        RECS_BUNDLE_TOML,
    );

    let driver_path: PathBuf = project_dir.join("driver.py");
    fs::write(&driver_path, PY_RECS_DRIVER).expect("write driver.py");

    let sdk: PathBuf = repo_root().join("sdks").join("python");
    let shim: PathBuf = project_dir.join("shim");
    fs::create_dir_all(shim.join("polyplug").join("abi")).expect("create shim polyplug/abi");
    fs::write(shim.join("polyplug").join("__init__.py"), b"").expect("write polyplug init");
    fs::write(shim.join("polyplug").join("abi").join("__init__.py"), b"")
        .expect("write polyplug.abi init");
    fs::copy(
        sdk.join("abi").join("abi.py"),
        shim.join("polyplug").join("abi").join("abi.py"),
    )
    .expect("copy polyplug/abi/abi.py");

    let pythonpath: String = env::join_paths([
        gen_dir.clone(),
        sdk.join("guest"),
        sdk.join("polyplug_abi"),
        shim,
    ])
    .expect("join PYTHONPATH")
    .to_string_lossy()
    .into_owned();

    let run: Output = Command::new("python3")
        .arg(&driver_path)
        .env("PYTHONPATH", &pythonpath)
        .output()
        .expect("failed to spawn python3");
    assert!(
        run.status.success(),
        "python3 recs driver failed (status {:?})\n--- stdout ---\n{}\n--- stderr ---\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr),
    );
    assert!(
        String::from_utf8_lossy(&run.stdout).contains("round-tripped byte-correct"),
        "python3 recs driver must report success, got:\n{}",
        String::from_utf8_lossy(&run.stdout),
    );
}

/// C# driver for struct-with-enum-field.
const CS_RECS_DRIVER: &str = r####"using Polyplug.Abi;
using Polyplug.Guest;

static void Check(bool cond, string what) { if (!cond) throw new Exception("mismatch: " + what); }

var plugin = new RecsPlugin();
unsafe
{
    ArrayOf_Rec w = plugin.Recs();
    Check(w.len == 2, "recs.len");
    Rec* rs = (Rec*)(void*)(nuint)w.items;
    Check(rs[0].flag == Status.Busy && rs[0].n == 1, "r0");
    Check(rs[1].flag == Status.Idle && rs[1].n == 2, "r1");
}
Console.WriteLine("OK: Array<Rec{enum field}> round-tripped byte-correct");

sealed class RecsPlugin : ISysRecsGuestContract
{
    private readonly ReturnArena _arena = new(4096);
    public ArrayOf_Rec Recs()
    {
        _arena.Reset();
        Rec[] rows =
        {
            new() { flag = Status.Busy, n = 1 },
            new() { flag = Status.Idle, n = 2 },
        };
        var (items, len) = _arena.AllocArray<Rec>(rows);
        return new ArrayOf_Rec { items = items, len = len };
    }
}
"####;

#[test]
fn csharp_struct_with_enum_field_round_trips() {
    let tmp: TempDir = tempdir().expect("tempdir");
    let project_dir: PathBuf = tmp.path().join("bundle");
    let gen_dir: PathBuf = project_dir.join("gen");
    fs::create_dir_all(&project_dir).expect("create project dir");

    generate_bundle_with(
        &project_dir,
        &gen_dir,
        "csharp",
        RECS_API_TOML,
        RECS_BUNDLE_TOML,
    );

    fs::write(project_dir.join("Program.cs"), CS_RECS_DRIVER).expect("write Program.cs");

    let abi_csproj: PathBuf = repo_root().join("sdks/csharp/abi/Polyplug.Abi.csproj");
    let guest_csproj: PathBuf = repo_root().join("sdks/csharp/guest/Polyplug.Guest.csproj");
    let csproj: String = format!(
        "<Project Sdk=\"Microsoft.NET.Sdk\">\n  \
         <PropertyGroup>\n    \
         <OutputType>Exe</OutputType>\n    \
         <TargetFramework>net10.0</TargetFramework>\n    \
         <Nullable>enable</Nullable>\n    \
         <AllowUnsafeBlocks>true</AllowUnsafeBlocks>\n    \
         <ImplicitUsings>enable</ImplicitUsings>\n  \
         </PropertyGroup>\n  \
         <ItemGroup>\n    \
         <ProjectReference Include=\"{abi}\" />\n    \
         <ProjectReference Include=\"{guest}\" />\n  \
         </ItemGroup>\n\
         </Project>\n",
        abi = abi_csproj.display(),
        guest = guest_csproj.display(),
    );
    let csproj_path: PathBuf = project_dir.join("recs.csproj");
    fs::write(&csproj_path, csproj).expect("write recs.csproj");

    let run: Output = Command::new("dotnet")
        .arg("run")
        .arg("-c")
        .arg("Release")
        .arg("--project")
        .arg(&csproj_path)
        .output()
        .expect("failed to spawn dotnet");
    assert!(
        run.status.success(),
        "dotnet recs driver failed (status {:?})\n--- stdout ---\n{}\n--- stderr ---\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr),
    );
    assert!(
        String::from_utf8_lossy(&run.stdout).contains("round-tripped byte-correct"),
        "dotnet recs driver must report success, got:\n{}",
        String::from_utf8_lossy(&run.stdout),
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Enum PARAMETER dispatch (python-specific regression). An enum arrives at the
// ABI as its repr integer, not as a ctypes type. Python read a SINGLE enum param
// with `Status.from_address(args_ptr)` — but a generated enum is `enum.IntEnum`,
// which has no `from_address`, so dispatch raised AttributeError. The multi-param
// arg-pack path already read enum fields through their repr ctype. Fixed in
// `emit_guest_abi_args_unpack`. This is python-only: lua reads the arg as
// `uint32_t`, and the native generators (rust/cpp/csharp) receive enum params as
// their real language enum type. Covers both the single-param and the packed
// (enum + scalar) shapes.
// ═══════════════════════════════════════════════════════════════════════════

const ENUMPARAM_API_TOML: &str = r#"
[[enum]]
name = "Status"
repr = "u32"
[[enum.variants]]
name = "Idle"
value = "0"
[[enum.variants]]
name = "Busy"
value = "7"

[[plugin_contract]]
name = "sys.P"
version = "1.0.0"

[[plugin_contract.functions]]
name = "one"
params = [ { name = "s", type = "Status" } ]
return = "u32"

[[plugin_contract.functions]]
name = "two"
params = [ { name = "s", type = "Status" }, { name = "n", type = "u32" } ]
return = "u32"
"#;

const ENUMPARAM_BUNDLE_TOML: &str = r#"
[bundle]
name = "sys_p"
version = "1.0.0"
api = "api.toml"
loader = "native"

[bundle.file]
linux.x86_64 = "libp.so"

[[plugin]]
name = "p"
implements = ["sys.P@1.0"]
"#;

const PY_ENUMPARAM_DRIVER: &str = r#"import ctypes
from types import SimpleNamespace
from guest.types import SysPContractTwoArgs
from guest.contracts import p_one_abi, p_two_abi

impl = SimpleNamespace(one=lambda s: int(s) + 100, two=lambda s, n: int(s) + int(n))


def noop(_size, _arena):
    return 0


# Single enum param: read in place as its repr integer.
one_args = ctypes.c_uint32(7)  # Busy
one_out = ctypes.c_uint32(0)
p_one_abi(impl, ctypes.addressof(one_args), ctypes.addressof(one_out), 0, noop)
assert one_out.value == 107, one_out.value

# Enum + scalar, read through the generated arg-pack struct (enum field = repr).
two_args = SysPContractTwoArgs(s=7, n=5)
two_out = ctypes.c_uint32(0)
p_two_abi(impl, ctypes.addressof(two_args), ctypes.addressof(two_out), 0, noop)
assert two_out.value == 12, two_out.value

print("OK: enum params dispatched byte-correct")
"#;

#[test]
fn python_enum_param_dispatches() {
    let tmp: TempDir = tempdir().expect("tempdir");
    let project_dir: PathBuf = tmp.path().join("bundle");
    let gen_dir: PathBuf = project_dir.join("gen");
    fs::create_dir_all(&project_dir).expect("create project dir");

    generate_bundle_with(
        &project_dir,
        &gen_dir,
        "python",
        ENUMPARAM_API_TOML,
        ENUMPARAM_BUNDLE_TOML,
    );

    let driver_path: PathBuf = project_dir.join("driver.py");
    fs::write(&driver_path, PY_ENUMPARAM_DRIVER).expect("write driver.py");

    let sdk: PathBuf = repo_root().join("sdks").join("python");
    let shim: PathBuf = project_dir.join("shim");
    fs::create_dir_all(shim.join("polyplug").join("abi")).expect("create shim polyplug/abi");
    fs::write(shim.join("polyplug").join("__init__.py"), b"").expect("write polyplug init");
    fs::write(shim.join("polyplug").join("abi").join("__init__.py"), b"")
        .expect("write polyplug.abi init");
    fs::copy(
        sdk.join("abi").join("abi.py"),
        shim.join("polyplug").join("abi").join("abi.py"),
    )
    .expect("copy polyplug/abi/abi.py");

    let pythonpath: String = env::join_paths([
        gen_dir.clone(),
        sdk.join("guest"),
        sdk.join("polyplug_abi"),
        shim,
    ])
    .expect("join PYTHONPATH")
    .to_string_lossy()
    .into_owned();

    let run: Output = Command::new("python3")
        .arg(&driver_path)
        .env("PYTHONPATH", &pythonpath)
        .output()
        .expect("failed to spawn python3");
    assert!(
        run.status.success(),
        "python3 enum-param driver failed (status {:?})\n--- stdout ---\n{}\n--- stderr ---\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr),
    );
    assert!(
        String::from_utf8_lossy(&run.stdout).contains("dispatched byte-correct"),
        "python3 enum-param driver must report success, got:\n{}",
        String::from_utf8_lossy(&run.stdout),
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Buffer regression guard (rust). `Buffer` is an OWNING type — deliberately NOT
// `Copy` (unlike borrowed `StringView`) — so a struct/arg-pack embedding it must
// NOT derive `Copy/Clone`, must `use polyplug_abi::Buffer`, and a multi-param
// unpack must read the POD pack by value (`core::ptr::read`) rather than moving
// the non-`Copy` field out of a shared `&pack`. All three were real bugs fixed
// in commit 324ee54c (found via the CheatGear contract, which lives outside this
// suite). This test locks them in: a contract with `Buffer` as a struct field
// AND as a standalone param must generate rust glue that COMPILES.
// ═══════════════════════════════════════════════════════════════════════════

const BUFFER_API_TOML: &str = r#"
[[types]]
name = "Frame"
fields = [
    { name = "data", type = "Buffer" },
    { name = "id",   type = "u32" },
]

[[plugin_contract]]
name = "sys.Sink"
version = "1.0.0"

[[plugin_contract.functions]]
name = "push"
params = [
    { name = "frame", type = "Frame" },
    { name = "extra", type = "Buffer" },
]
return = "u32"
"#;

const BUFFER_BUNDLE_TOML: &str = r#"
[bundle]
name = "sys_sink"
version = "1.0.0"
api = "api.toml"
loader = "native"

[bundle.file]
linux.x86_64 = "libsink.so"

[[plugin]]
name = "sink"
implements = ["sys.Sink@1.0"]
"#;

/// Implements the generated `Buffer`-carrying trait and moves the non-`Copy`
/// `Buffer` field out of the by-value `Frame` — which only compiles if the
/// generated types are `use`d, non-`Copy`, and unpacked by value.
const BUFFER_MAIN_RS: &str = r##"#[path = "../gen/guest/mod.rs"]
mod generated;

use generated::contracts::SysSinkGuestContract;
use generated::types::Frame;
use polyplug_abi::Buffer;
use polyplug_guest::{GuestError, HostContext};

struct Sink;

impl SysSinkGuestContract for Sink {
    fn push(&self, frame: Frame, extra: Buffer) -> Result<u32, GuestError> {
        // Move both non-`Copy` Buffers out to prove they are owned values, not
        // borrows behind a shared reference.
        let _owned_data: Buffer = frame.data;
        let _owned_extra: Buffer = extra;
        Ok(frame.id)
    }
}

#[unsafe(no_mangle)]
pub fn polyplug_create_sink(_host: HostContext) -> Box<dyn SysSinkGuestContract> {
    Box::new(Sink)
}

fn main() {
    // SAFETY: null host, never dereferenced (Sink stores nothing from it).
    let host: HostContext = unsafe { HostContext::new(core::ptr::null()) };
    let _plugin: Box<dyn SysSinkGuestContract> = polyplug_create_sink(host);
    println!("OK: Buffer-in-struct-and-param compiles");
}
"##;

#[test]
fn rust_buffer_in_struct_and_param_compiles() {
    let tmp: TempDir = tempdir().expect("tempdir");
    let project_dir: PathBuf = tmp.path().join("driver");
    let gen_dir: PathBuf = project_dir.join("gen");
    fs::create_dir_all(project_dir.join("src")).expect("create src dir");

    generate_bundle_with(
        &project_dir,
        &gen_dir,
        "rust",
        BUFFER_API_TOML,
        BUFFER_BUNDLE_TOML,
    );
    assert!(
        gen_dir.join("guest/types.rs").exists(),
        "generated guest/types.rs must exist"
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
    fs::write(project_dir.join("src/main.rs"), BUFFER_MAIN_RS).expect("write src/main.rs");

    let target_dir: PathBuf = tmp.path().join("target");
    let build: Output = Command::new(env!("CARGO"))
        .arg("build")
        .arg("--manifest-path")
        .arg(project_dir.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", &target_dir)
        .output()
        .expect("failed to spawn cargo build for the buffer driver");
    assert!(
        build.status.success(),
        "Buffer-in-struct-and-param glue failed to compile (status {:?})\n--- stdout ---\n{}\n--- stderr ---\n{}",
        build.status.code(),
        String::from_utf8_lossy(&build.stdout),
        String::from_utf8_lossy(&build.stderr),
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Python types module must import `Buffer` when a struct has a `Buffer` field.
// The `polyplug_abi` imports were hardcoded to `StringView` only, so the `Frame`
// struct above (`data: Buffer`) emitted `("data", Buffer)` with `Buffer`
// undefined → `NameError` at class creation. Surfaced generating the CheatGear
// SDK (its `ReadResult` carries a `Buffer`). Fixed in `python_types_abi_imports`.
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn python_buffer_in_struct_types_import() {
    let tmp: TempDir = tempdir().expect("tempdir");
    let project_dir: PathBuf = tmp.path().join("bundle");
    let gen_dir: PathBuf = project_dir.join("gen");
    fs::create_dir_all(&project_dir).expect("create project dir");

    generate_bundle_with(
        &project_dir,
        &gen_dir,
        "python",
        BUFFER_API_TOML,
        BUFFER_BUNDLE_TOML,
    );

    // Instantiating `Frame` (which has a `Buffer` field) fails at class creation
    // if `Buffer` was not imported into the generated types module.
    let driver: &str = "from guest.types import Frame\nf = Frame()\nprint('OK: Buffer-field struct imports and instantiates')\n";
    let driver_path: PathBuf = project_dir.join("driver.py");
    fs::write(&driver_path, driver).expect("write driver.py");

    let sdk: PathBuf = repo_root().join("sdks").join("python");
    let shim: PathBuf = project_dir.join("shim");
    fs::create_dir_all(shim.join("polyplug").join("abi")).expect("create shim polyplug/abi");
    fs::write(shim.join("polyplug").join("__init__.py"), b"").expect("write polyplug init");
    fs::write(shim.join("polyplug").join("abi").join("__init__.py"), b"")
        .expect("write polyplug.abi init");
    fs::copy(
        sdk.join("abi").join("abi.py"),
        shim.join("polyplug").join("abi").join("abi.py"),
    )
    .expect("copy polyplug/abi/abi.py");

    let pythonpath: String = env::join_paths([
        gen_dir.clone(),
        sdk.join("guest"),
        sdk.join("polyplug_abi"),
        shim,
    ])
    .expect("join PYTHONPATH")
    .to_string_lossy()
    .into_owned();

    let run: Output = Command::new("python3")
        .arg(&driver_path)
        .env("PYTHONPATH", &pythonpath)
        .output()
        .expect("failed to spawn python3");
    assert!(
        run.status.success(),
        "python3 Buffer-field types import failed (status {:?})\n--- stdout ---\n{}\n--- stderr ---\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr),
    );
    assert!(
        String::from_utf8_lossy(&run.stdout).contains("imports and instantiates"),
        "driver must report success, got:\n{}",
        String::from_utf8_lossy(&run.stdout),
    );
}
