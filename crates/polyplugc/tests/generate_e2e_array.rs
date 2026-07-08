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
