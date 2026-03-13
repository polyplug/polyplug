//! Type mapping edge-case integration tests for polyplug_codegen.
//!
//! Tests uncovered edge cases only — cases that are NOT exercised by the
//! existing unit tests inside the generator source files:
//!
//! 1. U64 / I64 map to `bigint` in the Deno (V8) TypeScript generator.
//! 2. U64 / I64 map to `{ lo: number; hi: number }` in the QuickJS generator.
//! 3. C++ struct emitter maps U64 fields to `uint64_t` with no alignment
//!    specifier (current behaviour — generator does not emit `alignas`).
//! 4. C# user-type emitter always emits `StructLayout(LayoutKind.Sequential)`,
//!    never `LayoutKind.Explicit`.

#![allow(clippy::expect_used)]

use polyplug_codegen::{generate, GenerateConfig, Lang, Side};
use std::io::Write as _;

// ─── TOML helpers ─────────────────────────────────────────────────────────────

/// Write `content` to a named temp file and return the `tempfile::NamedTempFile`.
/// The caller must keep the returned value alive for as long as the path is used.
fn write_temp_toml(content: &str) -> tempfile::NamedTempFile {
    let mut file: tempfile::NamedTempFile = tempfile::Builder::new()
        .suffix(".toml")
        .tempfile()
        .expect("create temp file");
    file.write_all(content.as_bytes()).expect("write temp toml");
    file
}

/// API TOML that declares a contract with a u64 param and an i64 return value,
/// plus a struct that carries both u64 and i64 fields.
const U64_I64_API_TOML: &str = r#"
[[types]]
name = "TimestampPair"
fields = [
  { name = "start_ns", type = "u64" },
  { name = "end_ns",   type = "i64" }
]

[[contract]]
name = "bench.timer"
version = "1.0.0"

[[contract.functions]]
name = "elapsed"
params = [{ name = "start", type = "u64" }]
return = "i64"

[[contract.functions]]
name = "pair"
return = "TimestampPair"
"#;

/// Convenience: run `generate()` against an inline TOML string.
///
/// Returns the list of generated files (path + content) produced by the
/// specified language / side combination.
fn run_generate(
    toml_content: &str,
    lang: Lang,
    side: Side,
) -> Vec<polyplug_codegen::GeneratedFile> {
    let tmp: tempfile::NamedTempFile = write_temp_toml(toml_content);
    let config: GenerateConfig = GenerateConfig {
        api_toml: tmp.path().to_path_buf(),
        lang,
        side,
        out_dir: std::path::PathBuf::from("/tmp/polyplug_type_mapping_edge_cases"),
    };
    generate(config)
        .expect("generate() must not fail for valid API TOML")
        .files
}

/// Find the first generated file whose path ends with `suffix`.
fn find_file<'a>(
    files: &'a [polyplug_codegen::GeneratedFile],
    suffix: &str,
) -> &'a polyplug_codegen::GeneratedFile {
    files
        .iter()
        .find(|f: &&polyplug_codegen::GeneratedFile| f.path.to_string_lossy().ends_with(suffix))
        .unwrap_or_else(|| panic!("generated file ending with '{suffix}' not found"))
}

// ─── 1. Deno / V8 BigInt mapping ──────────────────────────────────────────────

/// `u64` fields and parameters must map to `bigint` in Deno TypeScript output.
#[test]
fn deno_u64_field_maps_to_bigint() {
    let files: Vec<polyplug_codegen::GeneratedFile> =
        run_generate(U64_I64_API_TOML, Lang::JsDeno, Side::Guest);
    let types_ts: &polyplug_codegen::GeneratedFile = find_file(&files, "types.ts");
    let content: &str = &types_ts.content;

    // The struct `TimestampPair` contains a u64 field `start_ns`.
    // In Deno TypeScript the field must carry `bigint`, not `number`.
    assert!(
        content.contains("readonly start_ns: bigint"),
        "u64 field 'start_ns' must be 'bigint' in Deno TS output, got:\n{content}"
    );
}

/// `i64` fields and parameters must map to `bigint` in Deno TypeScript output.
#[test]
fn deno_i64_field_maps_to_bigint() {
    let files: Vec<polyplug_codegen::GeneratedFile> =
        run_generate(U64_I64_API_TOML, Lang::JsDeno, Side::Guest);
    let types_ts: &polyplug_codegen::GeneratedFile = find_file(&files, "types.ts");
    let content: &str = &types_ts.content;

    // `end_ns` is declared as `i64` in the TOML.
    assert!(
        content.contains("readonly end_ns: bigint"),
        "i64 field 'end_ns' must be 'bigint' in Deno TS output, got:\n{content}"
    );
}

