//! js_quickjs generator — produces QuickJS-compatible TypeScript/JavaScript guest code.
//!
//! THIS FILE IS PART OF polyplugc.
//! Generates code using lo/hi u32 split for all u64/pointer values.

use std::path::PathBuf;

use crate::error::CodegenError;
use crate::generators::CodeGenerator;
use crate::generators::GeneratedFile;
use crate::generators::GeneratedFiles;
use crate::ir::AbiBuiltin;
use crate::ir::EnumDef;
use crate::ir::EnumVariant;
use crate::ir::PrimitiveType;
use crate::ir::ResolvedBundle;
use crate::ir::ResolvedContract;
use crate::ir::ResolvedDependency;
use crate::ir::ResolvedField;
use crate::ir::ResolvedFunction;
use crate::ir::ResolvedParam;
use crate::ir::ResolvedPlugin;
use crate::ir::ResolvedType;
use crate::ir::ResolvedTypeRef;
use crate::ir::ValidatedIr;

/// Generator for js-quickjs plugin bundles.
///
/// Produces TypeScript files using lo/hi u32 pairs for 64-bit values
/// (QuickJS uses f64 internally, so bigint is not available).
pub(crate) struct JsQuickjsGenerator;

impl CodeGenerator for JsQuickjsGenerator {
    fn language_name(&self) -> &'static str {
        "js-quickjs"
    }

    fn generate_host(
        &self,
        ir: &ValidatedIr,
        files: &mut GeneratedFiles,
    ) -> Result<(), CodegenError> {
        files.files.push(GeneratedFile {
            path: PathBuf::from("host/types.ts"),
            content: generate_types_ts(ir),
            force_regenerate: false,
        });
        files.files.push(GeneratedFile {
            path: PathBuf::from("host/callers.ts"),
            content: generate_callers_ts(ir),
            force_regenerate: false,
        });
        Ok(())
    }

    fn generate_guest(
        &self,
        ir: &ValidatedIr,
        files: &mut GeneratedFiles,
    ) -> Result<(), CodegenError> {
        files.files.push(GeneratedFile {
            path: PathBuf::from("guest/types.ts"),
            content: generate_types_ts(ir),
            force_regenerate: false,
        });
        files.files.push(GeneratedFile {
            path: PathBuf::from("guest/contracts.ts"),
            content: generate_contracts_ts(ir),
            force_regenerate: false,
        });
        files.files.push(GeneratedFile {
            path: PathBuf::from("guest/vtable.ts"),
            content: generate_vtable_ts(),
            force_regenerate: false,
        });
        files.files.push(GeneratedFile {
            path: PathBuf::from("guest/init.ts"),
            content: generate_init_ts(ir),
            force_regenerate: false,
        });
        if ir.bundle.is_some() {
            files.files.push(GeneratedFile {
                path: PathBuf::from("manifest.toml"),
                content: generate_manifest_toml(ir),
                force_regenerate: true,
            });
        }
        files.files.push(GeneratedFile {
            path: PathBuf::from("README.md"),
            content: generate_readme_quickjs(ir),
            force_regenerate: false,
        });
        Ok(())
    }
}

/// Map a resolved type reference to its TypeScript representation (QuickJS lo/hi style).
fn ts_type_ref(ty: &ResolvedTypeRef) -> String {
    match ty {
        ResolvedTypeRef::Primitive(p) => ts_primitive(p).to_owned(),
        ResolvedTypeRef::AbiType(b) => ts_abi_builtin(b).to_owned(),
        ResolvedTypeRef::UserDefined(name) => name.clone(),
    }
}

fn ts_primitive(p: &PrimitiveType) -> &'static str {
    match p {
        PrimitiveType::U8
        | PrimitiveType::U16
        | PrimitiveType::U32
        | PrimitiveType::I8
        | PrimitiveType::I16
        | PrimitiveType::I32
        | PrimitiveType::F32
        | PrimitiveType::F64 => "number",
        PrimitiveType::Bool => "boolean",
        PrimitiveType::U64 | PrimitiveType::I64 => "{ lo: number; hi: number }",
    }
}

