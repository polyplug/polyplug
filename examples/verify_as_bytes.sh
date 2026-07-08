#!/usr/bin/env bash
#
# verify_as_bytes.sh — cross-language runtime proof that the Buffer `as_bytes`
# helper returns the buffer's bytes BYTE-EXACT (never UTF-8 decoded) in every
# language SDK, and yields an empty view for a zero-length buffer.
#
# sdk_validator enforces that the helper EXISTS; this script proves the BEHAVIOR
# by executing each SDK's real `as_bytes` against a fixed buffer whose bytes are
# {0x00, 0xFF, 0x41} — an interior NUL and a byte (0xFF) that is never valid
# UTF-8, so any helper that decoded instead of borrowing raw bytes would corrupt
# or throw. Each language prints "<hex> <zero_len>":
#   hex      = lowercase hex of as_bytes over the 3-byte buffer  → "00ff41"
#   zero_len = length of as_bytes over a zero-length buffer       → "0"
# so the fixed expectation is "00ff41 0".
#
# Zero-copy (the "don't copy where the runtime allows it" contract) is asserted
# structurally in the Rust unit tests (pointer identity) and the C# xunit
# AsBytes tests (mutate-then-observe); the JS runtimes copy by necessity (Deno
# FFI getArrayBuffer / QuickJS bridge cannot view host memory), which is why this
# script checks only byte-exactness — the one property every language shares.
#
# A missing toolchain is skipped loudly, not failed (mirrors verify_to_str_errors.sh);
# any executed language whose output differs from the expectation fails the script.
set -u

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

EXPECTED="00ff41 0"
FAIL=0
RAN=0

check() {
    local lang="$1" got="$2"
    RAN=$((RAN + 1))
    if [ "$got" = "$EXPECTED" ]; then
        echo "  [$lang] OK"
    else
        echo "  [$lang] MISMATCH"
        echo "      got:      '$got'"
        echo "      expected: '$EXPECTED'"
        FAIL=$((FAIL + 1))
    fi
}

echo "== Buffer as_bytes byte-exactness parity =="
echo "expected per language: '$EXPECTED'  (00ff41 = raw bytes incl. NUL + 0xFF, 0 = empty for zero-length buffer)"

# ── Rust (authority): guest SDK unit tests ────────────────────────────────────
if command -v cargo >/dev/null 2>&1; then
    if cargo test -q -p polyplug_guest >/dev/null 2>&1; then
        echo "  [rust] OK (polyplug_guest as_bytes byte-exact + zero-copy tests pass)"
        RAN=$((RAN + 1))
    else
        echo "  [rust] FAIL (polyplug_guest tests failed)"
        FAIL=$((FAIL + 1))
    fi
else
    echo "  [rust] SKIPPED (cargo not found)"
fi

# ── Python ────────────────────────────────────────────────────────────────────
if command -v python3 >/dev/null 2>&1; then
    OUT="$(python3 -c "
import sys, ctypes
sys.path.insert(0, '$ROOT/sdks/python')
sys.path.insert(0, '$ROOT/sdks/python/polyplug_abi')
from polyplug_abi.string_view_helper import as_bytes
from polyplug_abi.abi import Buffer
data = (ctypes.c_ubyte * 3)(0x00, 0xff, 0x41)
addr = ctypes.cast(data, ctypes.c_void_p).value
buf = Buffer(ptr=addr, len=3, cap=3)
hx = bytes(as_bytes(buf)).hex()
z = Buffer(ptr=addr, len=0, cap=3)
print(hx, len(as_bytes(z)))" 2>/dev/null)"
    check python "$OUT"
else
    echo "  [python] SKIPPED (python3 not found)"
fi

# ── Lua (LuaJIT) ──────────────────────────────────────────────────────────────
if command -v luajit >/dev/null 2>&1; then
    OUT="$(luajit -e "
local M = dofile('$ROOT/sdks/lua/abi/abi.lua')
local ffi = require('ffi')
local data = ffi.new('uint8_t[3]', {0x00, 0xff, 0x41})
local buf = ffi.new('Buffer'); buf.ptr = data; buf.len = 3; buf.cap = 3
local ptr, len = M.as_bytes(buf)
local hx = ''
for i = 0, len - 1 do hx = hx .. string.format('%02x', ptr[i]) end
local z = ffi.new('Buffer'); z.ptr = data; z.len = 0; z.cap = 3
local _, zlen = M.as_bytes(z)
io.write(hx, ' ', zlen)" 2>/dev/null)"
    check lua "$OUT"
else
    echo "  [lua] SKIPPED (luajit not found)"
fi

