#!/usr/bin/env bash
#
# verify_to_str_errors.sh — cross-language runtime proof that the StringView
# `to_str` helper ERRORS on a readable-but-invalid UTF-8 view (owner decision:
# "to_str invalid UTF-8 must error"), in every language SDK, while a valid view
# still decodes normally.
#
# sdk_validator enforces that the helpers EXIST; docs/SDK_HELPERS.md pins the
# semantics; this script proves the BEHAVIOR by executing each SDK's real helper
# against a fixed invalid view (bytes 0x68 0x69 0xFF = "hi" + a lone 0xFF, which
# is never valid UTF-8) and a valid view ("hi"). Each language must print
# "ERR hi": ERR = the invalid view raised/threw/returned Err, hi = the valid
# view decoded.
#
# A missing toolchain is skipped loudly, not failed (mirrors verify_hosts.sh /
# verify_id_helpers.sh); any executed language that does NOT error on the invalid
# view (or mangles the valid one) fails the script.
set -u

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

EXPECTED="ERR hi"
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

echo "== to_str invalid-UTF-8 error parity =="
echo "expected per language: '$EXPECTED'  (ERR = invalid view errored, hi = valid view decoded)"

# ── Rust (authority): guest SDK unit tests ────────────────────────────────────
if command -v cargo >/dev/null 2>&1; then
    if cargo test -q -p polyplug_guest >/dev/null 2>&1; then
        echo "  [rust] OK (polyplug_guest to_str invalid-UTF-8 tests pass)"
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
# sdks/python makes 'abi' resolve as a PEP-420 namespace package (abi.abi is the
# canonical generated module the polyplug_abi package re-exports); the second
# path makes the polyplug_abi package itself importable.
sys.path.insert(0, '$ROOT/sdks/python')
sys.path.insert(0, '$ROOT/sdks/python/polyplug_abi')
from polyplug_abi.string_view_helper import to_str
from polyplug_abi.abi import StringView
def mk(b):
    buf = ctypes.create_string_buffer(b, len(b))
    return StringView(ptr=ctypes.cast(buf, ctypes.c_void_p).value, len=len(b)), buf
sv, _keep = mk(b'hi\xff')
try:
    to_str(sv); inv='NOERR'
except UnicodeDecodeError:
    inv='ERR'
sv2, _k2 = mk(b'hi')
print(inv, to_str(sv2))" 2>/dev/null)"
    check python "$OUT"
else
    echo "  [python] SKIPPED (python3 not found)"
fi

# ── Lua (LuaJIT) ──────────────────────────────────────────────────────────────
if command -v luajit >/dev/null 2>&1; then
    OUT="$(luajit -e "