fn ts_abi_builtin(b: &AbiBuiltin) -> &'static str {
    match b {
        AbiBuiltin::StringView => "{ ptr_lo: number; ptr_hi: number; len: number }",
        AbiBuiltin::Buffer => "{ ptr_lo: number; ptr_hi: number; len: number; cap: number }",
        AbiBuiltin::Ptr => "{ lo: number; hi: number }",
        AbiBuiltin::Void => "void",
    }
}

fn substitute_variant_refs_js(declared_variants: &[EnumVariant], expr: &str) -> String {
    let chars: Vec<char> = expr.chars().collect();
    let len: usize = chars.len();
    let mut result: String = String::new();
    let mut i: usize = 0;
    while i < len {
        let c: char = chars[i];
        if c.is_alphabetic() || c == '_' {
            let start: usize = i;
            while i < len && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let ident: String = chars[start..i].iter().collect();
            let found: Option<&EnumVariant> = declared_variants.iter().find(|v| v.name == ident);
            if let Some(ref_variant) = found {
                result.push('(');
                result.push_str(&ref_variant.value);
                result.push(')');
            } else {
                result.push_str(&ident);
            }
        } else {
            result.push(c);
            i += 1;
        }
    }
    result
}

fn generate_js_quickjs_enum(out: &mut String, e: &EnumDef) {
    if e.bitflag {
        out.push_str(&format!("/** @bitflag Enum {} */\n", e.name));
    } else {
        out.push_str(&format!("/** Enum {} */\n", e.name));
    }
    out.push_str(&format!("const {} = Object.freeze({{\n", e.name));
    for variant in &e.variants {
        let subst_value: String = substitute_variant_refs_js(&e.variants, &variant.value);
        out.push_str(&format!("    {}: {},\n", variant.name, subst_value));
    }
    out.push_str("} as const);\n");
    out.push_str(&format!(
        "type {} = typeof {}[keyof typeof {}];\n\n",
        e.name, e.name, e.name
    ));
}
fn generate_types_ts(ir: &ValidatedIr) -> String {
    let mut out: String = String::new();
    out.push_str(
        "// THIS FILE IS AUTO-GENERATED BY polyplugc\n\
         // DO NOT EDIT BY HAND\n\
         // Runtime: js-quickjs\n\n",
    );
    for e in &ir.enums {
        generate_js_quickjs_enum(&mut out, e);
    }
    for type_def in &ir.types {
        render_resolved_type(&mut out, type_def);
    }
    out
}

fn render_resolved_type(out: &mut String, type_def: &ResolvedType) {
    out.push_str(&format!("export interface {} {{\n", type_def.name));
    for field in &type_def.fields {
        render_resolved_field(out, field);
    }
    out.push_str("}\n\n");
}

fn render_resolved_field(out: &mut String, field: &ResolvedField) {
    let ts_t: String = ts_type_ref(&field.ty);
    out.push_str(&format!("    readonly {}: {};\n", field.name, ts_t));
}

fn generate_contracts_ts(ir: &ValidatedIr) -> String {
    let mut out: String = String::new();
    out.push_str(
        "// THIS FILE IS AUTO-GENERATED BY polyplugc\n\
         // DO NOT EDIT BY HAND\n\
         // Runtime: js-quickjs\n\n",
    );
    out.push_str("import type { } from './types';\n\n");
    for contract in &ir.contracts {
        render_contract_quickjs(&mut out, contract);
    }
    out
}

fn render_contract_quickjs(out: &mut String, contract: &ResolvedContract) {
    let class_name: String = contract_to_class_name(&contract.name);
    out.push_str(&format!("export abstract class {} {{\n", class_name));
    for func in &contract.functions {
        render_function_signature_quickjs(out, func);
    }
    out.push_str("}\n\n");
}