# ── JavaScript (Deno host abi mirror) ─────────────────────────────────────────
if command -v deno >/dev/null 2>&1; then
    cat > "$TMP/asbytes_abi.ts" <<EOF
import { asBytes } from "$ROOT/sdks/js/abi/abi.ts";
const data = new Uint8Array([0x00, 0xff, 0x41]);
const buf = { ptr: BigInt(Deno.UnsafePointer.value(Deno.UnsafePointer.of(data))), len: 3, cap: 3 };
let hx = ""; for (const b of asBytes(buf)) hx += b.toString(16).padStart(2, "0");
const z = { ptr: buf.ptr, len: 0, cap: 3 };
console.log(hx, asBytes(z).length);
EOF
    OUT="$(deno run --quiet --allow-ffi --unstable-ffi "$TMP/asbytes_abi.ts" 2>/dev/null)"
    check js-deno "$OUT"

    cat > "$TMP/asbytes_guest.ts" <<EOF
const { asBytes } = await import("$ROOT/sdks/js/guest/polyplug_guest.js");
const bridge = { readMemory: () => new Uint8Array([0x00, 0xff, 0x41]).buffer };
let hx = ""; for (const b of asBytes(bridge, { ptr_hi: 0, ptr_lo: 1, len: 3 })) hx += b.toString(16).padStart(2, "0");
console.log(hx, asBytes(bridge, { ptr_hi: 0, ptr_lo: 0, len: 0 }).length);
EOF
    OUT="$(deno run --quiet "$TMP/asbytes_guest.ts" 2>/dev/null)"
    check js-quickjs-guest "$OUT"
else
    echo "  [js] SKIPPED (deno not found)"
fi

# ── C++ (g++) ─────────────────────────────────────────────────────────────────
if command -v g++ >/dev/null 2>&1; then
    cat > "$TMP/asbytes.cpp" <<EOF
#include <cstdio>
#include "polyplug/abi.hpp"
using namespace polyplug::abi;
int main() {
    unsigned char data[3] = {0x00, 0xff, 0x41};
    Buffer buf{data, 3, 3};
    for (auto b : as_bytes(buf)) printf("%02x", b);
    Buffer z{data, 0, 3};
    printf(" %zu", as_bytes(z).size());
}
EOF
    if g++ -std=c++20 -I "$ROOT/sdks/cpp/abi" "$TMP/asbytes.cpp" -o "$TMP/asbytescpp" 2>/dev/null; then
        check cpp "$("$TMP/asbytescpp")"
    else
        echo "  [cpp] FAIL (compilation error)"
        FAIL=$((FAIL + 1))
    fi
else
    echo "  [cpp] SKIPPED (g++ not found)"
fi

# ── C# (.NET) ─────────────────────────────────────────────────────────────────
if command -v dotnet >/dev/null 2>&1; then
    mkdir -p "$TMP/cs"
    cat > "$TMP/cs/asbytes.csproj" <<EOF
<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutputType>Exe</OutputType>
    <TargetFramework>net10.0</TargetFramework>
    <ImplicitUsings>enable</ImplicitUsings>
    <Nullable>enable</Nullable>
    <AllowUnsafeBlocks>true</AllowUnsafeBlocks>
  </PropertyGroup>
  <ItemGroup>
    <Compile Include="$ROOT/sdks/csharp/abi/Abi.cs" />
  </ItemGroup>
</Project>
EOF
    cat > "$TMP/cs/Program.cs" <<EOF
using System.Runtime.InteropServices;
using Polyplug.Abi;
byte[] data = { 0x00, 0xff, 0x41 };
GCHandle h = GCHandle.Alloc(data, GCHandleType.Pinned);
var buf = new Polyplug.Abi.Buffer { Ptr = h.AddrOfPinnedObject(), Len = (nuint)3, Cap = (nuint)3 };
var sb = new System.Text.StringBuilder();
foreach (byte b in buf.AsBytes()) sb.Append(b.ToString("x2"));
var z = new Polyplug.Abi.Buffer { Ptr = h.AddrOfPinnedObject(), Len = (nuint)0, Cap = (nuint)3 };
System.Console.WriteLine(\$"{sb} {z.AsBytes().Length}");
EOF
    OUT="$(dotnet run -c Release --project "$TMP/cs" 2>/dev/null)"
    check csharp "$OUT"
else
    echo "  [csharp] SKIPPED (dotnet not found)"
fi

echo
if [ "$FAIL" -ne 0 ]; then
    echo "RESULT: $FAIL language(s) failed the as_bytes byte-exactness contract ($RAN executed)"
    exit 1
fi
echo "RESULT: all $RAN executed languages return byte-exact as_bytes and empty for a zero-length buffer"
