//! Generator correctness tests.
//!
//! Verifies that the Rust code generator produces output that:
//!   1. Assigns vtable slot indices that are sequential and correct (0, 1, 2, …).
//!   2. Emits function signatures (param names, types, return type) that exactly
//!      match the contract declared in the IR.
//!   3. Emits a vtable FNS entry for every function declared in a contract
//!      (missing-function detection).
//!
//! All tests operate at the IR + generator level — no compilation or dynamic
//! loading is required.

#![allow(clippy::expect_used)]

use polyplug::abi::contract_id as fnv_contract_id;
use polyplug_codegen::ir::PrimitiveType;
use polyplug_codegen::ir::ResolvedContract;
use polyplug_codegen::ir::ResolvedFunction;
use polyplug_codegen::ir::ResolvedParam;
use polyplug_codegen::ir::ResolvedTypeRef;
use polyplug_codegen::ir::ValidatedIr;
use polyplug_codegen::ir::Version;
use polyplug_codegen::{GenerateConfig, GenerateOutput, GeneratedFile, Lang, Side};
use std::path::PathBuf;

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Build a minimal [`ValidatedIr`] with a single contract containing the
/// provided functions.  `function_id` values mirror declaration order.
fn make_ir(contract_name: &str, major: u32, functions: Vec<ResolvedFunction>) -> ValidatedIr {
    let contract_id: u64 = fnv_contract_id(contract_name, major);
    ValidatedIr {
        types: vec![],
        enums: vec![],
        contracts: vec![ResolvedContract {
            name: contract_name.to_owned(),
            contract_id,
            version: Version {
                major,
                minor: 0,
                patch: 0,
            },
            functions,
        }],
        bundle: None,
    }
}

/// Build a [`ResolvedFunction`] with the given name, slot index, params and
/// optional return type.
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
    }
}

/// Build a [`ResolvedParam`] with a primitive type.
fn prim_param(name: &str, prim: PrimitiveType) -> ResolvedParam {
    ResolvedParam {
        name: name.to_owned(),
        ty: ResolvedTypeRef::Primitive(prim),
    }
}

/// Run the Rust guest generator on `ir` using a unique `test_tag` directory
/// and return the content of `guest/vtables.rs`.
fn generate_guest_vtables(ir: ValidatedIr, test_tag: &str) -> String {
    run_guest_generator(ir, test_tag, "vtables.rs")
}

/// Run the Rust guest generator on `ir` using a unique `test_tag` directory
/// and return the content of `guest/contracts.rs`.
fn generate_guest_contracts(ir: ValidatedIr, test_tag: &str) -> String {
    run_guest_generator(ir, test_tag, "contracts.rs")
}

/// Core generator driver: writes a temp api.toml, runs generate(), and returns
/// the content of the generated file whose name ends with `file_suffix`.
fn run_guest_generator(ir: ValidatedIr, test_tag: &str, file_suffix: &str) -> String {
    // Each test gets its own sub-directory to avoid races between parallel tests.
    let tmp_dir: PathBuf = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("gen_correctness")
        .join(test_tag);
    std::fs::create_dir_all(&tmp_dir).expect("create tmp dir");

    // Serialise IR to TOML and write it next to the output directory.
    let api_toml_content: String = ir_to_api_toml(&ir);
    let api_path: PathBuf = tmp_dir.join("api.toml");
    std::fs::write(&api_path, &api_toml_content).expect("write api.toml");

    let config: GenerateConfig = GenerateConfig {
        api_toml: api_path,
        lang: Lang::Rust,
        side: Side::Guest,
        out_dir: tmp_dir.join("out"),
    };
    let output: GenerateOutput = polyplug_codegen::generate(config).expect("generate");

    output
        .files
        .into_iter()
        .find(|f: &GeneratedFile| {
            f.path
                .file_name()
                .map(|n: &std::ffi::OsStr| n == file_suffix)
                .unwrap_or(false)
        })
        .unwrap_or_else(|| panic!("{file_suffix} must be generated"))
        .content
}