/// `u64` function parameters must map to `bigint` in Deno contract output.
#[test]
fn deno_u64_param_maps_to_bigint() {
    let files: Vec<polyplug_codegen::GeneratedFile> =
        run_generate(U64_I64_API_TOML, Lang::JsDeno, Side::Guest);
    let contracts_ts: &polyplug_codegen::GeneratedFile = find_file(&files, "contracts.ts");
    let content: &str = &contracts_ts.content;

    // `elapsed(start: bigint)` — the u64 param must be typed as `bigint`.
    assert!(
        content.contains("start: bigint"),
        "u64 param 'start' must be 'bigint' in Deno contracts.ts, got:\n{content}"
    );
}

/// `i64` function return values must map to `bigint` in Deno contract output.
#[test]
fn deno_i64_return_maps_to_bigint() {
    let files: Vec<polyplug_codegen::GeneratedFile> =
        run_generate(U64_I64_API_TOML, Lang::JsDeno, Side::Guest);
    let contracts_ts: &polyplug_codegen::GeneratedFile = find_file(&files, "contracts.ts");
    let content: &str = &contracts_ts.content;

    // `abstract elapsed(...): bigint` — i64 return must be typed as `bigint`.
    assert!(
        content.contains("): bigint"),
        "i64 return of 'elapsed' must be 'bigint' in Deno contracts.ts, got:\n{content}"
    );
}

/// `u64` must NOT map to `number` in Deno TypeScript (it maps to `bigint`).
/// This is the key correctness guard: wrong mapping would cause silent data loss
/// because JavaScript `number` is f64 and cannot represent all u64 values.
#[test]
fn deno_u64_never_maps_to_number() {
    let files: Vec<polyplug_codegen::GeneratedFile> =
        run_generate(U64_I64_API_TOML, Lang::JsDeno, Side::Guest);
    // Check all generated files — no u64/i64 field should appear typed as `number`.
    for file in &files {
        let content: &str = &file.content;
        // Neither `start_ns: number` nor `end_ns: number` should appear.
        assert!(
            !content.contains("start_ns: number"),
            "u64 field 'start_ns' must NOT be 'number' in Deno output (file: {:?})",
            file.path
        );
        assert!(
            !content.contains("end_ns: number"),
            "i64 field 'end_ns' must NOT be 'number' in Deno output (file: {:?})",
            file.path
        );
    }
}

// ─── 2. QuickJS lo/hi u32-pair mapping ────────────────────────────────────────

/// `u64` fields must map to `{ lo: number; hi: number }` in QuickJS TypeScript.
///
/// QuickJS uses the f64-based JavaScript number type internally, so it has no
/// native BigInt. 64-bit values are therefore split into two u32 halves.
#[test]
fn quickjs_u64_field_maps_to_lo_hi_pair() {
    let files: Vec<polyplug_codegen::GeneratedFile> =
        run_generate(U64_I64_API_TOML, Lang::JsQuickJs, Side::Guest);
    let types_ts: &polyplug_codegen::GeneratedFile = find_file(&files, "types.ts");
    let content: &str = &types_ts.content;

    // `start_ns` is u64 — must appear as `{ lo: number; hi: number }`.
    assert!(
        content.contains("readonly start_ns: { lo: number; hi: number }"),
        "u64 field 'start_ns' must be '{{ lo: number; hi: number }}' in QuickJS TS output, got:\n{content}"
    );
}

/// `i64` fields must map to `{ lo: number; hi: number }` in QuickJS TypeScript.
#[test]
fn quickjs_i64_field_maps_to_lo_hi_pair() {
    let files: Vec<polyplug_codegen::GeneratedFile> =
        run_generate(U64_I64_API_TOML, Lang::JsQuickJs, Side::Guest);
    let types_ts: &polyplug_codegen::GeneratedFile = find_file(&files, "types.ts");
    let content: &str = &types_ts.content;

    // `end_ns` is i64 — must appear as `{ lo: number; hi: number }`.
    assert!(
        content.contains("readonly end_ns: { lo: number; hi: number }"),
        "i64 field 'end_ns' must be '{{ lo: number; hi: number }}' in QuickJS TS output, got:\n{content}"
    );
}

