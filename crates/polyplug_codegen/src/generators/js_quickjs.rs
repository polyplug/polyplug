//! js_quickjs generator — produces QuickJS-compatible TypeScript/JavaScript guest code.
//!
//! THIS FILE IS PART OF polyplugc.
//! Generates code using lo/hi u32 split for all u64/pointer values.

use std::path::PathBuf;

use crate::error::PolyplugcError;
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
    ) -> Result<(), PolyplugcError> {
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
    ) -> Result<(), PolyplugcError> {
        files.files.push(GeneratedFile {
            path: PathBuf::from("guest/types.ts"),
            content: generate_types_ts(ir),
            force_regenerate: false,
        });
        files.files.push(GeneratedFile {
            path: PathBuf::from("guest/contracts.ts"),
            content: generate_contracts_ts(ir)?,
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

fn generate_contracts_ts(ir: &ValidatedIr) -> Result<String, PolyplugcError> {
    let mut out: String = String::new();
    out.push_str(
        "// THIS FILE IS AUTO-GENERATED BY polyplugc\n\
         // DO NOT EDIT BY HAND\n\
         // Runtime: js-quickjs\n\n",
    );
    out.push_str("import type { } from './types';\n\n");

    if let Some(bundle) = &ir.bundle {
        for plugin in &bundle.plugins {
            for contract_impl in &plugin.implements {
                if let Some(contract) = ir.contracts.iter().find(|c| {
                    let contract_full =
                        format!("{}@{}.{}", c.name, c.version.major, c.version.minor);
                    &contract_full == contract_impl
                }) {
                    render_plugin_vtable_quickjs(&mut out, &plugin.name, contract)?;
                }
            }
        }
    }

    Ok(out)
}

fn render_plugin_vtable_quickjs(
    out: &mut String,
    plugin_name: &str,
    contract: &ResolvedContract,
) -> Result<(), PolyplugcError> {
    let plugin_var: String = plugin_name.to_uppercase().replace(['.', '-'], "_");
    let contract_name_full: String = format!("{}@{}", contract.name, contract.version.major);
    let contract_id: u64 = polyplug_abi::contract_id(&contract.name, contract.version.major);
    let contract_lo: u32 = (contract_id & 0xFFFFFFFF) as u32;
    let contract_hi: u32 = (contract_id >> 32) as u32;
    let function_count: usize = contract.functions.len();

    out.push_str(&format!(
        "// Plugin: {plugin_name} ({contract_name_full})\n"
    ));
    for func in &contract.functions {
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
            "//   {fn_name}({params}): {ret_type}\n",
            fn_name = func.name
        ));
    }

    out.push_str(&format!("\nconst {plugin_var}_VTABLE = {{\n"));
    out.push_str(&format!("    contractLo: 0x{:08X},\n", contract_lo));
    out.push_str(&format!("    contractHi: 0x{:08X},\n", contract_hi));
    out.push_str(&format!("    fnCount: {function_count},\n"));
    out.push_str("    functions: null as unknown as number[],\n");
    out.push_str(&format!("    contractName: \"{contract_name_full}\"\n"));
    out.push_str("};\n");

    let set_impl_name: String = format!(
        "set{}Impl",
        plugin_name
            .replace(['.', '-'], "_")
            .split('_')
            .map(|s| {
                let mut chars = s.chars();
                match chars.next() {
                    None => String::new(),
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                }
            })
            .collect::<String>()
    );

    out.push_str(&format!("\nexport function {set_impl_name}("));
    let impl_params: Vec<String> = contract
        .functions
        .iter()
        .enumerate()
        .map(|(idx, f)| {
            let params: String = f
                .params
                .iter()
                .map(|p| format!("{}: {}", p.name, ts_type_ref(&p.ty)))
                .collect::<Vec<String>>()
                .join(", ");
            let ret: String = match &f.returns {
                None => "void".to_owned(),
                Some(ty) => ts_type_ref(ty),
            };
            format!("fn{idx}: ({}) => {}", params, ret)
        })
        .collect();
    out.push_str(&impl_params.join(", "));
    out.push_str("): void {\n");

    // Build functions array
    out.push_str(&format!("    {plugin_var}_VTABLE.functions = ["));
    let fn_refs: Vec<String> = contract
        .functions
        .iter()
        .enumerate()
        .map(|(idx, _)| format!("fn{idx}"))
        .collect();
    out.push_str(&fn_refs.join(", "));
    out.push_str("];\n");

    out.push_str("    registerVtable(\n");
    out.push_str(&format!(
        "        {plugin_var}_VTABLE.contractLo, {plugin_var}_VTABLE.contractHi,\n"
    ));
    out.push_str("        1, 0, // vtable ptr (placeholder - host allocates)\n");
    out.push_str(&format!("        {plugin_var}_VTABLE.fnCount,\n"));
    out.push_str(&format!("        {plugin_var}_VTABLE.contractName\n"));
    out.push_str("    );\n");
    out.push_str("}\n");

    Ok(())
}

fn generate_vtable_ts() -> String {
    "// THIS FILE IS AUTO-GENERATED BY polyplugc\n\
     // DO NOT EDIT BY HAND\n\
     \n\
     export function registerVtable(\n\
         contractLo: number, contractHi: number,\n\
         vtablePtrLo: number, vtablePtrHi: number,\n\
         fnCount: number,\n\
         contractName: string\n\
     ): void {\n\
     \x20   (globalThis as unknown as { polyplug: { registerVtable: (clo: number, chi: number, vlo: number, vhi: number, fcnt: number, name: string) => void } }).polyplug.registerVtable(contractLo, contractHi, vtablePtrLo, vtablePtrHi, fnCount, contractName);\n\
     }\n"
        .to_owned()
}

fn generate_init_ts(ir: &ValidatedIr) -> String {
    let bundle: &ResolvedBundle = match ir.bundle.as_ref() {
        Some(b) => b,
        None => return String::from("// ERROR: init.ts called without bundle IR\n"),
    };

    let mut out: String = String::new();
    out.push_str(
        "// THIS FILE IS AUTO-GENERATED BY polyplugc\n\
         // DO NOT EDIT BY HAND\n\
         // Runtime: js-quickjs\n\n",
    );

    out.push_str("import {\n");
    for (idx, plugin) in bundle.plugins.iter().enumerate() {
        let set_impl_name: String = format!(
            "set{}Impl",
            plugin
                .name
                .replace(['.', '-'], "_")
                .split('_')
                .map(|s| {
                    let mut chars = s.chars();
                    match chars.next() {
                        None => String::new(),
                        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                    }
                })
                .collect::<String>()
        );
        if idx > 0 {
            out.push_str(",\n");
        }
        out.push_str(&format!("    {set_impl_name}"));
    }
    out.push_str("\n} from './contracts';\n\n");

    out.push_str("// Plugin implementation registration\n");
    out.push_str("// Call each set*Impl function with your plugin's function implementations\n");
    out.push_str("// Example:\n");
    for plugin in &bundle.plugins {
        let set_impl_name: String = format!(
            "set{}Impl",
            plugin
                .name
                .replace(['.', '-'], "_")
                .split('_')
                .map(|s| {
                    let mut chars = s.chars();
                    match chars.next() {
                        None => String::new(),
                        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                    }
                })
                .collect::<String>()
        );
        out.push_str(&format!(
            "//   {set_impl_name}(/* your function implementations */);\n"
        ));
    }

    out.push_str("\n// Implement your plugin functions and call set*Impl above to register them\n");
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
        .map(|impl_str: String| {
            if let Some(at_pos) = impl_str.find('@') {
                let contract_name: &str = &impl_str[..at_pos];
                let version_part: &str = &impl_str[at_pos + 1..];
                if let Some(dot_pos) = version_part.find('.') {
                    let major: &str = &version_part[..dot_pos];
                    format!("{}@{}", contract_name, major)
                } else {
                    impl_str
                }
            } else {
                impl_str
            }
        })
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

    let provides_set: std::collections::HashSet<String> = provides.iter().cloned().collect();
    let fn_count_entries: Vec<String> = ir
        .contracts
        .iter()
        .filter(|c: &&ResolvedContract| {
            provides_set.contains(&format!("{}@{}", c.name, c.version.major))
        })
        .map(|c: &ResolvedContract| {
            let fn_count: u32 = c.functions.len() as u32;
            format!("\"{}@{}\" = {}", c.name, c.version.major, fn_count)
        })
        .collect();
    let function_count_toml: String = format!("{{ {} }}", fn_count_entries.join(", "));

    let reinit: bool = bundle.needs_reinit_on_dep_reload;
    let file_field: String = super::format_manifest_file_field(&bundle.file);
    let runtime: &str = &bundle.runtime;

    let mut dep_toml: String = String::new();
    for dep in &bundle.dependencies {
        dep_toml.push_str("\n[[dependency]]\n");
        match dep {
            ResolvedDependency::ByContract {
                contract,
                min_version,
                ..
            } => {
                dep_toml.push_str("kind = \"contract\"\n");
                dep_toml.push_str(&format!("contract = \"{contract}\"\n"));
                dep_toml.push_str(&format!("min_version = \"{min_version}.0\"\n"));
            }
            ResolvedDependency::ByBundle {
                bundle,
                contract,
                min_version,
                ..
            } => {
                dep_toml.push_str("kind = \"bundle\"\n");
                dep_toml.push_str(&format!("bundle = \"{bundle}\"\n"));
                dep_toml.push_str(&format!("contract = \"{contract}\"\n"));
                dep_toml.push_str(&format!("min_version = \"{min_version}.0\"\n"));
            }
        }
    }

    format!(
        "# THIS FILE IS AUTO-GENERATED BY polyplugc. DO NOT EDIT.\n\
         name = \"{name}\"\n\
         bundle_id = {bundle_id}\n\
         version = \"{version}\"\n\
         runtime = \"{runtime}\"\n\
         provides = {provides_toml}\n\
         function_count = {function_count_toml}\n\
         needs_reinit_on_dep_reload = {reinit}\n\
         {file_field}\n\
         {dep_toml}",
        bundle_id = bundle.bundle_id
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

    out.push_str("import { getVtable } from './types';\n\n");

    // Module-level function pointer cache to avoid repeated type casting
    out.push_str("// Function pointer cache - avoids repeated type casting overhead\n");
    out.push_str("const _funcCache = new Map<number, (args: any, out: any) => { lo: number; hi: number }>();\n\n");

    for contract in &ir.contracts {
        generate_host_caller_class_quickjs(&mut out, contract);
    }

    out
}

fn generate_host_caller_class_quickjs(out: &mut String, contract: &ResolvedContract) {
    let class_name: String = contract_to_class_name(&contract.name);
    let contract_id: u64 = polyplug_abi::contract_id(&contract.name, contract.version.major);
    let contract_lo: u32 = (contract_id & 0xFFFFFFFF) as u32;
    let contract_hi: u32 = (contract_id >> 32) as u32;

    out.push_str(&format!(
        "/** Host caller for contract `{}` */\n",
        contract.name
    ));
    out.push_str(&format!("export class {}Contract {{\n", class_name));
    out.push_str("    private vtable: any;\n\n");
    out.push_str("    constructor() {\n");
    out.push_str(&format!(
        "        this.vtable = getVtable(0x{:08X}, 0x{:08X});\n",
        contract_lo, contract_hi
    ));
    out.push_str("    }\n\n");

    for func in &contract.functions {
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

        out.push_str(&format!("    /** Call `{}` */\n", func.name));
        out.push_str(&format!("    {}({}): {} {{\n", func.name, params, ret_type));

        if func.params.is_empty() {
            out.push_str("        const argsPtr = 0;\n");
        } else if func.params.len() == 1 {
            let param = &func.params[0];
            out.push_str(&format!("        const argsPtr = {};\n", param.name));
        } else {
            out.push_str("        const args = {");
            for param in &func.params {
                out.push_str(&format!(" {}: {},", param.name, param.name));
            }
            out.push_str(" };\n");
            out.push_str("        const argsPtr = args;\n");
        }

        out.push_str(&format!(
            "        const fnPtr = this.vtable.functions[{}];\n",
            func.function_id
        ));
        out.push_str("        if (!fnPtr) throw new Error('function not available');\n");
        out.push_str("        let fn = _funcCache.get(fnPtr);\n");
        out.push_str("        if (!fn) {\n");
        out.push_str("            fn = fnPtr as unknown as (args: any, out: any) => { lo: number; hi: number };\n");
        out.push_str("            _funcCache.set(fnPtr, fn);\n");
        out.push_str("        }\n");
        out.push_str("        const outVal = { lo: 0, hi: 0 };\n");
        out.push_str("        const err = fn(argsPtr, outVal);\n");
        out.push_str("        if (err.lo !== 0 || err.hi !== 0) throw new Error('call failed');\n");

        if func.returns.is_some() {
            out.push_str("        return outVal;\n");
        } else {
            out.push_str("        return;\n");
        }

        out.push_str("    }\n\n");
    }

    out.push_str("}\n\n");
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