fn render_function_signature_quickjs(out: &mut String, func: &ResolvedFunction) {
    let params: String = func
        .params
        .iter()
        .map(|p: &ResolvedParam| format!("{}: {}", p.name, ts_type_ref(&p.ty)))
        .collect::<Vec<String>>()
        .join(", ");
    let ret_type: String = match &func.returns {
        None => "void".to_owned(),
        Some(ty) => ts_type_ref(ty),
    };
    out.push_str(&format!(
        "    abstract {}({}): {};\n",
        func.name, params, ret_type
    ));
}

fn generate_vtable_ts() -> String {
    "// THIS FILE IS AUTO-GENERATED BY polyplugc\n\
     // DO NOT EDIT BY HAND\n\
     \n\
     export function registerVtable(\n\
         contractLo: number, contractHi: number,\n\
         vtablePtrLo: number, vtablePtrHi: number\n\
     ): void {\n\
     \x20   (globalThis as unknown as { polyplug: { registerVtable: (clo: number, chi: number, vlo: number, vhi: number) => void } }).polyplug.registerVtable(contractLo, contractHi, vtablePtrLo, vtablePtrHi);\n\
     }\n"
        .to_owned()
}

fn generate_init_ts(ir: &ValidatedIr) -> String {
    let has_trace: bool = ir
        .bundle
        .as_ref()
        .is_some_and(|b: &crate::ir::ResolvedBundle| {
            b.plugins
                .iter()
                .any(|p: &crate::ir::ResolvedPlugin| p.optional.contains(&"trace".to_owned()))
        });
    let mut out: String = String::new();
    out.push_str(
        "// THIS FILE IS AUTO-GENERATED BY polyplugc\n\
         // DO NOT EDIT BY HAND\n\
         // Runtime: js-quickjs\n\n",
    );
    out.push_str("function init(bundlePath: string): void {\n");
    out.push_str("    // Dependency resolution and vtable registration would go here.\n");
    out.push_str("    // Replace with generated values from bundle.toml.\n");
    if has_trace {
        out.push_str("    const EXT_TRACE_ID: number = 0xC4EB9AEE;\n");
        out.push_str("    // Optional: trace extension\n");
        out.push_str("    // QuickJS uses lo/hi u32 split for pointer-sized values\n");
        out.push_str(
            "    const { lo: traceLo, hi: traceHi } = polyplug.getExtension(EXT_TRACE_ID);\n",
        );
        out.push_str("    // traceLo and traceHi are both 0 if extension not available\n");
        out.push_str("    void traceLo; void traceHi;\n");
    }
    out.push_str("}\n\n");
    out.push_str("// bundlePath injected as global by polyplug host loader\n");
    out.push_str("init(globalThis.bundlePath);\n");
    out
}