/// `u64` parameters must map to `{ lo: number; hi: number }` in QuickJS contracts.
#[test]
fn quickjs_u64_param_maps_to_lo_hi_pair() {
    let files: Vec<polyplug_codegen::GeneratedFile> =
        run_generate(U64_I64_API_TOML, Lang::JsQuickJs, Side::Guest);
    let contracts_ts: &polyplug_codegen::GeneratedFile = find_file(&files, "contracts.ts");
    let content: &str = &contracts_ts.content;

    // The `elapsed` function takes a u64 `start` param — must use lo/hi.
    assert!(
        content.contains("start: { lo: number; hi: number }"),
        "u64 param 'start' must be '{{ lo: number; hi: number }}' in QuickJS contracts.ts, got:\n{content}"
    );
}

/// `u64` must NOT become `bigint` in QuickJS (QuickJS has no native BigInt).
#[test]
fn quickjs_u64_never_maps_to_bigint() {
    let files: Vec<polyplug_codegen::GeneratedFile> =
        run_generate(U64_I64_API_TOML, Lang::JsQuickJs, Side::Guest);
    for file in &files {
        let content: &str = &file.content;
        assert!(
            !content.contains("start_ns: bigint"),
            "u64 field must NOT be 'bigint' in QuickJS output (file: {:?}), got:\n{content}",
            file.path
        );
        assert!(
            !content.contains("end_ns: bigint"),
            "i64 field must NOT be 'bigint' in QuickJS output (file: {:?}), got:\n{content}",
            file.path
        );
    }
}

// ─── 3. C++ struct emitter — alignment for SIMD-typed fields ──────────────────

/// C++ struct emitter maps `u64` fields to `uint64_t` (the correct ABI type).
///
/// The generator does NOT insert `alignas` specifiers — structs crossing the
/// ABI boundary use natural platform alignment only. This test pins that
/// current, intentional behaviour: no `alignas` leaks into the output.
#[test]
fn cpp_u64_field_maps_to_uint64_t() {
    let files: Vec<polyplug_codegen::GeneratedFile> =
        run_generate(U64_I64_API_TOML, Lang::Cpp, Side::Guest);
    let types_hpp: &polyplug_codegen::GeneratedFile = find_file(&files, "types.hpp");
    let content: &str = &types_hpp.content;

    // Struct `TimestampPair` must declare `uint64_t start_ns`.
    assert!(
        content.contains("uint64_t start_ns"),
        "u64 field 'start_ns' must map to 'uint64_t' in C++ types.hpp, got:\n{content}"
    );
}

/// C++ struct emitter maps `i64` fields to `int64_t`.
#[test]
fn cpp_i64_field_maps_to_int64_t() {
    let files: Vec<polyplug_codegen::GeneratedFile> =
        run_generate(U64_I64_API_TOML, Lang::Cpp, Side::Guest);
    let types_hpp: &polyplug_codegen::GeneratedFile = find_file(&files, "types.hpp");
    let content: &str = &types_hpp.content;

    // `end_ns` is i64 — must be `int64_t`.
    assert!(
        content.contains("int64_t end_ns"),
        "i64 field 'end_ns' must map to 'int64_t' in C++ types.hpp, got:\n{content}"
    );
}

/// C++ struct emitter must NOT emit `alignas` specifiers for ABI structs.
///
/// Alignment specifiers on ABI structs would break cross-compiler portability.
/// SIMD alignment requirements must be handled by the caller, not codegen.
#[test]
fn cpp_struct_has_no_alignas_specifier() {
    let files: Vec<polyplug_codegen::GeneratedFile> =
        run_generate(U64_I64_API_TOML, Lang::Cpp, Side::Guest);
    let types_hpp: &polyplug_codegen::GeneratedFile = find_file(&files, "types.hpp");
    let content: &str = &types_hpp.content;

    assert!(
        !content.contains("alignas"),
        "C++ types.hpp must NOT contain 'alignas' specifiers for ABI structs, got:\n{content}"
    );
    assert!(
        !content.contains("__attribute__((aligned"),
        "C++ types.hpp must NOT contain '__attribute__((aligned' for ABI structs, got:\n{content}"
    );
}

/// C++ enum emitter maps a `u64`-repr enum to `uint64_t` underlying type.
#[test]
fn cpp_enum_u64_repr_maps_to_uint64_t() {
    const U64_ENUM_TOML: &str = r#"
[[enum]]
name = "EventKind"
repr = "u64"

[[enum.variants]]
name = "None"
value = "0"

[[enum.variants]]
name = "Tick"
value = "1"

[[contract]]
name = "bench.events"
version = "1.0.0"

[[contract.functions]]
name = "kind"
return = "EventKind"
"#;

    let files: Vec<polyplug_codegen::GeneratedFile> =
        run_generate(U64_ENUM_TOML, Lang::Cpp, Side::Guest);
    let types_hpp: &polyplug_codegen::GeneratedFile = find_file(&files, "types.hpp");
    let content: &str = &types_hpp.content;

    assert!(
        content.contains("enum class EventKind : uint64_t"),
        "u64-repr enum must use 'uint64_t' underlying type in C++, got:\n{content}"
    );
}

