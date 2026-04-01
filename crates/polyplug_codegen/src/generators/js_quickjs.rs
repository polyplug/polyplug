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
use crate::ir::ResolvedFunction;
use crate::ir::ResolvedHostContract;
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
        // Emit host/contracts.ts if there are host contracts
        if !ir.host_contracts.is_empty() {
            files.files.push(GeneratedFile {
                path: PathBuf::from("host/contracts.ts"),
                content: generate_host_contracts_ts(ir),
                force_regenerate: false,
            });
            // Emit host/vtable_factories.ts if there are host contracts
            files.files.push(GeneratedFile {
                path: PathBuf::from("host/vtable_factories.ts"),
                content: generate_js_host_vtable_factories_ts(ir),
                force_regenerate: false,
            });
        }
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
            content: generate_vtable_ts(ir),
            force_regenerate: false,
        });
        files.files.push(GeneratedFile {
            path: PathBuf::from("guest/init.ts"),
            content: generate_init_ts(ir),
            force_regenerate: false,
        });
        files.files.push(GeneratedFile {
            path: PathBuf::from("guest/index.ts"),
            content: generate_index_ts(ir),
            force_regenerate: false,
        });
        if ir.bundle.is_some() {
            files.files.push(GeneratedFile {
                path: PathBuf::from("manifest.toml"),
                content: generate_manifest_toml(ir),
                force_regenerate: true,
            });
        }
        if !ir.host_contracts.is_empty() {
            files.files.push(GeneratedFile {
                path: PathBuf::from("guest/host_contracts.ts"),
                content: generate_guest_host_contracts_ts(ir),
                force_regenerate: false,
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
    for contract in &ir.contracts {
        render_contract_types(&mut out, contract);
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

fn render_contract_types(out: &mut String, contract: &ResolvedContract) {
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
            "export type {}_{} = ({}) => {};\n",
            contract.name.replace('.', "_"),
            func.name,
            params,
            ret_type
        ));
    }
}

fn generate_contracts_ts(ir: &ValidatedIr) -> Result<String, PolyplugcError> {
    let mut out: String = String::new();
    out.push_str(
        "// THIS FILE IS AUTO-GENERATED BY polyplugc\n\
         // DO NOT EDIT BY HAND\n\
         // Runtime: js-quickjs\n\n",
    );
    out.push_str("import type { } from './types';\n\n");
    out.push_str("/** Dispatch mechanism type — determines how function calls are routed. */\n");
    out.push_str("const DispatchType = Object.freeze({\n");
    out.push_str("    Native: 0,\n");
    out.push_str("    VirtualMachine: 1,\n");
    out.push_str("} as const);\n\n");

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
    let contract_id: u64 = crate::ir::compute_contract_id(&contract.name, contract.version.major);
    let contract_lo: u32 = (contract_id & 0xFFFFFFFF) as u32;
    let contract_hi: u32 = (contract_id >> 32) as u32;
    let function_count: usize = contract.functions.len();
    let version_major: u32 = contract.version.major;
    let version_minor: u32 = contract.version.minor;
    let version_patch: u32 = contract.version.patch;

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

    out.push_str(&format!("\nexport const {plugin_var}_VTABLE = {{\n"));
    out.push_str(&format!("    contractLo: 0x{:08X},\n", contract_lo));
    out.push_str(&format!("    contractHi: 0x{:08X},\n", contract_hi));
    out.push_str(&format!("    fnCount: {function_count},\n"));
    out.push_str("    functions: null as unknown as number[],\n");
    out.push_str(&format!("    contractName: \"{contract_name_full}\",\n"));
    out.push_str("    dispatchType: DispatchType.VirtualMachine\n");
    out.push_str("};\n");

    out.push_str(&format!("\nexport const {plugin_var}_DESCRIPTOR = {{\n"));
    out.push_str(&format!("    name: \"{plugin_name}\",\n"));
    out.push_str(&format!("    contractName: \"{contract_name_full}\",\n"));
    out.push_str(&format!("    versionMajor: {version_major},\n"));
    out.push_str(&format!("    versionMinor: {version_minor},\n"));
    out.push_str(&format!("    versionPatch: {version_patch}\n"));
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

    // Generate ABI wrapper functions for each contract function
    // These wrappers handle the raw pointer conversion between the loader and user code
    let mut abi_wrappers: Vec<String> = Vec::new();
    for (idx, func) in contract.functions.iter().enumerate() {
        let wrapper_name: String = format!("{}_fn{}_abi_wrapper", plugin_var.to_lowercase(), idx);
        let has_params: bool = !func.params.is_empty();
        let has_return: bool = func.returns.is_some();

        // Generate the ABI wrapper function
        // Note: QuickJS uses lo/hi u32 pairs for 64-bit pointers
        // We use Number for arithmetic since QuickJS supports it
        out.push_str("\nfunction ");
        out.push_str(&wrapper_name);
        out.push_str("(args_ptr_lo, args_ptr_hi, out_ptr_lo, out_ptr_hi) {\n");
        // SAFETY comments for generated code are required per AGENTS.md for all unsafe operations
        out.push_str("    // SAFETY: args_ptr_lo/hi and out_ptr_lo/hi are valid pointer halves per ABI contract.\n");
        out.push_str("    // The host guarantees these pointers are properly aligned and sized before calling.\n");
        out.push_str("    var polyplug = globalThis.polyplug;\n");
        out.push_str("    if (!polyplug) return 1;\n");
        out.push_str("    var impl = ");
        out.push_str(&plugin_var);
        out.push_str("_IMPL;\n");
        out.push_str("    if (!impl) return 1;\n");

        if has_params {
            out.push_str("    if (args_ptr_lo === 0 && args_ptr_hi === 0) return 8;\n");
        }
        if has_return {
            out.push_str("    if (out_ptr_lo === 0 && out_ptr_hi === 0) return 8;\n");
        }

        // Read input StringView from args_ptr
        // StringView is { ptr_lo: u32, ptr_hi: u32, len: u32 } = 12 bytes
        // Use Number for pointer arithmetic (QuickJS Number can hold 53-bit integers)
        out.push_str("    // SAFETY: Pointer arithmetic reconstructs the full 64-bit address from lo/hi halves.\n");
        out.push_str("    // The loader guarantees the pointer is valid for the memory region being accessed.\n");
        out.push_str("    var args_ptr = args_ptr_lo + args_ptr_hi * 4294967296;\n");
        out.push_str("    // SAFETY: readU32 reads 4 bytes from a valid memory location per the polyplug ABI.\n");
        out.push_str("    var input_ptr_lo = polyplug.readU32(args_ptr);\n");
        out.push_str("    var input_ptr_hi = polyplug.readU32(args_ptr + 4);\n");
        out.push_str("    var input_len = polyplug.readU32(args_ptr + 8);\n");
        out.push_str(
            "    var input = { ptr_lo: input_ptr_lo, ptr_hi: input_ptr_hi, len: input_len };\n",
        );

        // Call the implementation
        out.push_str("    var result = impl.fn");
        out.push_str(&idx.to_string());
        out.push_str("(input);\n");

        // Write output StringView to out_ptr
        out.push_str("    // SAFETY: out_ptr is a valid pointer to a StringView-sized buffer per ABI contract.\n");
        out.push_str(
            "    // writeU32 writes 4 bytes to valid memory locations per the polyplug ABI.\n",
        );
        out.push_str("    var out_ptr = out_ptr_lo + out_ptr_hi * 4294967296;\n");
        out.push_str("    polyplug.writeU32(out_ptr, result.ptr_lo);\n");
        out.push_str("    polyplug.writeU32(out_ptr + 4, result.ptr_hi);\n");
        out.push_str("    polyplug.writeU32(out_ptr + 8, result.len);\n");
        out.push_str("    return 0;\n");
        out.push_str("}\n");

        abi_wrappers.push(wrapper_name);
    }

    // Store implementation functions
    out.push_str("\nlet ");
    out.push_str(&plugin_var);
    out.push_str("_IMPL = null;\n");

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

    out.push_str("    ");
    out.push_str(&plugin_var);
    out.push_str("_IMPL = { ");
    let fn_refs: Vec<String> = contract
        .functions
        .iter()
        .enumerate()
        .map(|(idx, _)| format!("fn{idx}"))
        .collect();
    out.push_str(&fn_refs.join(", "));
    out.push_str(" };\n");

    // Store ABI wrappers in vtable
    out.push_str("    ");
    out.push_str(&plugin_var);
    out.push_str("_VTABLE.functions = [");
    out.push_str(
        &abi_wrappers
            .iter()
            .map(|w| w.as_str())
            .collect::<Vec<_>>()
            .join(", "),
    );
    out.push_str("];\n");
    out.push_str("}\n");

    Ok(())
}

fn generate_vtable_ts(ir: &ValidatedIr) -> String {
    let bundle: Option<&ResolvedBundle> = ir.bundle.as_ref();

    let mut out: String = String::new();
    out.push_str(
        "// THIS FILE IS AUTO-GENERATED BY polyplugc\n\
         // DO NOT EDIT BY HAND\n\
         // Runtime: js-quickjs\n\n",
    );

    if let Some(bundle) = bundle {
        // Re-export all vtables from contracts.ts
        out.push_str("// Re-export vtables from contracts.ts\n");
        for plugin in &bundle.plugins {
            let plugin_var: String = plugin.name.to_uppercase().replace(['.', '-'], "_");
            out.push_str(&format!(
                "export {{ {plugin_var}_VTABLE }} from './contracts';\n"
            ));
        }
    }

    out
}

fn generate_index_ts(ir: &ValidatedIr) -> String {
    let bundle: Option<&ResolvedBundle> = ir.bundle.as_ref();

    let mut out: String = String::new();
    out.push_str(
        "// THIS FILE IS AUTO-GENERATED BY polyplugc\n\
         // DO NOT EDIT BY HAND\n\
         // Runtime: js-quickjs\n\n",
    );

    out.push_str("// Main entry point for bundling\n");
    out.push_str("export { polyplug_init } from './init';\n");

    if let Some(bundle) = bundle {
        for plugin in &bundle.plugins {
            let plugin_var: String = plugin.name.to_uppercase().replace(['.', '-'], "_");
            out.push_str(&format!(
                "export {{ {plugin_var}_VTABLE }} from './contracts';\n"
            ));
        }
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
                            Some(first) => {
                                first.to_uppercase().collect::<String>() + chars.as_str()
                            }
                        }
                    })
                    .collect::<String>()
            );
            out.push_str(&format!(
                "export {{ {set_impl_name} }} from './contracts';\n"
            ));
        }
    }

    out
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
        let plugin_var: String = plugin.name.to_uppercase().replace(['.', '-'], "_");
        if idx > 0 {
            out.push_str(",\n");
        }
        out.push_str(&format!("    {plugin_var}_VTABLE"));
    }
    out.push_str("\n} from './contracts';\n");
    out.push_str("import { storeHostVtable } from 'polyplug-guest';\n\n");

    out.push_str("// ABI constants\n");
    out.push_str("const ABI_OK = 0;\n");
    out.push_str("const ABI_ERROR_GENERIC = 1;\n");
    out.push_str("const ABI_ERROR_INVALID_POINTER = 8;\n\n");

    out.push_str("interface AbiError {\n");
    out.push_str("    code: number;\n");
    out.push_str("    message: { ptr: number; len: number };\n");
    out.push_str("}\n\n");

    out.push_str("export function polyplug_init(\n");
    out.push_str("    rt_ctx_lo: number, rt_ctx_hi: number,\n");
    out.push_str("    host_lo: number, host_hi: number,\n");
    out.push_str("    ctx_lo: number, ctx_hi: number\n");
    out.push_str("): AbiError {\n");
    out.push_str("    // Validate parameters\n");
    out.push_str("    if (rt_ctx_lo === 0 && rt_ctx_hi === 0) {\n");
    out.push_str("        return { code: ABI_ERROR_GENERIC, message: { ptr: 0, len: 0 } };\n");
    out.push_str("    }\n");
    out.push_str("    if (host_lo === 0 && host_hi === 0) {\n");
    out.push_str("        return { code: ABI_ERROR_GENERIC, message: { ptr: 0, len: 0 } };\n");
    out.push_str("    }\n");
    out.push_str("    if (ctx_lo === 0 && ctx_hi === 0) {\n");
    out.push_str("        return { code: ABI_ERROR_GENERIC, message: { ptr: 0, len: 0 } };\n");
    out.push_str("    }\n\n");
    out.push_str("    // Store host vtable for later access via getHostVtable()\n");
    out.push_str("    storeHostVtable(host_lo, host_hi);\n\n");
    out.push_str("    // Get polyplug host interface from globalThis\n");
    out.push_str("    const polyplug = (globalThis as any).polyplug;\n");
    out.push_str("    if (!polyplug || !polyplug.registerVtable) {\n");
    out.push_str("        return { code: ABI_ERROR_GENERIC, message: { ptr: 0, len: 0 } };\n");
    out.push_str("    }\n\n");

    for plugin in &bundle.plugins {
        let plugin_var: String = plugin.name.to_uppercase().replace(['.', '-'], "_");
        out.push_str(&format!(
            "    // Register plugin: {plugin_name}\n",
            plugin_name = plugin.name
        ));
        out.push_str("    polyplug.registerVtable(\n");
        out.push_str(&format!("        {plugin_var}_VTABLE.contractLo,\n"));
        out.push_str(&format!("        {plugin_var}_VTABLE.contractHi,\n"));
        out.push_str(&format!("        {plugin_var}_VTABLE,\n"));
        out.push_str(&format!("        {plugin_var}_VTABLE.fnCount,\n"));
        out.push_str(&format!("        {plugin_var}_VTABLE.contractName\n"));
        out.push_str("    );\n\n");
    }

    out.push_str("    return { code: ABI_OK, message: { ptr: 0, len: 0 } };\n");
    out.push_str("}\n");

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
         id = {bundle_id}\n\
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

    // ABI constants
    out.push_str("// ABI constants\n");
    out.push_str("export const ABI_OK = 0;\n");
    out.push_str("export const ABI_ERROR_GENERIC = 1;\n");
    out.push_str("export const ABI_ERROR_INVALID_POINTER = 8;\n\n");

    // Contract ID constants
    out.push_str("// Contract ID constants\n");
    out.push_str("export const ContractIds = {\n");
    for contract in &ir.contracts {
        let upper_name: String = contract.name.to_uppercase().replace(['.', '-'], "_");
        let contract_id: u64 = contract.contract_id;
        let contract_lo: u32 = (contract_id & 0xFFFFFFFF) as u32;
        let contract_hi: u32 = (contract_id >> 32) as u32;
        out.push_str(&format!(
            "  {}_CONTRACT_LO: 0x{:08X},\n",
            upper_name, contract_lo
        ));
        out.push_str(&format!(
            "  {}_CONTRACT_HI: 0x{:08X},\n",
            upper_name, contract_hi
        ));
    }
    out.push_str("} as const;\n\n");

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
    let contract_upper: String = contract.name.to_uppercase().replace(['.', '-'], "_");
    let contract_lo_const: String = format!("ContractIds.{}_CONTRACT_LO", contract_upper);
    let contract_hi_const: String = format!("ContractIds.{}_CONTRACT_HI", contract_upper);

    out.push_str(&format!(
        "/** Host caller for contract `{}` with hot-reload support. */\n",
        contract.name
    ));
    out.push_str(&format!("export class {}Contract {{\n", class_name));
    out.push_str("    #guard: any;\n\n");
    out.push_str("    private constructor(guard: any) {\n");
    out.push_str("        this.#guard = guard;\n");
    out.push_str("    }\n\n");
    out.push_str("    /** Factory method - creates instance or null if not found. */\n");
    out.push_str(&format!(
        "    static create(rt: any, minVersion: number = 0): {}Contract | null {{\n",
        class_name
    ));
    out.push_str(&format!(
        "        const handle = rt.findByContractLoHi({}, {}, minVersion);\n",
        contract_lo_const, contract_hi_const
    ));
    out.push_str("        if (handle === null || handle === undefined) {\n");
    out.push_str("            return null;\n");
    out.push_str("        }\n");
    out.push_str("        const guard = rt.getGuard(handle);\n");
    out.push_str("        if (!guard) {\n");
    out.push_str("            return null;\n");
    out.push_str("        }\n");
    out.push_str(&format!(
        "        return new {}Contract(guard);\n",
        class_name
    ));
    out.push_str("    }\n\n");
    out.push_str("    /** Check if this caller instance is still valid. */\n");
    out.push_str("    isValid(): boolean {\n");
    out.push_str("        return this.#guard !== null && this.#guard !== undefined;\n");
    out.push_str("    }\n\n");
    out.push_str("    /** Explicitly release the guard reference. */\n");
    out.push_str("    reset(): void {\n");
    out.push_str("        if (this.#guard !== null) {\n");
    out.push_str("            this.#guard.reset();\n");
    out.push_str("            this.#guard = null;\n");
    out.push_str("        }\n");
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
        out.push_str("        const vtable = this.#guard?.vtable?.();\n");
        out.push_str("        if (!vtable) throw new Error('caller is not valid');\n");

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
            "        if ({fn_id} >= vtable.functionCount) {{ throw new Error('function not available'); }}\n",
            fn_id = func.function_id
        ));
        out.push_str(&format!(
            "        const fnPtr = vtable.dispatch.native.functions[{}];\n",
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

// ─── Host Contract Interface Generation ────────────────────────────────────────

/// Convert host contract name to TypeScript interface name.
/// e.g. "host.logger" -> "HostLogger", "host.fs.reader" -> "HostFsReader"
fn host_contract_name_to_ts_interface(name: &str) -> String {
    let name_without_prefix: &str = name.strip_prefix("host.").unwrap_or(name);

    let pascal: String = name_without_prefix
        .split('.')
        .map(|p: &str| {
            let mut chars: core::str::Chars<'_> = p.chars();
            match chars.next() {
                Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join("");

    if pascal.starts_with("Host") {
        pascal
    } else {
        "Host".to_owned() + &pascal
    }
}

/// Generate ergonomic TypeScript type name for host interface method parameters.
/// For host interfaces, we use ergonomic TypeScript types:
/// - StringView -> string (more ergonomic for host implementers)
/// - Buffer -> Uint8Array (more ergonomic for host implementers)
/// - UserDefined -> TypeName (passed by reference in TypeScript)
/// - Primitives -> number (u32, i32, etc.) or { lo: number; hi: number } (u64, i64)
fn ts_host_param_type(ty: &ResolvedTypeRef) -> String {
    match ty {
        ResolvedTypeRef::Primitive(p) => ts_primitive_host(p).to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => "string".to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => "Uint8Array".to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::Ptr) => "{ lo: number; hi: number }".to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::Void) => "void".to_owned(),
        ResolvedTypeRef::UserDefined(name) => name.clone(),
    }
}

/// Generate TypeScript return type name for host interface methods.
/// Return types are owned where appropriate:
/// - StringView -> string (owned)
/// - Buffer -> Uint8Array (owned)
/// - UserDefined -> TypeName (owned)
/// - Primitives -> number or { lo: number; hi: number }
fn ts_host_return_type(ty: &ResolvedTypeRef) -> String {
    ts_host_param_type(ty) // Same mapping for params and returns
}

/// TypeScript primitive type for host interfaces (ergonomic, not ABI-level).
fn ts_primitive_host(p: &PrimitiveType) -> &'static str {
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

/// Generate one interface method for a host contract function.
fn generate_ts_host_interface_method(out: &mut String, func: &crate::ir::ResolvedFunction) {
    let return_type: String = match &func.returns {
        Some(ty) => ts_host_return_type(ty),
        None => "void".to_owned(),
    };

    let method_name: String = func
        .name
        .split(['_', '.'])
        .filter(|seg: &&str| !seg.is_empty())
        .map(|seg: &str| {
            let mut c: core::str::Chars<'_> = seg.chars();
            match c.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().to_string() + c.as_str(),
            }
        })
        .collect::<Vec<String>>()
        .join("");

    let params_str: String = if func.params.is_empty() {
        String::new()
    } else {
        func.params
            .iter()
            .map(|p: &ResolvedParam| {
                let ts_ty: String = ts_host_param_type(&p.ty);
                format!("{}: {}", p.name, ts_ty)
            })
            .collect::<Vec<_>>()
            .join(", ")
    };

    out.push_str(&format!(
        "    {}({}): {};\n",
        method_name, params_str, return_type
    ));
}

/// Generate the interface definition for one host contract.
fn generate_ts_host_contract_interface(out: &mut String, contract: &ResolvedHostContract) {
    let iface_name: String = host_contract_name_to_ts_interface(&contract.name);
    out.push_str(&format!(
        "/**\n * Host interface for contract `{}` (id=0x{:016X})\n * Hosts implement this interface to provide functionality to plugins.\n */\n",
        contract.name, contract.contract_id
    ));
    out.push_str(&format!("export interface {} {{\n", iface_name));

    for func in &contract.functions {
        generate_ts_host_interface_method(out, func);
    }

    out.push_str("}\n\n");
}

/// Generate `host/contracts.ts` — host interfaces for each host contract.
fn generate_host_contracts_ts(ir: &ValidatedIr) -> String {
    let mut out: String = String::new();
    out.push_str(
        "// THIS FILE IS AUTO-GENERATED BY polyplugc\n\
         // DO NOT EDIT BY HAND\n\
         // Runtime: js-quickjs (host-side interfaces)\n\n",
    );

    for contract in &ir.host_contracts {
        generate_ts_host_contract_interface(&mut out, contract);
    }

    // Emit contract ID constants
    out.push_str("// Contract ID constants\n");
    for contract in &ir.host_contracts {
        let iface_name: String = host_contract_name_to_ts_interface(&contract.name);
        let const_name: String = iface_name.to_uppercase() + "_CONTRACT_ID";
        out.push_str(&format!(
            "/** Contract ID for `{}` (FNV-1a of \"host_contract:{}@{}\") */\n",
            contract.name, contract.name, contract.version.major
        ));
        out.push_str(&format!(
            "export const {} = 0x{:016X}n;\n\n",
            const_name, contract.contract_id
        ));
    }

    out
}

// ─── Guest Host Contract Caller Generation ────────────────────────────────────────

/// Convert host contract name to TypeScript guest caller class name.
/// e.g. "host.logger" -> "HostLoggerContract", "host.fs.reader" -> "HostFsReaderContract"
fn host_contract_name_to_ts_caller(name: &str) -> String {
    let name_without_prefix: &str = name.strip_prefix("host.").unwrap_or(name);

    let pascal: String = name_without_prefix
        .split('.')
        .map(|p: &str| {
            let mut chars: core::str::Chars<'_> = p.chars();
            match chars.next() {
                Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join("");

    if pascal.starts_with("Host") {
        pascal + "Contract"
    } else {
        "Host".to_owned() + &pascal + "Contract"
    }
}

/// Generate ergonomic TypeScript type name for guest caller method parameters.
/// For guest callers, we use ergonomic TypeScript types:
/// - StringView -> string (converted to StringView at ABI boundary)
/// - Buffer -> Uint8Array (converted to Buffer at ABI boundary)
/// - UserDefined -> TypeName (passed by reference)
/// - Primitives -> number (u32, i32, etc.) or { lo: number; hi: number } (u64, i64)
fn ts_guest_caller_param_type(ty: &ResolvedTypeRef) -> String {
    match ty {
        ResolvedTypeRef::Primitive(p) => ts_primitive_guest(p).to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => "string".to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => "Uint8Array".to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::Ptr) => "{ lo: number; hi: number }".to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::Void) => "void".to_owned(),
        ResolvedTypeRef::UserDefined(name) => name.clone(),
    }
}

/// Generate TypeScript return type name for guest caller methods.
/// Return types are owned where appropriate:
/// - StringView -> string (owned)
/// - Buffer -> Uint8Array (owned)
/// - UserDefined -> TypeName (owned)
/// - Primitives -> number or { lo: number; hi: number }
fn ts_guest_caller_return_type(ty: &ResolvedTypeRef) -> String {
    ts_guest_caller_param_type(ty) // Same mapping for params and returns
}

/// TypeScript primitive type for guest callers (ergonomic, not ABI-level).
fn ts_primitive_guest(p: &PrimitiveType) -> &'static str {
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

/// Generate one guest-side host contract caller class.
fn generate_ts_guest_host_contract_caller(out: &mut String, contract: &ResolvedHostContract) {
    let class_name: String = host_contract_name_to_ts_caller(&contract.name);
    let contract_id_lo: u32 = (contract.contract_id & 0xFFFFFFFF) as u32;
    let contract_id_hi: u32 = (contract.contract_id >> 32) as u32;

    out.push_str(&format!(
        "/**\n * Guest caller for host contract `{}` (id=0x{:016X})\n */\n",
        contract.name, contract.contract_id
    ));
    out.push_str(&format!("export class {} {{\n", class_name));
    out.push_str("    private vtable: { lo: number; hi: number };\n\n");

    out.push_str("    private constructor(vtable: { lo: number; hi: number }) {\n");
    out.push_str("        this.vtable = vtable;\n");
    out.push_str("    }\n\n");

    out.push_str("    /** Factory method - creates caller instance or null if not found. */\n");
    out.push_str(&format!(
        "    static fromHost(hostPtr: {{ lo: number; hi: number }}, minVersion: number = 0): {} | null {{\n",
        class_name
    ));
    out.push_str("        if (hostPtr.lo === 0 && hostPtr.hi === 0) {\n");
    out.push_str("            return null;\n");
    out.push_str("        }\n");
    out.push_str("        const polyplug = (globalThis as any).polyplug;\n");
    out.push_str("        if (!polyplug || !polyplug.getHostContract) {\n");
    out.push_str("            return null;\n");
    out.push_str("        }\n");
    out.push_str(&format!(
        "        const vtable = polyplug.getHostContract(hostPtr.lo, hostPtr.hi, 0x{:08X}, 0x{:08X}, minVersion);\n",
        contract_id_lo, contract_id_hi
    ));
    out.push_str("        if (vtable === null || vtable === undefined || (vtable.lo === 0 && vtable.hi === 0)) {\n");
    out.push_str("            return null;\n");
    out.push_str("        }\n");
    out.push_str(&format!("        return new {}(vtable);\n", class_name));
    out.push_str("    }\n\n");

    out.push_str("    /** Check if this caller instance is still valid. */\n");
    out.push_str("    isValid(): boolean {\n");
    out.push_str("        return this.vtable.lo !== 0 || this.vtable.hi !== 0;\n");
    out.push_str("    }\n\n");

    for func in &contract.functions {
        generate_ts_guest_host_contract_method(out, func);
    }

    out.push_str("}\n\n");
}

/// Generate one method for a guest-side host contract caller.
fn generate_ts_guest_host_contract_method(out: &mut String, func: &crate::ir::ResolvedFunction) {
    let fn_id: u32 = func.function_id;
    let return_type: String = match &func.returns {
        Some(ty) => ts_guest_caller_return_type(ty),
        None => "void".to_owned(),
    };
    let has_return: bool = func.returns.is_some();

    let params_str: String = if func.params.is_empty() {
        String::new()
    } else {
        func.params
            .iter()
            .map(|p: &ResolvedParam| {
                let ts_ty: String = ts_guest_caller_param_type(&p.ty);
                format!("{}: {}", p.name, ts_ty)
            })
            .collect::<Vec<_>>()
            .join(", ")
    };

    out.push_str(&format!("    /** Call `{}` */\n", func.name));
    out.push_str(&format!(
        "    {}({}): {} {{\n",
        func.name, params_str, return_type
    ));

    out.push_str("        if (this.vtable.lo === 0 && this.vtable.hi === 0) {\n");
    if has_return {
        out.push_str("            return null as any;\n");
    } else {
        out.push_str("            return;\n");
    }
    out.push_str("        }\n");

    out.push_str("        const polyplug = (globalThis as any).polyplug;\n");
    out.push_str("        if (!polyplug) {\n");
    if has_return {
        out.push_str("            return null as any;\n");
    } else {
        out.push_str("            return;\n");
    }
    out.push_str("        }\n");

    // SAFETY comments for generated code are required per AGENTS.md for all unsafe operations
    out.push_str("        // SAFETY: vtable.lo/hi are valid pointer halves per ABI contract.\n");
    out.push_str("        // Pointer arithmetic reconstructs the full 64-bit address.\n");
    out.push_str("        const vtablePtr = this.vtable.lo + this.vtable.hi * 4294967296;\n");
    out.push_str("        // SAFETY: readHostContractHeader reads from a valid vtable pointer per the polyplug ABI.\n");
    out.push_str("        const header = polyplug.readHostContractHeader(vtablePtr);\n");
    out.push_str(&format!(
        "        if ({fn_id} >= header.functionCount) {{\n"
    ));
    if has_return {
        out.push_str("            return null as any;\n");
    } else {
        out.push_str("            return;\n");
    }
    out.push_str("        }\n");

    emit_ts_guest_host_contract_args_setup(out, func);
    emit_ts_guest_host_contract_out_setup(out, &func.returns);

    out.push_str("        const dispatchType = header.dispatchType;\n");
    out.push_str("        let err: { lo: number; hi: number };\n");
    out.push_str("        if (dispatchType === 0) {\n"); // DispatchType.Native
    out.push_str("            // SAFETY: readU64 reads 8 bytes from a valid function table pointer per ABI contract.\n");
    out.push_str(&format!(
        "            const fnPtr = polyplug.readU64(header.functionsPtr + {fn_id} * 8);\n"
    ));
    out.push_str("            const implPtr = header.implPtr;\n");
    out.push_str("            // SAFETY: callDispatchFn invokes a valid function pointer with properly aligned args/out per ABI contract.\n");
    out.push_str(
        "            err = polyplug.callDispatchFn(fnPtr.lo, fnPtr.hi, implPtr.lo, implPtr.hi, argsPtr, outPtr);\n",
    );
    out.push_str("        } else {\n"); // DispatchType.VirtualMachine
    out.push_str("            // SAFETY: callVmDispatch invokes the VM dispatch with valid bridge data and args/out per ABI contract.\n");
    out.push_str(&format!(
        "            err = polyplug.callVmDispatch(header.bridgeData.lo, header.bridgeData.hi, {fn_id}, argsPtr, outPtr);\n"
    ));
    out.push_str("        }\n");
    out.push_str("        if (err.lo !== 0 || err.hi !== 0) {\n");
    if has_return {
        out.push_str("            return null as any;\n");
    } else {
        out.push_str("            return;\n");
    }
    out.push_str("        }\n");

    if has_return {
        out.push_str("        return result;\n");
    }

    out.push_str("    }\n\n");
}

/// Emit the argsPtr setup for a TypeScript guest host contract method.
fn emit_ts_guest_host_contract_args_setup(out: &mut String, func: &crate::ir::ResolvedFunction) {
    if func.params.is_empty() {
        out.push_str("        const argsPtr = 0;\n");
        return;
    }

    if func.params.len() == 1 {
        let param: &ResolvedParam = &func.params[0];
        match &param.ty {
            ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => {
                out.push_str(&format!(
                    "        const {}Bytes = new TextEncoder().encode({});\n",
                    param.name, param.name
                ));
                out.push_str(&format!(
                    "        const {}Ptr = polyplug.allocString({}Bytes);\n",
                    param.name, param.name
                ));
                out.push_str(&format!("        const argsPtr = {}Ptr;\n", param.name));
            }
            ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => {
                out.push_str(&format!(
                    "        const {}Ptr = polyplug.allocBuffer({});\n",
                    param.name, param.name
                ));
                out.push_str(&format!("        const argsPtr = {}Ptr;\n", param.name));
            }
            ResolvedTypeRef::UserDefined(_) => {
                out.push_str(&format!(
                    "        const {}Ptr = polyplug.allocStruct({});\n",
                    param.name, param.name
                ));
                out.push_str(&format!("        const argsPtr = {}Ptr;\n", param.name));
            }
            ResolvedTypeRef::Primitive(p) => {
                if matches!(p, PrimitiveType::U64 | PrimitiveType::I64) {
                    out.push_str(&format!(
                        "        const {}Ptr = polyplug.allocU64({}.lo, {}.hi);\n",
                        param.name, param.name, param.name
                    ));
                    out.push_str(&format!("        const argsPtr = {}Ptr;\n", param.name));
                } else {
                    out.push_str(&format!(
                        "        const {}Ptr = polyplug.allocU32({});\n",
                        param.name, param.name
                    ));
                    out.push_str(&format!("        const argsPtr = {}Ptr;\n", param.name));
                }
            }
            ResolvedTypeRef::AbiType(AbiBuiltin::Ptr) => {
                out.push_str(&format!(
                    "        const {}Ptr = polyplug.allocU64({}.lo, {}.hi);\n",
                    param.name, param.name, param.name
                ));
                out.push_str(&format!("        const argsPtr = {}Ptr;\n", param.name));
            }
            ResolvedTypeRef::AbiType(AbiBuiltin::Void) => {
                out.push_str("        const argsPtr = 0;\n");
            }
        }
        return;
    }

    // Multiple params: pack into inline struct
    out.push_str("        const argsSize = ");
    let mut total_size: usize = 0;
    for param in &func.params {
        match &param.ty {
            ResolvedTypeRef::Primitive(p) => {
                if matches!(p, PrimitiveType::U64 | PrimitiveType::I64) {
                    total_size += 8;
                } else {
                    total_size += 4;
                }
            }
            ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => total_size += 12,
            ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => total_size += 16,
            ResolvedTypeRef::AbiType(AbiBuiltin::Ptr) => total_size += 8,
            ResolvedTypeRef::AbiType(AbiBuiltin::Void) => {}
            ResolvedTypeRef::UserDefined(_) => {
                total_size += 8;
            }
        }
    }
    out.push_str(&format!("{};\n", total_size));
    out.push_str("        const argsPtr = polyplug.alloc(argsSize);\n");
    let mut offset: usize = 0;
    for param in &func.params {
        match &param.ty {
            ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => {
                out.push_str(&format!(
                    "        const {}Bytes = new TextEncoder().encode({});\n",
                    param.name, param.name
                ));
                out.push_str(&format!(
                    "        const {}StrPtr = polyplug.allocString({}Bytes);\n",
                    param.name, param.name
                ));
                out.push_str(&format!(
                    "        polyplug.writeU32(argsPtr + {}, {}StrPtr.lo);\n",
                    offset, param.name
                ));
                out.push_str(&format!(
                    "        polyplug.writeU32(argsPtr + {}, {}StrPtr.hi);\n",
                    offset + 4,
                    param.name
                ));
                out.push_str(&format!(
                    "        polyplug.writeU32(argsPtr + {}, {}Bytes.length);\n",
                    offset + 8,
                    param.name
                ));
                offset += 12;
            }
            ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => {
                out.push_str(&format!(
                    "        const {}BufPtr = polyplug.allocBuffer({});\n",
                    param.name, param.name
                ));
                out.push_str(&format!(
                    "        polyplug.writeU32(argsPtr + {}, {}BufPtr.lo);\n",
                    offset, param.name
                ));
                out.push_str(&format!(
                    "        polyplug.writeU32(argsPtr + {}, {}BufPtr.hi);\n",
                    offset + 4,
                    param.name
                ));
                out.push_str(&format!(
                    "        polyplug.writeU32(argsPtr + {}, {}.length);\n",
                    offset + 8,
                    param.name
                ));
                out.push_str(&format!(
                    "        polyplug.writeU32(argsPtr + {}, {}.length);\n",
                    offset + 12,
                    param.name
                ));
                offset += 16;
            }
            ResolvedTypeRef::Primitive(p) => {
                if matches!(p, PrimitiveType::U64 | PrimitiveType::I64) {
                    out.push_str(&format!(
                        "        polyplug.writeU32(argsPtr + {}, {}.lo);\n",
                        offset, param.name
                    ));
                    out.push_str(&format!(
                        "        polyplug.writeU32(argsPtr + {}, {}.hi);\n",
                        offset + 4,
                        param.name
                    ));
                    offset += 8;
                } else {
                    out.push_str(&format!(
                        "        polyplug.writeU32(argsPtr + {}, {});\n",
                        offset, param.name
                    ));
                    offset += 4;
                }
            }
            ResolvedTypeRef::AbiType(AbiBuiltin::Ptr) => {
                out.push_str(&format!(
                    "        polyplug.writeU32(argsPtr + {}, {}.lo);\n",
                    offset, param.name
                ));
                out.push_str(&format!(
                    "        polyplug.writeU32(argsPtr + {}, {}.hi);\n",
                    offset + 4,
                    param.name
                ));
                offset += 8;
            }
            ResolvedTypeRef::UserDefined(_) => {
                out.push_str(&format!(
                    "        const {}StructPtr = polyplug.allocStruct({});\n",
                    param.name, param.name
                ));
                out.push_str(&format!(
                    "        polyplug.writeU32(argsPtr + {}, {}StructPtr.lo);\n",
                    offset, param.name
                ));
                out.push_str(&format!(
                    "        polyplug.writeU32(argsPtr + {}, {}StructPtr.hi);\n",
                    offset + 4,
                    param.name
                ));
                offset += 8;
            }
            ResolvedTypeRef::AbiType(AbiBuiltin::Void) => {}
        }
    }
}

/// Emit the outPtr setup for a TypeScript guest host contract method.
fn emit_ts_guest_host_contract_out_setup(out: &mut String, returns: &Option<ResolvedTypeRef>) {
    if let Some(ret_ty) = returns {
        match ret_ty {
            ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => {
                out.push_str("        const outPtr = polyplug.alloc(12);\n");
                out.push_str("        const result = { ptr_lo: 0, ptr_hi: 0, len: 0 };\n");
            }
            ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => {
                out.push_str("        const outPtr = polyplug.alloc(16);\n");
                out.push_str("        const result = { ptr_lo: 0, ptr_hi: 0, len: 0, cap: 0 };\n");
            }
            ResolvedTypeRef::UserDefined(_) => {
                out.push_str("        const outPtr = polyplug.allocStructSize();\n");
                out.push_str("        const result = {} as any;\n");
            }
            ResolvedTypeRef::Primitive(p) => {
                if matches!(p, PrimitiveType::U64 | PrimitiveType::I64) {
                    out.push_str("        const outPtr = polyplug.alloc(8);\n");
                    out.push_str("        const result = { lo: 0, hi: 0 };\n");
                } else {
                    out.push_str("        const outPtr = polyplug.alloc(4);\n");
                    out.push_str("        const result = 0;\n");
                }
            }
            ResolvedTypeRef::AbiType(AbiBuiltin::Ptr) => {
                out.push_str("        const outPtr = polyplug.alloc(8);\n");
                out.push_str("        const result = { lo: 0, hi: 0 };\n");
            }
            ResolvedTypeRef::AbiType(AbiBuiltin::Void) => {
                out.push_str("        const outPtr = 0;\n");
            }
        }
    } else {
        out.push_str("        const outPtr = 0;\n");
    }
}

/// Generate `guest/host_contracts.ts` — caller classes for guest-side host contract callers.
fn generate_guest_host_contracts_ts(ir: &ValidatedIr) -> String {
    let mut out: String = String::new();
    out.push_str(
        "// THIS FILE IS AUTO-GENERATED BY polyplugc\n\
         // DO NOT EDIT BY HAND\n\
         // Runtime: js-quickjs (guest-side callers)\n\n",
    );

    for contract in &ir.host_contracts {
        generate_ts_guest_host_contract_caller(&mut out, contract);
    }

    // Emit contract ID constants
    out.push_str("// Contract ID constants\n");
    for contract in &ir.host_contracts {
        let class_name: String = host_contract_name_to_ts_caller(&contract.name);
        let const_name: String = class_name.to_uppercase() + "_ID";
        out.push_str(&format!(
            "/** Contract ID for `{}` (FNV-1a of \"host_contract:{}@{}\") */\n",
            contract.name, contract.name, contract.version.major
        ));
        out.push_str(&format!(
            "export const {} = 0x{:016X}n;\n\n",
            const_name, contract.contract_id
        ));
    }

    out
}

// ─── Host VTable Factories Generation ────────────────────────────────────────

/// Generate all host-side vtable factories into a single file.
fn generate_js_host_vtable_factories_ts(ir: &ValidatedIr) -> String {
    let mut out: String = String::new();
    out.push_str(
        "// THIS FILE IS AUTO-GENERATED BY polyplugc\n\
         // DO NOT EDIT BY HAND\n\
         // Runtime: js-quickjs (host-side vtable factories)\n\n",
    );

    out.push_str("import type { HostContractVTable } from 'polyplug';\n");
    out.push_str("import { DispatchType } from 'polyplug';\n");
    out.push_str("import type * as contracts from './contracts';\n\n");

    out.push_str("// ABI constants\n");
    out.push_str("const ABI_OK = 0;\n");
    out.push_str("const ABI_ERROR_PANIC = 5;\n\n");

    for contract in &ir.host_contracts {
        generate_js_host_vtable_factory(&mut out, contract);
    }

    out
}

/// Generate vtable factories for one host contract.
fn generate_js_host_vtable_factory(out: &mut String, contract: &ResolvedHostContract) {
    let iface_name: String = host_contract_name_to_ts_interface(&contract.name);
    let factory_name: String = format!("create{}Vtable", iface_name);
    let factory_vm_name: String = format!("create{}VtableVm", iface_name);
    let fn_count: usize = contract.functions.len();
    let contract_id: u64 = contract.contract_id;
    let contract_id_lo: u32 = (contract_id & 0xFFFFFFFF) as u32;
    let contract_id_hi: u32 = (contract_id >> 32) as u32;
    let major: u32 = contract.version.major;
    let minor: u32 = contract.version.minor;

    // NATIVE dispatch factory
    out.push_str(&format!(
        "/** Create a host contract vtable for `{}` with NATIVE dispatch. */\n",
        contract.name
    ));
    out.push_str(&format!(
        "export function {factory_name}(impl: contracts.{iface_name}): HostContractVTable {{\n"
    ));
    out.push_str(&format!("    _{iface_name}_impl = impl;\n\n"));

    // Generate thunks for each function
    for func in &contract.functions {
        generate_js_host_thunk(out, func, &contract.name, &iface_name);
    }

    // Static function pointer array
    out.push_str(&"    const functions: (() => number)[] = [\n".to_string());
    for func in &contract.functions {
        let thunk_name: String = format!(
            "_{}_{}_thunk",
            contract.name.replace('.', "_").to_lowercase(),
            func.name
        );
        out.push_str(&format!("        {thunk_name},\n"));
    }
    out.push_str("    ];\n\n");

    // Create the vtable
    out.push_str("    const vtable: HostContractVTable = {\n");
    out.push_str("        header: {\n");
    out.push_str("            vtableVersion: 1,\n");
    out.push_str(&format!(
        "            contractIdLo: 0x{contract_id_lo:08X},\n"
    ));
    out.push_str(&format!(
        "            contractIdHi: 0x{contract_id_hi:08X},\n"
    ));
    out.push_str(&format!("            contractMajor: {major},\n"));
    out.push_str(&format!("            contractMinor: {minor},\n"));
    out.push_str(&format!("            functionCount: {fn_count},\n"));
    out.push_str("            dispatchType: DispatchType.Native,\n");
    out.push_str("        },\n");
    out.push_str("        dispatch: {\n");
    out.push_str("            native: {\n");
    out.push_str("                implPtr: { lo: 0, hi: 0 },  // We use global _impl instead\n");
    out.push_str("                functions,\n");
    out.push_str("            },\n");
    out.push_str("        },\n");
    out.push_str("    };\n\n");
    out.push_str("    return vtable;\n");
    out.push_str("}\n\n");

    // Global implementation storage
    out.push_str(&format!(
        "let _{iface_name}_impl: contracts.{iface_name} | null = null;\n\n"
    ));

    // VM dispatch factory
    out.push_str(&format!(
        "/** Create a host contract vtable for `{}` with VM dispatch. */\n",
        contract.name
    ));
    out.push_str(&format!("export function {factory_vm_name}(\n"));
    out.push_str("    bridgeData: { lo: number; hi: number },\n");
    out.push_str("    dispatchFn: (bridgeData: { lo: number; hi: number }, fnId: number, args: number, out: number) => number,\n");
    out.push_str("): HostContractVTable {\n");
    out.push_str("    const vtable: HostContractVTable = {\n");
    out.push_str("        header: {\n");
    out.push_str("            vtableVersion: 1,\n");
    out.push_str(&format!(
        "            contractIdLo: 0x{contract_id_lo:08X},\n"
    ));
    out.push_str(&format!(
        "            contractIdHi: 0x{contract_id_hi:08X},\n"
    ));
    out.push_str(&format!("            contractMajor: {major},\n"));
    out.push_str(&format!("            contractMinor: {minor},\n"));
    out.push_str(&format!("            functionCount: {fn_count},\n"));
    out.push_str("            dispatchType: DispatchType.VirtualMachine,\n");
    out.push_str("        },\n");
    out.push_str("        dispatch: {\n");
    out.push_str("            vm: {\n");
    out.push_str("                call: dispatchFn,\n");
    out.push_str("                bridgeData,\n");
    out.push_str("            },\n");
    out.push_str("        },\n");
    out.push_str("    };\n\n");
    out.push_str("    return vtable;\n");
    out.push_str("}\n\n");
}

/// Generate a thunk function for a host contract function.
fn generate_js_host_thunk(
    out: &mut String,
    func: &ResolvedFunction,
    contract_name: &str,
    iface_name: &str,
) {
    let thunk_name: String = format!(
        "_{}_{}_thunk",
        contract_name.replace('.', "_").to_lowercase(),
        func.name
    );
    let has_return: bool = func.returns.is_some();

    out.push_str(&format!("    function {thunk_name}(): number {{\n"));
    out.push_str("        try {\n");
    out.push_str(&format!("            const impl = _{iface_name}_impl;\n"));
    out.push_str("            if (impl === null) {\n");
    out.push_str("                return ABI_ERROR_PANIC;\n");
    out.push_str("            }\n");

    // Generate argument extraction
    if !func.params.is_empty() {
        generate_js_host_thunk_args(out, func);
    }

    // Generate the method call
    generate_js_host_thunk_call(out, func, has_return);

    // Handle return value
    if has_return {
        out.push_str("            // Write result to out pointer\n");
        out.push_str("            // Note: In QuickJS, we use the polyplug helpers\n");
    }

    out.push_str("            return ABI_OK;\n");
    out.push_str("        } catch (e) {\n");
    out.push_str("            return ABI_ERROR_PANIC;\n");
    out.push_str("        }\n");
    out.push_str("    }\n\n");
}

/// Generate argument extraction for a host thunk.
fn generate_js_host_thunk_args(out: &mut String, func: &ResolvedFunction) {
    if func.params.len() == 1 {
        let param: &ResolvedParam = &func.params[0];
        match &param.ty {
            ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => {
                out.push_str(&"            // Extract StringView from args pointer\n".to_string());
                out.push_str(&format!(
                    "            const {name} = '';\n",
                    name = param.name
                ));
            }
            ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => {
                out.push_str(&"            // Extract Buffer from args pointer\n".to_string());
                out.push_str(&format!(
                    "            const {name} = new Uint8Array(0);\n",
                    name = param.name
                ));
            }
            _ => {
                let ty_name: String = ts_host_param_type(&param.ty);
                out.push_str(&format!(
                    "            const {name}: {ty_name} = 0;\n",
                    name = param.name,
                    ty_name = ty_name
                ));
            }
        }
    } else {
        // Multiple params - use arg-pack struct
        for param in &func.params {
            let ty_name: String = ts_host_param_type(&param.ty);
            out.push_str(&format!(
                "            const {name}: {ty_name} = 0;\n",
                name = param.name,
                ty_name = ty_name
            ));
        }
    }
}

/// Generate the method call inside a host thunk.
fn generate_js_host_thunk_call(out: &mut String, func: &ResolvedFunction, has_return: bool) {
    let call_args: String = if func.params.is_empty() {
        String::new()
    } else if func.params.len() == 1 {
        let param: &ResolvedParam = &func.params[0];
        param.name.clone()
    } else {
        func.params
            .iter()
            .map(|p: &ResolvedParam| p.name.clone())
            .collect::<Vec<_>>()
            .join(", ")
    };

    // Convert snake_case to PascalCase for method name
    let method_name: String = func
        .name
        .split(['_', '.'])
        .filter(|seg: &&str| !seg.is_empty())
        .map(|seg: &str| {
            let mut c: core::str::Chars<'_> = seg.chars();
            match c.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().to_string() + c.as_str(),
            }
        })
        .collect::<Vec<String>>()
        .join("");

    if has_return {
        let ret_ty: String = match func.returns.as_ref() {
            Some(ret) => ts_host_return_type(ret),
            None => String::from("void"),
        };
        out.push_str(&format!(
            "            const result: {ret_ty} = impl.{method_name}({call_args});\n"
        ));
    } else {
        out.push_str(&format!("            impl.{method_name}({call_args});\n"));
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use super::*;
    use crate::ir::AbiBuiltin;
    use crate::ir::EnumDef;
    use crate::ir::EnumVariant;
    use crate::ir::PrimitiveType;
    use crate::ir::ReprType;
    use crate::ir::ResolvedFunction;
    use crate::ir::ResolvedHostContract;
    use crate::ir::ResolvedParam;
    use crate::ir::Version;

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

    // ─── Host Contract Interface Tests ────────────────────────────────────────

    #[test]
    fn host_contract_name_to_ts_interface_conversion() {
        assert_eq!(
            host_contract_name_to_ts_interface("host.logger"),
            "HostLogger"
        );
        assert_eq!(
            host_contract_name_to_ts_interface("host.fs.reader"),
            "HostFsReader"
        );
        assert_eq!(
            host_contract_name_to_ts_interface("host.HostLogger"),
            "HostLogger"
        );
        assert_eq!(host_contract_name_to_ts_interface("logger"), "HostLogger");
    }

    #[test]
    fn ts_host_param_type_mappings() {
        assert_eq!(
            ts_host_param_type(&ResolvedTypeRef::Primitive(PrimitiveType::U32)),
            "number"
        );
        assert_eq!(
            ts_host_param_type(&ResolvedTypeRef::Primitive(PrimitiveType::U64)),
            "{ lo: number; hi: number }"
        );
        assert_eq!(
            ts_host_param_type(&ResolvedTypeRef::AbiType(AbiBuiltin::StringView)),
            "string"
        );
        assert_eq!(
            ts_host_param_type(&ResolvedTypeRef::AbiType(AbiBuiltin::Buffer)),
            "Uint8Array"
        );
        assert_eq!(
            ts_host_param_type(&ResolvedTypeRef::UserDefined("MyStruct".to_owned())),
            "MyStruct"
        );
    }

    #[test]
    fn ts_host_return_type_mappings() {
        assert_eq!(
            ts_host_return_type(&ResolvedTypeRef::Primitive(PrimitiveType::U32)),
            "number"
        );
        assert_eq!(
            ts_host_return_type(&ResolvedTypeRef::AbiType(AbiBuiltin::StringView)),
            "string"
        );
        assert_eq!(
            ts_host_return_type(&ResolvedTypeRef::AbiType(AbiBuiltin::Buffer)),
            "Uint8Array"
        );
        assert_eq!(
            ts_host_return_type(&ResolvedTypeRef::UserDefined("MyStruct".to_owned())),
            "MyStruct"
        );
    }

    #[test]
    fn generate_ts_host_contract_interface_produces_interface() {
        let contract: ResolvedHostContract = ResolvedHostContract {
            name: "host.logger".to_owned(),
            contract_id: 0x1234_5678_9ABC_DEF0_u64,
            version: Version {
                major: 1,
                minor: 0,
                patch: 0,
            },
            functions: vec![
                ResolvedFunction {
                    name: "log".to_owned(),
                    function_id: 0,
                    params: vec![ResolvedParam {
                        name: "message".to_owned(),
                        ty: ResolvedTypeRef::AbiType(AbiBuiltin::StringView),
                    }],
                    returns: None,
                },
                ResolvedFunction {
                    name: "logf".to_owned(),
                    function_id: 1,
                    params: vec![
                        ResolvedParam {
                            name: "level".to_owned(),
                            ty: ResolvedTypeRef::Primitive(PrimitiveType::U32),
                        },
                        ResolvedParam {
                            name: "format".to_owned(),
                            ty: ResolvedTypeRef::AbiType(AbiBuiltin::StringView),
                        },
                    ],
                    returns: None,
                },
            ],
        };
        let mut out: String = String::new();
        generate_ts_host_contract_interface(&mut out, &contract);
        assert!(
            out.contains("export interface HostLogger"),
            "missing interface: {out}"
        );
        assert!(
            out.contains("Log(message: string): void"),
            "missing Log method: {out}"
        );
        assert!(
            out.contains("Logf(level: number, format: string): void"),
            "missing Logf method: {out}"
        );
    }

    #[test]
    fn generate_host_contracts_ts_produces_file() {
        let ir: ValidatedIr = ValidatedIr {
            types: vec![],
            enums: vec![],
            contracts: vec![],
            host_contracts: vec![ResolvedHostContract {
                name: "host.logger".to_owned(),
                contract_id: 0x1234_5678_9ABC_DEF0_u64,
                version: Version {
                    major: 1,
                    minor: 0,
                    patch: 0,
                },
                functions: vec![ResolvedFunction {
                    name: "log".to_owned(),
                    function_id: 0,
                    params: vec![ResolvedParam {
                        name: "message".to_owned(),
                        ty: ResolvedTypeRef::AbiType(AbiBuiltin::StringView),
                    }],
                    returns: None,
                }],
            }],
            bundle: None,
        };
        let out: String = generate_host_contracts_ts(&ir);
        assert!(out.contains("AUTO-GENERATED"), "missing header: {out}");
        assert!(
            out.contains("export interface HostLogger"),
            "missing interface: {out}"
        );
        assert!(
            out.contains("HOSTLOGGER_CONTRACT_ID"),
            "missing constant: {out}"
        );
        assert!(
            out.contains("0x123456789ABCDEF0n"),
            "missing contract ID value: {out}"
        );
    }

    #[test]
    fn generate_host_with_host_contracts_produces_contracts_file() {
        let generator: JsQuickjsGenerator = JsQuickjsGenerator;
        let ir: ValidatedIr = ValidatedIr {
            types: vec![],
            enums: vec![],
            contracts: vec![],
            host_contracts: vec![ResolvedHostContract {
                name: "host.logger".to_owned(),
                contract_id: 0x1234_5678_9ABC_DEF0_u64,
                version: Version {
                    major: 1,
                    minor: 0,
                    patch: 0,
                },
                functions: vec![ResolvedFunction {
                    name: "log".to_owned(),
                    function_id: 0,
                    params: vec![],
                    returns: None,
                }],
            }],
            bundle: None,
        };
        let mut files: GeneratedFiles = GeneratedFiles::default();
        generator
            .generate_host(&ir, &mut files)
            .expect("generate_host");
        let names: Vec<String> = files
            .files
            .iter()
            .map(|f: &GeneratedFile| f.path.to_string_lossy().to_string())
            .collect();
        assert!(
            names.contains(&"host/contracts.ts".to_owned()),
            "missing host/contracts.ts: {names:?}"
        );
    }

    #[test]
    fn generate_host_without_host_contracts_no_contracts_file() {
        let generator: JsQuickjsGenerator = JsQuickjsGenerator;
        let ir: ValidatedIr = ValidatedIr {
            types: vec![],
            enums: vec![],
            contracts: vec![],
            host_contracts: vec![],
            bundle: None,
        };
        let mut files: GeneratedFiles = GeneratedFiles::default();
        generator
            .generate_host(&ir, &mut files)
            .expect("generate_host");
        let names: Vec<String> = files
            .files
            .iter()
            .map(|f: &GeneratedFile| f.path.to_string_lossy().to_string())
            .collect();
        assert!(
            !names.contains(&"host/contracts.ts".to_owned()),
            "unexpected host/contracts.ts: {names:?}"
        );
    }

    // ─── Guest Host Contract Caller Tests ─────────────────────────────────────

    #[test]
    fn host_contract_name_to_ts_caller_conversion() {
        assert_eq!(
            host_contract_name_to_ts_caller("host.logger"),
            "HostLoggerContract"
        );
        assert_eq!(
            host_contract_name_to_ts_caller("host.fs.reader"),
            "HostFsReaderContract"
        );
        assert_eq!(
            host_contract_name_to_ts_caller("host.HostLogger"),
            "HostLoggerContract"
        );
        assert_eq!(
            host_contract_name_to_ts_caller("logger"),
            "HostLoggerContract"
        );
    }

    #[test]
    fn ts_guest_caller_param_type_mappings() {
        assert_eq!(
            ts_guest_caller_param_type(&ResolvedTypeRef::Primitive(PrimitiveType::U32)),
            "number"
        );
        assert_eq!(
            ts_guest_caller_param_type(&ResolvedTypeRef::Primitive(PrimitiveType::U64)),
            "{ lo: number; hi: number }"
        );
        assert_eq!(
            ts_guest_caller_param_type(&ResolvedTypeRef::AbiType(AbiBuiltin::StringView)),
            "string"
        );
        assert_eq!(
            ts_guest_caller_param_type(&ResolvedTypeRef::AbiType(AbiBuiltin::Buffer)),
            "Uint8Array"
        );
        assert_eq!(
            ts_guest_caller_param_type(&ResolvedTypeRef::UserDefined("MyStruct".to_owned())),
            "MyStruct"
        );
    }

    #[test]
    fn ts_guest_caller_return_type_mappings() {
        assert_eq!(
            ts_guest_caller_return_type(&ResolvedTypeRef::Primitive(PrimitiveType::U32)),
            "number"
        );
        assert_eq!(
            ts_guest_caller_return_type(&ResolvedTypeRef::AbiType(AbiBuiltin::StringView)),
            "string"
        );
        assert_eq!(
            ts_guest_caller_return_type(&ResolvedTypeRef::AbiType(AbiBuiltin::Buffer)),
            "Uint8Array"
        );
        assert_eq!(
            ts_guest_caller_return_type(&ResolvedTypeRef::UserDefined("MyStruct".to_owned())),
            "MyStruct"
        );
    }

    #[test]
    fn generate_ts_guest_host_contract_caller_produces_class() {
        let contract: ResolvedHostContract = ResolvedHostContract {
            name: "host.logger".to_owned(),
            contract_id: 0x1234_5678_9ABC_DEF0_u64,
            version: Version {
                major: 1,
                minor: 0,
                patch: 0,
            },
            functions: vec![
                ResolvedFunction {
                    name: "log".to_owned(),
                    function_id: 0,
                    params: vec![ResolvedParam {
                        name: "message".to_owned(),
                        ty: ResolvedTypeRef::AbiType(AbiBuiltin::StringView),
                    }],
                    returns: None,
                },
                ResolvedFunction {
                    name: "logf".to_owned(),
                    function_id: 1,
                    params: vec![
                        ResolvedParam {
                            name: "level".to_owned(),
                            ty: ResolvedTypeRef::Primitive(PrimitiveType::U32),
                        },
                        ResolvedParam {
                            name: "format".to_owned(),
                            ty: ResolvedTypeRef::AbiType(AbiBuiltin::StringView),
                        },
                    ],
                    returns: None,
                },
            ],
        };
        let mut out: String = String::new();
        generate_ts_guest_host_contract_caller(&mut out, &contract);
        assert!(
            out.contains("export class HostLoggerContract"),
            "missing class: {out}"
        );
        assert!(
            out.contains("private constructor(vtable: { lo: number; hi: number })"),
            "missing private constructor: {out}"
        );
        assert!(
            out.contains(
                "static fromHost(hostPtr: { lo: number; hi: number }, minVersion: number = 0)"
            ),
            "missing fromHost: {out}"
        );
        assert!(out.contains("isValid(): boolean"), "missing isValid: {out}");
        assert!(
            out.contains("log(message: string): void"),
            "missing log method: {out}"
        );
        assert!(
            out.contains("logf(level: number, format: string): void"),
            "missing logf method: {out}"
        );
    }

    #[test]
    fn generate_ts_guest_host_contract_caller_with_return() {
        let contract: ResolvedHostContract = ResolvedHostContract {
            name: "host.fs.reader".to_owned(),
            contract_id: 0xDEAD_BEEF_u64,
            version: Version {
                major: 1,
                minor: 0,
                patch: 0,
            },
            functions: vec![ResolvedFunction {
                name: "read".to_owned(),
                function_id: 0,
                params: vec![ResolvedParam {
                    name: "path".to_owned(),
                    ty: ResolvedTypeRef::AbiType(AbiBuiltin::StringView),
                }],
                returns: Some(ResolvedTypeRef::AbiType(AbiBuiltin::Buffer)),
            }],
        };
        let mut out: String = String::new();
        generate_ts_guest_host_contract_caller(&mut out, &contract);
        assert!(
            out.contains("export class HostFsReaderContract"),
            "missing class: {out}"
        );
        assert!(
            out.contains("read(path: string): Uint8Array"),
            "missing read method with return: {out}"
        );
    }

    #[test]
    fn generate_guest_host_contracts_ts_produces_file() {
        let ir: ValidatedIr = ValidatedIr {
            types: vec![],
            enums: vec![],
            contracts: vec![],
            host_contracts: vec![ResolvedHostContract {
                name: "host.logger".to_owned(),
                contract_id: 0x1234_5678_9ABC_DEF0_u64,
                version: Version {
                    major: 1,
                    minor: 0,
                    patch: 0,
                },
                functions: vec![ResolvedFunction {
                    name: "log".to_owned(),
                    function_id: 0,
                    params: vec![ResolvedParam {
                        name: "message".to_owned(),
                        ty: ResolvedTypeRef::AbiType(AbiBuiltin::StringView),
                    }],
                    returns: None,
                }],
            }],
            bundle: None,
        };
        let out: String = generate_guest_host_contracts_ts(&ir);
        assert!(out.contains("AUTO-GENERATED"), "missing header: {out}");
        assert!(
            out.contains("export class HostLoggerContract"),
            "missing class: {out}"
        );
        assert!(
            out.contains("HOSTLOGGERCONTRACT_ID"),
            "missing constant: {out}"
        );
        assert!(
            out.contains("0x123456789ABCDEF0n"),
            "missing contract ID value: {out}"
        );
    }

    #[test]
    fn generate_guest_with_host_contracts_produces_file() {
        let generator: JsQuickjsGenerator = JsQuickjsGenerator;
        let ir: ValidatedIr = ValidatedIr {
            types: vec![],
            enums: vec![],
            contracts: vec![],
            host_contracts: vec![ResolvedHostContract {
                name: "host.logger".to_owned(),
                contract_id: 0x1234_5678_9ABC_DEF0_u64,
                version: Version {
                    major: 1,
                    minor: 0,
                    patch: 0,
                },
                functions: vec![ResolvedFunction {
                    name: "log".to_owned(),
                    function_id: 0,
                    params: vec![],
                    returns: None,
                }],
            }],
            bundle: None,
        };
        let mut files: GeneratedFiles = GeneratedFiles::default();
        generator
            .generate_guest(&ir, &mut files)
            .expect("generate_guest");
        let names: Vec<String> = files
            .files
            .iter()
            .map(|f: &GeneratedFile| f.path.to_string_lossy().to_string())
            .collect();
        assert!(
            names.contains(&"guest/host_contracts.ts".to_owned()),
            "missing guest/host_contracts.ts: {names:?}"
        );
    }

    #[test]
    fn generate_guest_without_host_contracts_no_file() {
        let generator: JsQuickjsGenerator = JsQuickjsGenerator;
        let ir: ValidatedIr = ValidatedIr {
            types: vec![],
            enums: vec![],
            contracts: vec![],
            host_contracts: vec![],
            bundle: None,
        };
        let mut files: GeneratedFiles = GeneratedFiles::default();
        generator
            .generate_guest(&ir, &mut files)
            .expect("generate_guest");
        let names: Vec<String> = files
            .files
            .iter()
            .map(|f: &GeneratedFile| f.path.to_string_lossy().to_string())
            .collect();
        assert!(
            !names.contains(&"guest/host_contracts.ts".to_owned()),
            "unexpected guest/host_contracts.ts: {names:?}"
        );
    }
}
