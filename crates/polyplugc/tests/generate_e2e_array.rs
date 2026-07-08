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