fn generate_manifest_toml(ir: &ValidatedIr) -> String {
    let bundle: &ResolvedBundle = match ir.bundle.as_ref() {
        Some(b) => b,
        None => return String::from("# ERROR: bundle manifest called without bundle IR\n"),
    };

    let name: &str = &bundle.name;
    let version: String = format!(
        "{}.{}.{}",
        bundle.version.major, bundle.version.minor, bundle.version.patch
    );

    let mut provides: Vec<String> = bundle
        .plugins
        .iter()
        .flat_map(|p: &ResolvedPlugin| p.implements.iter().cloned())
        .collect();
    provides.sort();
    provides.dedup();

    let provides_toml: String = if provides.is_empty() {
        String::from("[]")
    } else {
        format!(
            "[{}]",
            provides
                .iter()
                .map(|s: &String| format!("\"{}\"", s))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };

    let fn_count_entries: Vec<String> = ir
        .contracts
        .iter()
        .map(|c: &ResolvedContract| {
            let fn_count: u32 = c.functions.len() as u32;
            format!("\"{}@{}\" = {}", c.name, c.version.major, fn_count)
        })
        .collect();
    let function_count_toml: String = format!("{{ {} }}", fn_count_entries.join(", "));

    let reinit: bool = bundle.needs_reinit_on_dep_reload;
    let file_field: String = super::format_manifest_file_field(&bundle.file);
    let runtime: &str = &bundle.runtime;

    format!(
        "# THIS FILE IS AUTO-GENERATED BY polyplugc. DO NOT EDIT.\n\
         name = \"{name}\"\n\
         bundle_name = \"{name}\"\n\
         version = \"{version}\"\n\
         runtime = \"{runtime}\"\n\
         {file_field}\n\
         provides = {provides_toml}\n\
         function_count = {function_count_toml}\n\
         needs_reinit_on_dep_reload = {reinit}\n\
         {dep_toml}"
    )
}

fn generate_readme_quickjs(ir: &ValidatedIr) -> String {
    let bundle_name: &str = ir
        .contracts
        .first()
        .map(|c: &ResolvedContract| c.name.as_str())
        .unwrap_or("my-bundle");
    format!(
        "# js-quickjs Plugin Bundle: {bundle_name}\n\n\
         ## Requirements\n\
         - rolldown: `npm i -g rolldown`\n\n\
         ## Build\n\
         ```bash\n\
         rolldown index.ts --format iife --platform neutral --file bundle.js\n\
         ```\n"
    )
}

fn generate_callers_ts(ir: &ValidatedIr) -> String {
    let mut out: String = String::new();
    out.push_str(
        "// THIS FILE IS AUTO-GENERATED BY polyplugc\n\
         // DO NOT EDIT BY HAND\n\
         // Runtime: js-quickjs (host-side callers)\n\n",
    );
    out.push_str("// TODO: implement host caller classes\n");
    // ir reserved for future caller generation
    let _: &ValidatedIr = ir;
    out
}

fn contract_to_class_name(contract_name: &str) -> String {
    contract_name
        .split('.')
        .map(|part: &str| {
            let mut chars: core::str::Chars<'_> = part.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect::<Vec<String>>()
        .join("")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::EnumDef;
    use crate::ir::EnumVariant;
    use crate::ir::ReprType;

    #[test]
    fn generate_js_quickjs_enum_non_bitflag() {
        let e: EnumDef = EnumDef {
            name: "PixelFormat".to_owned(),
            repr: ReprType::U32,
            bitflag: false,
            variants: vec![
                EnumVariant {
                    name: "Unknown".to_owned(),
                    value: "0".to_owned(),
                },
                EnumVariant {
                    name: "Rgba8".to_owned(),
                    value: "1".to_owned(),
                },
            ],
        };
        let mut out: String = String::new();
        generate_js_quickjs_enum(&mut out, &e);
        assert!(
            out.contains("Object.freeze({"),
            "missing Object.freeze: {out}"
        );
        assert!(out.contains("Unknown: 0"), "missing Unknown: {out}");
        assert!(
            !out.contains("@bitflag"),
            "non-bitflag should not have @bitflag: {out}"
        );
    }

    #[test]
    fn generate_js_quickjs_enum_bitflag() {
        let e: EnumDef = EnumDef {
            name: "ImageFlags".to_owned(),
            repr: ReprType::U32,
            bitflag: true,
            variants: vec![
                EnumVariant {
                    name: "None".to_owned(),
                    value: "0".to_owned(),
                },
                EnumVariant {
                    name: "Compressed".to_owned(),
                    value: "1".to_owned(),
                },
            ],
        };
        let mut out: String = String::new();
        generate_js_quickjs_enum(&mut out, &e);
        assert!(out.contains("@bitflag"), "missing @bitflag: {out}");
        assert!(
            out.contains("Object.freeze({"),
            "missing Object.freeze: {out}"
        );
    }
}