/// Serialise a [`ValidatedIr`] to a valid `api.toml` string.
///
/// Uses the standard inline-table array format understood by the polyplug
/// parser.  Trailing commas are omitted — TOML does not permit them in arrays.
fn ir_to_api_toml(ir: &ValidatedIr) -> String {
    let mut out: String = String::new();

    for ty in &ir.types {
        let fields_inline: String = ty
            .fields
            .iter()
            .map(|f: &polyplug_codegen::ResolvedField| {
                format!(
                    "{{ name = \"{}\", type = \"{}\" }}",
                    f.name,
                    resolved_type_to_str(&f.ty)
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!(
            "[[types]]\nname = \"{}\"\nfields = [{fields_inline}]\n\n",
            ty.name
        ));
    }

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

/// Convert a [`ResolvedTypeRef`] back to the string token used in api.toml.
fn resolved_type_to_str(ty: &ResolvedTypeRef) -> &'static str {
    match ty {
        ResolvedTypeRef::Primitive(PrimitiveType::U8) => "u8",
        ResolvedTypeRef::Primitive(PrimitiveType::U16) => "u16",
        ResolvedTypeRef::Primitive(PrimitiveType::U32) => "u32",
        ResolvedTypeRef::Primitive(PrimitiveType::U64) => "u64",
        ResolvedTypeRef::Primitive(PrimitiveType::I8) => "i8",
        ResolvedTypeRef::Primitive(PrimitiveType::I16) => "i16",
        ResolvedTypeRef::Primitive(PrimitiveType::I32) => "i32",
        ResolvedTypeRef::Primitive(PrimitiveType::I64) => "i64",
        ResolvedTypeRef::Primitive(PrimitiveType::F32) => "f32",
        ResolvedTypeRef::Primitive(PrimitiveType::F64) => "f64",
        ResolvedTypeRef::Primitive(PrimitiveType::Bool) => "bool",
        ResolvedTypeRef::AbiType(polyplug_codegen::ir::AbiBuiltin::StringView) => "StringView",
        ResolvedTypeRef::AbiType(polyplug_codegen::ir::AbiBuiltin::Buffer) => "Buffer",
        ResolvedTypeRef::AbiType(polyplug_codegen::ir::AbiBuiltin::Ptr) => "ptr",
        ResolvedTypeRef::AbiType(polyplug_codegen::ir::AbiBuiltin::Void) => "void",
        ResolvedTypeRef::UserDefined(_) => {
            // UserDefined names are dynamic; no test uses them here.
            panic!("resolved_type_to_str: UserDefined not supported in ir_to_api_toml")
        }
    }
}

// ─── Test 1: VTable slot indices are sequential and correct ──────────────────

/// Verifies that every function in a contract is assigned a `function_id` that
/// matches its declaration order (0-based) and that the generated vtable FNS
/// array lists them in exactly that order.
///
/// Concretely: for a contract with functions `[alpha, beta, gamma]`, the
/// generator must emit:
/// ```text
/// static UPPER_FNS: [FnPtr; 3_usize] = [
///     FnPtr(lower_alpha_abi as *const ()),
///     FnPtr(lower_beta_abi as *const ()),
///     FnPtr(lower_gamma_abi as *const ()),
/// ];
/// ```
#[test]
fn vtable_slots_are_sequential() {
    let fns: Vec<ResolvedFunction> = vec![
        make_fn("alpha", 0, vec![], None),
        make_fn("beta", 1, vec![], None),
        make_fn("gamma", 2, vec![], None),
    ];
    let ir: ValidatedIr = make_ir("slot.check", 1, fns);
    let vtables: String = generate_guest_vtables(ir, "slots_sequential");

    // The FNS array must have exactly 3 entries.
    assert!(
        vtables.contains("[FnPtr; 3_usize]"),
        "expected FNS array of size 3:\n{vtables}"
    );

    // The entries must appear in declaration order.
    let pos_alpha: usize = vtables
        .find("slot_check_alpha_abi")
        .expect("alpha abi wrapper must appear in FNS");
    let pos_beta: usize = vtables
        .find("slot_check_beta_abi")
        .expect("beta abi wrapper must appear in FNS");
    let pos_gamma: usize = vtables
        .find("slot_check_gamma_abi")
        .expect("gamma abi wrapper must appear in FNS");

    assert!(
        pos_alpha < pos_beta,
        "alpha (slot 0) must appear before beta (slot 1) in FNS array"
    );
    assert!(
        pos_beta < pos_gamma,
        "beta (slot 1) must appear before gamma (slot 2) in FNS array"
    );
}

/// Verifies that the `function_count` field emitted in the PluginVTable
/// initialiser matches the number of functions declared in the contract.
#[test]
fn vtable_function_count_matches_contract() {
    let fns: Vec<ResolvedFunction> = vec![
        make_fn("op_a", 0, vec![], None),
        make_fn("op_b", 1, vec![prim_param("x", PrimitiveType::U32)], None),
        make_fn(
            "op_c",
            2,
            vec![],
            Some(ResolvedTypeRef::Primitive(PrimitiveType::U64)),
        ),
        make_fn(
            "op_d",
            3,
            vec![prim_param("v", PrimitiveType::F32)],
            Some(ResolvedTypeRef::Primitive(PrimitiveType::F32)),
        ),
    ];
    let expected_count: usize = fns.len();
    let ir: ValidatedIr = make_ir("count.check", 1, fns);
    let vtables: String = generate_guest_vtables(ir, "fn_count_matches");

    let expected_line: String = format!("function_count: {expected_count}_u32");
    assert!(
        vtables.contains(&expected_line),
        "expected `{expected_line}` in vtables.rs:\n{vtables}"
    );
}

/// Verifies that each ABI wrapper carries the correct `function_id` comment,
/// so the slot assignment visible in the source matches the declared index.
#[test]
fn vtable_wrapper_function_id_comments_match_slot() {
    let fns: Vec<ResolvedFunction> = vec![
        make_fn("first", 0, vec![], None),
        make_fn("second", 1, vec![], None),
        make_fn("third", 2, vec![], None),
    ];
    let ir: ValidatedIr = make_ir("id.comment", 1, fns);
    let vtables: String = generate_guest_vtables(ir, "wrapper_id_comments");

    // The generator emits: `/// ABI wrapper for <name> (function_id = <id>).`
    assert!(
        vtables.contains("function_id = 0"),
        "expected function_id = 0 comment for `first`:\n{vtables}"
    );
    assert!(
        vtables.contains("function_id = 1"),
        "expected function_id = 1 comment for `second`:\n{vtables}"
    );
    assert!(
        vtables.contains("function_id = 2"),
        "expected function_id = 2 comment for `third`:\n{vtables}"
    );
}

// ─── Test 2: Function signatures match the contract ──────────────────────────

/// Verifies that the guest trait method has a signature whose parameter names
/// and primitive types exactly match those declared in the IR contract.
#[test]
fn signature_primitive_params_match_contract() {
    let fns: Vec<ResolvedFunction> = vec![make_fn(
        "compute",
        0,
        vec![
            prim_param("width", PrimitiveType::U32),
            prim_param("height", PrimitiveType::U32),
        ],
        Some(ResolvedTypeRef::Primitive(PrimitiveType::U64)),
    )];
    let ir: ValidatedIr = make_ir("sig.check", 1, fns);
    let contracts: String = generate_guest_contracts(ir, "sig_primitive_params");

    // The trait method must have both params by their declared names and types.
    assert!(
        contracts.contains("width: u32"),
        "param `width: u32` must appear in trait method:\n{contracts}"
    );
    assert!(
        contracts.contains("height: u32"),
        "param `height: u32` must appear in trait method:\n{contracts}"
    );
    // Return type must match.
    assert!(
        contracts.contains("Result<u64, PluginError>"),
        "return type `Result<u64, PluginError>` must appear in trait method:\n{contracts}"
    );
}

/// Verifies that a no-param, no-return function generates a trait method with
/// the correct signature `fn name(&self) -> Result<(), PluginError>`.
#[test]
fn signature_void_function_matches_contract() {
    let fns: Vec<ResolvedFunction> = vec![make_fn("reset", 0, vec![], None)];
    let ir: ValidatedIr = make_ir("void.check", 1, fns);
    let contracts: String = generate_guest_contracts(ir, "sig_void_fn");

    assert!(
        contracts.contains("fn reset(&self)"),
        "trait must have `fn reset(&self)` method:\n{contracts}"
    );
    assert!(
        contracts.contains("Result<(), PluginError>"),
        "void return must be `Result<(), PluginError>`:\n{contracts}"
    );
}

/// Verifies that a function returning a StringView produces a trait method
/// whose return type is exactly `Result<StringView, PluginError>`.
#[test]
fn signature_stringview_return_matches_contract() {
    let fns: Vec<ResolvedFunction> = vec![make_fn(
        "get_name",
        0,
        vec![],
        Some(ResolvedTypeRef::AbiType(
            polyplug_codegen::ir::AbiBuiltin::StringView,
        )),
    )];
    let ir: ValidatedIr = make_ir("sv.check", 1, fns);
    let contracts: String = generate_guest_contracts(ir, "sig_stringview_return");

    assert!(
        contracts.contains("Result<StringView, PluginError>"),
        "StringView return must produce `Result<StringView, PluginError>`:\n{contracts}"
    );
}

/// Verifies that different primitive types (i64, f64, bool) each map to their
/// correct Rust equivalents in the emitted trait method.
#[test]
fn signature_various_primitive_types_match_contract() {
    let fns: Vec<ResolvedFunction> = vec![
        make_fn(
            "flag_op",
            0,
            vec![prim_param("enabled", PrimitiveType::Bool)],
            Some(ResolvedTypeRef::Primitive(PrimitiveType::Bool)),
        ),
        make_fn(
            "score",
            1,
            vec![prim_param("delta", PrimitiveType::F64)],
            Some(ResolvedTypeRef::Primitive(PrimitiveType::I64)),
        ),
    ];
    let ir: ValidatedIr = make_ir("prim.types", 1, fns);
    let contracts: String = generate_guest_contracts(ir, "sig_various_primitives");

    assert!(
        contracts.contains("enabled: bool"),
        "param `enabled: bool` must appear:\n{contracts}"
    );
    assert!(
        contracts.contains("Result<bool, PluginError>"),
        "`Result<bool, PluginError>` must appear:\n{contracts}"
    );
    assert!(
        contracts.contains("delta: f64"),
        "param `delta: f64` must appear:\n{contracts}"
    );
    assert!(
        contracts.contains("Result<i64, PluginError>"),
        "`Result<i64, PluginError>` must appear:\n{contracts}"
    );
}

// ─── Test 3: Missing function detection ──────────────────────────────────────

/// Verifies that every function declared in a contract has a corresponding ABI
/// wrapper symbol and an entry in the vtable FNS array.
///
/// If any function is omitted from the vtable the runtime cannot dispatch it —
/// this test catches that class of generator bug.
#[test]
fn all_contract_functions_appear_in_vtable() {
    let function_names: &[&str] = &["open", "read", "write", "close", "flush"];
    let fns: Vec<ResolvedFunction> = function_names
        .iter()
        .enumerate()
        .map(|(idx, name): (usize, &&str)| make_fn(name, idx as u32, vec![], None))
        .collect();

    let ir: ValidatedIr = make_ir("file.ops", 1, fns);
    let vtables: String = generate_guest_vtables(ir, "all_fns_in_vtable");

    for name in function_names {
        // Each function must have an ABI wrapper defined.
        let wrapper: String = format!("file_ops_{name}_abi");
        assert!(
            vtables.contains(&wrapper),
            "ABI wrapper `{wrapper}` missing from vtables.rs:\n{vtables}"
        );

        // Each function must appear in the FNS array.
        let fns_entry: String = format!("FnPtr(file_ops_{name}_abi as *const ())");
        assert!(
            vtables.contains(&fns_entry),
            "FNS entry `{fns_entry}` missing from vtables.rs:\n{vtables}"
        );
    }
}

/// Verifies that the FNS array size constant equals the number of declared
/// functions — catching the case where a function exists in the array but
/// the size constant is wrong (or vice-versa).
#[test]
fn fns_array_size_equals_declared_function_count() {
    for n in [1_usize, 3, 5, 8] {
        let fns: Vec<ResolvedFunction> = (0..n)
            .map(|i: usize| make_fn(&format!("fn_{i}"), i as u32, vec![], None))
            .collect();
        let contract_name: String = format!("array.sz.{n}");
        let test_tag: String = format!("fns_array_size_{n}");
        let ir: ValidatedIr = make_ir(&contract_name, 1, fns);
        let vtables: String = generate_guest_vtables(ir, &test_tag);

        let expected: String = format!("[FnPtr; {n}_usize]");
        assert!(
            vtables.contains(&expected),
            "for n={n}: expected `{expected}` in vtables.rs:\n{vtables}"
        );
    }
}

/// Verifies that a single-function contract generates exactly one FNS entry,
/// i.e. no duplicate or phantom entries are emitted.
#[test]
fn single_function_contract_has_exactly_one_fns_entry() {
    let fns: Vec<ResolvedFunction> = vec![make_fn("ping", 0, vec![], None)];
    let ir: ValidatedIr = make_ir("single.fn", 1, fns);
    let vtables: String = generate_guest_vtables(ir, "single_fn_one_fns_entry");

    // Count array entries specifically: lines that are indented FnPtr calls
    // inside the FNS array (e.g. `    FnPtr(single_fn_ping_abi as *const ())`),
    // not the struct definition line `pub struct FnPtr(pub *const ());`.
    let count: usize = vtables
        .lines()
        .filter(|line: &&str| {
            let trimmed: &str = line.trim_start();
            trimmed.starts_with("FnPtr(") && !trimmed.starts_with("FnPtr(pub")
        })
        .count();
    assert_eq!(
        count, 1,
        "expected exactly 1 FnPtr array entry for single-function contract, got {count}:\n{vtables}"
    );
}

/// Verifies that two distinct contracts each get their own complete FNS arrays
/// with no cross-contamination of function entries.
#[test]
fn multiple_contracts_have_independent_fns_arrays() {
    let contract_id_a: u64 = fnv_contract_id("multi.a", 1);
    let contract_id_b: u64 = fnv_contract_id("multi.b", 1);

    let ir: ValidatedIr = ValidatedIr {
        types: vec![],
        enums: vec![],
        contracts: vec![
            ResolvedContract {
                name: "multi.a".to_owned(),
                contract_id: contract_id_a,
                version: Version {
                    major: 1,
                    minor: 0,
                    patch: 0,
                },
                functions: vec![make_fn("foo", 0, vec![], None)],
            },
            ResolvedContract {
                name: "multi.b".to_owned(),
                contract_id: contract_id_b,
                version: Version {
                    major: 1,
                    minor: 0,
                    patch: 0,
                },
                functions: vec![make_fn("bar", 0, vec![], None)],
            },
        ],
        bundle: None,
    };
    let vtables: String = run_guest_generator(ir, "multi_contracts_independent", "vtables.rs");

    // Contract A's wrapper and FNS entry must be present.
    assert!(
        vtables.contains("multi_a_foo_abi"),
        "multi.a `foo` wrapper missing:\n{vtables}"
    );
    // Contract B's wrapper and FNS entry must be present.
    assert!(
        vtables.contains("multi_b_bar_abi"),
        "multi.b `bar` wrapper missing:\n{vtables}"
    );
    // Each contract must have its own independent FNS array (size 1 each).
    let fns_array_count: usize = vtables.matches("[FnPtr; 1_usize]").count();
    assert_eq!(
        fns_array_count, 2,
        "expected 2 separate `[FnPtr; 1_usize]` arrays (one per contract), \
         got {fns_array_count}:\n{vtables}"
    );
}
