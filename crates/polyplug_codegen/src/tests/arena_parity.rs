//! Call-arena parity tests for the C# and Python *host* caller generators.
//!
//! These assert at the string level that the generated host callers:
//!   1. Emit a per-caller call arena (backing buffer + `CallArena`) and reset it
//!      at the start of every arena-backed call, for contracts whose functions
//!      return a variable-size value (`StringView`, `Buffer`, user-defined).
//!   2. Hand that arena to the VM dispatch call for arena-backed functions, and
//!      pass a null arena for scalar-only functions (the host->alloc fallback).
//!   3. Omit the arena machinery entirely for scalar-only contracts.
//!
//! This mirrors the reference Rust host caller (`rust.rs`) and the C++ caller
//! (`cpp.rs`), keeping every generator's ABI mechanism identical (CLAUDE.md
//! Rule 10).

#![allow(clippy::expect_used)]

use crate::generate;
use crate::ir::{
    AbiBuiltin, PrimitiveType, ResolvedContract, ResolvedFunction, ResolvedParam, ResolvedTypeRef,
    ValidatedIr, Version,
};
use crate::{GenerateConfig, GenerateOutput, GeneratedFile, Lang, Side};
use polyplug_utils::guest_contract_id as fnv_contract_id;
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::PathBuf;

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn make_ir(contract_name: &str, functions: Vec<ResolvedFunction>) -> ValidatedIr {
    let contract_id: u64 = fnv_contract_id(contract_name, 1);
    ValidatedIr {
        types: vec![],
        enums: vec![],
        contracts: vec![ResolvedContract {
            name: contract_name.to_owned(),
            contract_id,
            version: Version {
                major: 1,
                minor: 0,
                patch: 0,
            },
            functions,
            docs: None,
        }],
        host_contracts: vec![],
        bundle: None,
    }
}

fn make_fn(
    name: &str,
    function_id: u32,
    params: Vec<ResolvedParam>,
    returns: Option<ResolvedTypeRef>,
) -> ResolvedFunction {
    ResolvedFunction {
        name: name.to_owned(),
        function_id,
        params,
        returns,
        docs: None,
        return_docs: None,
    }
}

fn sv_param(name: &str) -> ResolvedParam {
    ResolvedParam {
        name: name.to_owned(),
        ty: ResolvedTypeRef::AbiType(AbiBuiltin::StringView),
        docs: None,
    }
}

/// Generate host glue for `ir` in `lang`, returning the content of the generated
/// file whose name ends with `file_suffix`.
fn generate_host_file(ir: ValidatedIr, lang: Lang, test_tag: &str, file_suffix: &str) -> String {
    let tmp_dir: PathBuf = env::temp_dir().join("arena_parity").join(test_tag);
    fs::create_dir_all(&tmp_dir).expect("create tmp dir");

    let api_toml_content: String = ir_to_api_toml(&ir);
    let api_path: PathBuf = tmp_dir.join("api.toml");
    fs::write(&api_path, &api_toml_content).expect("write api.toml");

    let config: GenerateConfig = GenerateConfig {
        api_toml: api_path,
        lang,
        side: Side::Host,
        out_dir: tmp_dir.join("out"),
    };
    let output: GenerateOutput = generate(config).expect("generate");

    output
        .files
        .into_iter()
        .find(|f: &GeneratedFile| {
            f.path
                .file_name()
                .map(|n: &OsStr| n == file_suffix)
                .unwrap_or(false)
        })
        .unwrap_or_else(|| panic!("{file_suffix} must be generated"))
        .content
}