// ─── 4. C# StructLayout attributes ───────────────────────────────────────────

/// C# user-type structs must carry `StructLayout(LayoutKind.Sequential)`.
///
/// Sequential layout guarantees field order is preserved for ABI struct passing.
/// This is mandatory for interop with the native polyplug host runtime.
#[test]
fn csharp_user_struct_has_sequential_layout() {
    let files: Vec<polyplug_codegen::GeneratedFile> =
        run_generate(U64_I64_API_TOML, Lang::CSharp, Side::Guest);
    let types_cs: &polyplug_codegen::GeneratedFile = find_file(&files, "Types.cs");
    let content: &str = &types_cs.content;

    // The TimestampPair struct must carry the Sequential StructLayout attribute.
    assert!(
        content.contains("StructLayout(System.Runtime.InteropServices.LayoutKind.Sequential)"),
        "C# user struct must carry StructLayout(Sequential), got:\n{content}"
    );
}

/// C# user-type structs must NOT use `LayoutKind.Explicit`.
///
/// Explicit layout would require `[FieldOffset(...)]` annotations on every
/// field, which polyplugc does not emit. Using Explicit without offsets causes
/// a `TypeLoadException` at runtime.
#[test]
fn csharp_user_struct_never_explicit_layout() {
    let files: Vec<polyplug_codegen::GeneratedFile> =
        run_generate(U64_I64_API_TOML, Lang::CSharp, Side::Guest);
    let types_cs: &polyplug_codegen::GeneratedFile = find_file(&files, "Types.cs");
    let content: &str = &types_cs.content;

    assert!(
        !content.contains("LayoutKind.Explicit"),
        "C# user struct must NOT use LayoutKind.Explicit, got:\n{content}"
    );
}

/// C# `u64` fields must map to `ulong` (not `bigint` or any other type).
#[test]
fn csharp_u64_field_maps_to_ulong() {
    let files: Vec<polyplug_codegen::GeneratedFile> =
        run_generate(U64_I64_API_TOML, Lang::CSharp, Side::Guest);
    let types_cs: &polyplug_codegen::GeneratedFile = find_file(&files, "Types.cs");
    let content: &str = &types_cs.content;

    assert!(
        content.contains("public ulong start_ns"),
        "u64 field 'start_ns' must map to 'ulong' in C#, got:\n{content}"
    );
}

/// C# `i64` fields must map to `long`.
#[test]
fn csharp_i64_field_maps_to_long() {
    let files: Vec<polyplug_codegen::GeneratedFile> =
        run_generate(U64_I64_API_TOML, Lang::CSharp, Side::Guest);
    let types_cs: &polyplug_codegen::GeneratedFile = find_file(&files, "Types.cs");
    let content: &str = &types_cs.content;

    assert!(
        content.contains("public long end_ns"),
        "i64 field 'end_ns' must map to 'long' in C#, got:\n{content}"
    );
}

/// C# arg-pack structs (for 2+ param functions) must also carry Sequential layout.
#[test]
fn csharp_arg_pack_struct_has_sequential_layout() {
    const TWO_PARAM_TOML: &str = r#"
[[contract]]
name = "math.ops"
version = "1.0.0"

[[contract.functions]]
name = "add_longs"
params = [
  { name = "a", type = "u64" },
  { name = "b", type = "i64" }
]
return = "u64"
"#;

    let files: Vec<polyplug_codegen::GeneratedFile> =
        run_generate(TWO_PARAM_TOML, Lang::CSharp, Side::Guest);
    let types_cs: &polyplug_codegen::GeneratedFile = find_file(&files, "Types.cs");
    let content: &str = &types_cs.content;

    // The generated arg-pack struct for `add_longs(u64, i64)` must be Sequential.
    assert!(
        content.contains("StructLayout(System.Runtime.InteropServices.LayoutKind.Sequential)"),
        "C# arg-pack struct must carry StructLayout(Sequential), got:\n{content}"
    );
    // The u64 param must be `ulong`.
    assert!(
        content.contains("public ulong A"),
        "u64 arg-pack field 'A' must be 'ulong' in C#, got:\n{content}"
    );
    // The i64 param must be `long`.
    assert!(
        content.contains("public long B"),
        "i64 arg-pack field 'B' must be 'long' in C#, got:\n{content}"
    );
}