local M = dofile('$ROOT/sdks/lua/abi/abi.lua')
local ffi = require('ffi')
local function mk(t)
    local b = ffi.new('uint8_t[?]', #t, t)
    local sv = ffi.new('StringView')
    sv.ptr = b; sv.len = #t
    return sv, b
end
local sv, keep = mk({0x68, 0x69, 0xff})
local ok = pcall(M.to_str, sv)
local inv = ok and 'NOERR' or 'ERR'
local sv2, keep2 = mk({0x68, 0x69})
io.write(inv, ' ', M.to_str(sv2))" 2>/dev/null)"
    check lua "$OUT"
else
    echo "  [lua] SKIPPED (luajit not found)"
fi

# ── JavaScript (Deno host abi mirror) ─────────────────────────────────────────
if command -v deno >/dev/null 2>&1; then
    cat > "$TMP/tostr_abi.ts" <<EOF
import { stringViewToString } from "$ROOT/sdks/js/abi/abi.ts";
const bad = new Uint8Array([0x68, 0x69, 0xff]);
const svBad = { ptr: BigInt(Deno.UnsafePointer.value(Deno.UnsafePointer.of(bad))), len: 3 };
let inv = "NOERR";
try { stringViewToString(svBad); } catch (_e) { inv = "ERR"; }
const good = new Uint8Array([0x68, 0x69]);
const svGood = { ptr: BigInt(Deno.UnsafePointer.value(Deno.UnsafePointer.of(good))), len: 2 };
console.log(inv, stringViewToString(svGood));
EOF
    OUT="$(deno run --quiet --allow-ffi --unstable-ffi "$TMP/tostr_abi.ts" 2>/dev/null)"
    check js-deno "$OUT"

    # QuickJS guest path: force the manual _decodeUtf8 decoder (TextDecoder off)
    # and stub the host readMemory bridge so toStr exercises the validating scan.
    cat > "$TMP/tostr_guest.ts" <<EOF
let buf = new Uint8Array([0x68, 0x69, 0xff]).buffer;
(globalThis as Record<string, unknown>).polyplug = { readMemory: () => buf };
(globalThis as Record<string, unknown>).TextDecoder = undefined;
const { toStr } = await import("$ROOT/sdks/js/guest/polyplug_guest.js");
let inv = "NOERR";
try { toStr({ ptr_hi: 0, ptr_lo: 1, len: 3 }); } catch (_e) { inv = "ERR"; }
buf = new Uint8Array([0x68, 0x69]).buffer;
console.log(inv, toStr({ ptr_hi: 0, ptr_lo: 1, len: 2 }));
EOF
    OUT="$(deno run --quiet "$TMP/tostr_guest.ts" 2>/dev/null)"
    check js-quickjs-guest "$OUT"
else
    echo "  [js] SKIPPED (deno not found)"
fi

# ── C++ (g++) ─────────────────────────────────────────────────────────────────
if command -v g++ >/dev/null 2>&1; then
    cat > "$TMP/tostr.cpp" <<EOF
#include <cstdio>
#include <string>
#include "polyplug/abi.hpp"
using namespace polyplug::abi;
int main() {
    const unsigned char bad[3] = {0x68, 0x69, 0xff};
    StringView sv{bad, 3};
    const char* inv;
    try { (void)to_str(sv); inv = "NOERR"; }
    catch (const std::exception&) { inv = "ERR"; }
    const unsigned char good[2] = {0x68, 0x69};
    StringView sv2{good, 2};
    std::string val = to_str(sv2);
    printf("%s %s", inv, val.c_str());
}
EOF
    if g++ -std=c++17 -I "$ROOT/sdks/cpp/abi" "$TMP/tostr.cpp" -o "$TMP/tostrcpp" 2>/dev/null; then
        check cpp "$("$TMP/tostrcpp")"
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
    cat > "$TMP/cs/tostr.csproj" <<EOF
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
using System.Text;
using Polyplug.Abi;
byte[] bad = { 0x68, 0x69, 0xff };
GCHandle hb = GCHandle.Alloc(bad, GCHandleType.Pinned);
var sv = new StringView { Ptr = hb.AddrOfPinnedObject(), Len = (nuint)3 };
string inv;
try { _ = StringViewHelper.ToStr(sv); inv = "NOERR"; }
catch (DecoderFallbackException) { inv = "ERR"; }
byte[] good = { 0x68, 0x69 };
GCHandle hg = GCHandle.Alloc(good, GCHandleType.Pinned);
var sv2 = new StringView { Ptr = hg.AddrOfPinnedObject(), Len = (nuint)2 };
System.Console.WriteLine(\$"{inv} {StringViewHelper.ToStr(sv2)}");
EOF
    OUT="$(dotnet run -c Release --project "$TMP/cs" 2>/dev/null)"
    check csharp "$OUT"
else
    echo "  [csharp] SKIPPED (dotnet not found)"
fi

echo
if [ "$FAIL" -ne 0 ]; then
    echo "RESULT: $FAIL language(s) failed the invalid-UTF-8 error contract ($RAN executed)"
    exit 1
fi
echo "RESULT: all $RAN executed languages error on invalid UTF-8 and decode valid UTF-8"
