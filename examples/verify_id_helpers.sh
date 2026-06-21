#!/usr/bin/env bash
#
# verify_id_helpers.sh — cross-language parity gate for the contract/bundle ID
# helpers (fnv1a_64 / bundle_id / guest_contract_id / host_contract_id).
#
# Each language SDK ships its own FNV-1a 64-bit implementation (emitted from the
# per-language constants in crates/polyplug_abi/build/generate.rs, except Rust
# which uses crates/polyplug_utils directly). The hashes MUST be byte-identical
# across all six languages or a plugin built in one language cannot be resolved
# by a host in another. sdk_validator enforces that the helpers EXIST; this
# script proves they COMPUTE THE SAME VALUES by executing each SDK against a
# fixed golden vector set.
#
# Rust is the authority (its golden values are pinned in
# crates/polyplug_utils unit tests). A missing toolchain is skipped loudly, not
# failed (mirrors examples/verify_hosts.sh); any executed language that
# disagrees with the golden set fails the script.
set -u

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

# Golden vectors: "<input-description> <expected-hex16>". Derived from the
# canonical FNV-1a 64-bit scheme in crates/polyplug_utils (Rust authority).
G_EMPTY=cbf29ce484222325   # fnv1a_64("")
G_BUNDLE=fe6226876e3a35b2  # bundle_id("my-bundle")
G_GLOG=a1adf81fd5134c83    # guest_contract_id("logger", 1)
G_HLOG=ee2f1db90b2b5eff    # host_contract_id("logger", 1)
G_GIMG=fbf31bf02e2ab1dc    # guest_contract_id("image.decode", 1)
EXPECTED="$G_EMPTY $G_BUNDLE $G_GLOG $G_HLOG $G_GIMG"

FAIL=0
RAN=0

check() {
    local lang="$1" got="$2"
    RAN=$((RAN + 1))
    if [ "$got" = "$EXPECTED" ]; then
        echo "  [$lang] OK"
    else
        echo "  [$lang] MISMATCH"
        echo "      got:      $got"
        echo "      expected: $EXPECTED"
        FAIL=$((FAIL + 1))
    fi
}

echo "== contract/bundle ID helper parity =="
echo "expected: $EXPECTED"

# ── Rust (authority): unit-test golden pins ───────────────────────────────────
if command -v cargo >/dev/null 2>&1; then
    if cargo test -q -p polyplug_utils >/dev/null 2>&1; then
        echo "  [rust] OK (polyplug_utils golden tests pass)"
        RAN=$((RAN + 1))
    else
        echo "  [rust] FAIL (polyplug_utils golden tests failed)"
        FAIL=$((FAIL + 1))
    fi
else
    echo "  [rust] SKIPPED (cargo not found)"
fi

# ── Python ────────────────────────────────────────────────────────────────────
if command -v python3 >/dev/null 2>&1; then
    OUT="$(python3 -c "
import sys; sys.path.insert(0, '$ROOT/sdks/python/abi'); import abi
print('%016x %016x %016x %016x %016x' % (
    abi.fnv1a_64(b''), abi.bundle_id('my-bundle'),
    abi.guest_contract_id('logger', 1), abi.host_contract_id('logger', 1),
    abi.guest_contract_id('image.decode', 1)))" 2>/dev/null)"
    check python "$OUT"
else
    echo "  [python] SKIPPED (python3 not found)"
fi

# ── Lua (LuaJIT) ──────────────────────────────────────────────────────────────
if command -v luajit >/dev/null 2>&1; then
    OUT="$(luajit -e "
local M = dofile('$ROOT/sdks/lua/abi/abi.lua')
local function h(x) return string.format('%016x', x) end
io.write(h(M.fnv1a_64('')), ' ', h(M.bundle_id('my-bundle')), ' ',
         h(M.guest_contract_id('logger', 1)), ' ', h(M.host_contract_id('logger', 1)), ' ',
         h(M.guest_contract_id('image.decode', 1)))" 2>/dev/null)"
    check lua "$OUT"
else
    echo "  [lua] SKIPPED (luajit not found)"
fi

# ── JavaScript (Deno) ─────────────────────────────────────────────────────────
if command -v deno >/dev/null 2>&1; then
    cat > "$TMP/id.ts" <<EOF
import { fnv1a64, bundleId, guestContractId, hostContractId } from "$ROOT/sdks/js/abi/abi.ts";
const h = (x: bigint) => x.toString(16).padStart(16, '0');
console.log(h(fnv1a64('')), h(bundleId('my-bundle')), h(guestContractId('logger', 1)),
            h(hostContractId('logger', 1)), h(guestContractId('image.decode', 1)));
EOF
    OUT="$(deno run --quiet "$TMP/id.ts" 2>/dev/null)"
    check js "$OUT"
else
    echo "  [js] SKIPPED (deno not found)"
fi

# ── C++ (g++) ─────────────────────────────────────────────────────────────────
if command -v g++ >/dev/null 2>&1; then
    cat > "$TMP/id.cpp" <<EOF
#include <cstdio>
#include <cinttypes>
#include "polyplug/id.hpp"
using namespace polyplug;
int main() {
    printf("%016" PRIx64 " %016" PRIx64 " %016" PRIx64 " %016" PRIx64 " %016" PRIx64 "\n",
           fnv1a_64(""), bundle_id("my-bundle"), guest_contract_id("logger", 1),
           host_contract_id("logger", 1), guest_contract_id("image.decode", 1));
}
EOF
    if g++ -std=c++17 -I "$ROOT/sdks/cpp/host" "$TMP/id.cpp" -o "$TMP/idcpp" 2>/dev/null; then
        check cpp "$("$TMP/idcpp")"
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
    cat > "$TMP/cs/idcheck.csproj" <<EOF
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
using Polyplug.Abi;
string h(ulong x) => x.ToString("x16");
System.Console.WriteLine(\$"{h(ContractId.Fnv1a64(""))} {h(ContractId.BundleId("my-bundle"))} {h(ContractId.GuestContractId("logger", 1))} {h(ContractId.HostContractId("logger", 1))} {h(ContractId.GuestContractId("image.decode", 1))}");
EOF
    OUT="$(dotnet run -c Release --project "$TMP/cs" 2>/dev/null)"
    check csharp "$OUT"
else
    echo "  [csharp] SKIPPED (dotnet not found)"
fi

echo
if [ "$FAIL" -ne 0 ]; then
    echo "RESULT: $FAIL language(s) disagree with the golden vectors ($RAN executed)"
    exit 1
fi
echo "RESULT: all $RAN executed languages agree with the golden vectors"