fn ir_to_api_toml(ir: &ValidatedIr) -> String {
    let mut out: String = String::new();
    for contract in &ir.contracts {
        out.push_str(&format!(
            "[[contract]]\nname = \"{}\"\nversion = \"{}.{}.{}\"\n\n",
            contract.name, contract.version.major, contract.version.minor, contract.version.patch,
        ));
        for func in &contract.functions {
            out.push_str("[[contract.functions]]\n");
            out.push_str(&format!("name = \"{}\"\n", func.name));
            if !func.params.is_empty() {
                let params_inline: String = func
                    .params
                    .iter()
                    .map(|p: &ResolvedParam| {
                        format!(
                            "{{ name = \"{}\", type = \"{}\" }}",
                            p.name,
                            resolved_type_to_str(&p.ty)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                out.push_str(&format!("params = [{params_inline}]\n"));
            }
            if let Some(ret) = &func.returns {
                out.push_str(&format!("return = \"{}\"\n", resolved_type_to_str(ret)));
            }
            out.push('\n');
        }
    }
    out
}

fn resolved_type_to_str(ty: &ResolvedTypeRef) -> &'static str {
    match ty {
        ResolvedTypeRef::Primitive(PrimitiveType::U32) => "u32",
        ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => "StringView",
        ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => "Buffer",
        _ => panic!("resolved_type_to_str: type not used by arena_parity tests"),
    }
}

/// IR with one StringView-returning function (arena-backed) and one scalar
/// function (no arena). Shared by the C# and Python assertions.
fn mixed_arena_ir(contract_name: &str) -> ValidatedIr {
    make_ir(
        contract_name,
        vec![
            // function 0: StringView echo(StringView) — arena-backed.
            make_fn(
                "echo",
                0,
                vec![sv_param("input")],
                Some(ResolvedTypeRef::AbiType(AbiBuiltin::StringView)),
            ),
            // function 1: u32 add(u32, u32) — scalar, no arena.
            make_fn(
                "add",
                1,
                vec![
                    ResolvedParam {
                        name: "a".to_owned(),
                        ty: ResolvedTypeRef::Primitive(PrimitiveType::U32),
                        docs: None,
                    },
                    ResolvedParam {
                        name: "b".to_owned(),
                        ty: ResolvedTypeRef::Primitive(PrimitiveType::U32),
                        docs: None,
                    },
                ],
                Some(ResolvedTypeRef::Primitive(PrimitiveType::U32)),
            ),
        ],
    )
}

/// IR with only scalar functions — no contract needs an arena.
fn scalar_only_ir(contract_name: &str) -> ValidatedIr {
    make_ir(
        contract_name,
        vec![make_fn(
            "add",
            0,
            vec![
                ResolvedParam {
                    name: "a".to_owned(),
                    ty: ResolvedTypeRef::Primitive(PrimitiveType::U32),
                    docs: None,
                },
                ResolvedParam {
                    name: "b".to_owned(),
                    ty: ResolvedTypeRef::Primitive(PrimitiveType::U32),
                    docs: None,
                },
            ],
            Some(ResolvedTypeRef::Primitive(PrimitiveType::U32)),
        )],
    )
}

// ─── C# host caller assertions ─────────────────────────────────────────────────

#[test]
fn csharp_host_caller_emits_arena_for_stringview_return() {
    let ir: ValidatedIr = mixed_arena_ir("arena.cs");
    let callers: String = generate_host_file(ir, Lang::CSharp, "cs_mixed", "Callers.cs");

    // Inline arena helpers + per-caller buffer/field are emitted.
    assert!(
        callers.contains("internal static unsafe class CallArenaOps"),
        "C#: arena helpers must be emitted:\n{callers}"
    );
    assert!(
        callers.contains("private readonly byte* _arenaBuf;")
            && callers.contains("private CallArena _arena;"),
        "C#: per-caller arena fields must be emitted:\n{callers}"
    );
    assert!(
        callers.contains("NativeMemory.Alloc(CallArenaOps.CALL_ARENA_BUF_LEN)"),
        "C#: arena buffer must be C-heap (NativeMemory), not the managed heap:\n{callers}"
    );

    // Both dispatch branches exist (VM parity with rust.rs/cpp.rs).
    assert!(
        callers.contains("case DispatchType.Native:")
            && callers.contains("case DispatchType.VirtualMachine:"),
        "C#: host caller must branch on dispatch type:\n{callers}"
    );

    // The arena-backed (echo, fn 0) call rewinds the arena and passes a live arena
    // pointer to the VM dispatch; the scalar (add, fn 1) call passes a null arena.
    assert!(
        callers.contains("CallArenaOps.Reset(arenaResetPtr)"),
        "C#: arena-backed method must rewind the arena at call start:\n{callers}"
    );
    assert!(
        callers.contains("argsPtr, outPtr, arenaPtr, &err)"),
        "C#: arena-backed VM call must pass the live arena pointer then the AbiError out-param:\n{callers}"
    );
    assert!(
        callers.contains("argsPtr, outPtr, (CallArena*)null, &err)"),
        "C#: scalar VM call must pass a null arena (host->alloc fallback) then the AbiError out-param:\n{callers}"
    );

    // Dispose must use FreeAll (frees overflow chain) not Reset (rewind only).
    assert!(
        callers.contains("CallArenaOps.FreeAll(arenaPtr)"),
        "C#: Dispose must call FreeAll to release retained overflow blocks:\n{callers}"
    );
    // Reset is the rewind-only path; Dispose must never call it (would leak overflow blocks).
    assert!(
        !callers.contains("CallArenaOps.Reset(arenaPtr)"),
        "C#: Dispose must NOT call Reset (would leave overflow blocks unreleased):\n{callers}"
    );
}

#[test]
fn csharp_host_caller_omits_arena_for_scalar_only() {
    let ir: ValidatedIr = scalar_only_ir("scalar.cs");
    let callers: String = generate_host_file(ir, Lang::CSharp, "cs_scalar", "Callers.cs");

    assert!(
        !callers.contains("CallArenaOps"),
        "C#: scalar-only contract must NOT emit arena helpers:\n{callers}"
    );
    assert!(
        !callers.contains("_arenaBuf"),
        "C#: scalar-only contract must NOT emit an arena buffer:\n{callers}"
    );
    // VM dispatch still exists and uses a null arena.
    assert!(
        callers.contains("argsPtr, outPtr, (CallArena*)null, &err)"),
        "C#: scalar-only VM call must still pass a null arena then the AbiError out-param:\n{callers}"
    );
}

// ─── Python host caller assertions ──────────────────────────────────────────────

#[test]
fn python_host_caller_emits_arena_for_stringview_return() {
    let ir: ValidatedIr = mixed_arena_ir("arena.py");
    let callers: String = generate_host_file(ir, Lang::Python, "py_mixed", "callers.py");

    // Arena import + reset helper + per-caller buffer are emitted.
    assert!(
        callers.contains("from polyplug_abi import")
            && callers.contains("ArenaOverflowBlock")
            && callers.contains("CallArena"),
        "Python: CallArena/ArenaOverflowBlock must be imported:\n{callers}"
    );
    assert!(
        callers.contains("def _arena_reset("),
        "Python: arena reset helper must be emitted:\n{callers}"
    );
    assert!(
        callers.contains("ctypes.create_string_buffer(CALL_ARENA_BUF_LEN)"),
        "Python: arena buffer must be C-heap (create_string_buffer):\n{callers}"
    );

    // Arena-backed (echo) rewinds (1-arg) at call start; scalar (add) passes None.
    assert!(
        callers.contains("_arena_reset(self._arena)"),
        "Python: arena-backed method must rewind the arena (1-arg) at call start:\n{callers}"
    );
    assert!(
        callers.contains("def _arena_free_all("),
        "Python: _arena_free_all teardown helper must be emitted:\n{callers}"
    );
    assert!(
        callers.contains("_arena_free_all(self._arena, self._host)"),
        "Python: __del__ must call _arena_free_all (teardown frees blocks):\n{callers}"
    );
    assert!(
        callers.contains("out_ptr, ctypes.byref(self._arena), ctypes.byref(err))"),
        "Python: arena-backed VM call must pass the live arena then the AbiError out-param:\n{callers}"
    );
    assert!(
        callers.contains("out_ptr, None, ctypes.byref(err))"),
        "Python: scalar VM call must pass a null arena (host->alloc fallback) then the AbiError out-param:\n{callers}"
    );
}

#[test]
fn python_host_caller_omits_arena_for_scalar_only() {
    let ir: ValidatedIr = scalar_only_ir("scalar.py");
    let callers: String = generate_host_file(ir, Lang::Python, "py_scalar", "callers.py");

    assert!(
        !callers.contains("def _arena_reset("),
        "Python: scalar-only contract must NOT emit the arena reset helper:\n{callers}"
    );
    assert!(
        !callers.contains("def _arena_free_all("),
        "Python: scalar-only contract must NOT emit the arena free_all helper:\n{callers}"
    );
    assert!(
        !callers.contains("create_string_buffer"),
        "Python: scalar-only contract must NOT allocate an arena buffer:\n{callers}"
    );
    assert!(
        callers.contains("out_ptr, None, ctypes.byref(err))"),
        "Python: scalar-only VM call must pass a null arena then the AbiError out-param:\n{callers}"
    );
}
