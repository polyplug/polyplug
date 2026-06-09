//! js_quickjs generator — produces QuickJS-compatible TypeScript/JavaScript guest code.
//!
//! THIS FILE IS PART OF polyplugc.
//! Generates code using lo/hi u32 split for all u64/pointer values.

use std::collections::BTreeSet;
use std::path::PathBuf;

use super::CodeGenerator;
use super::GeneratedFile;
use super::GeneratedFiles;
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
use polyplug_codegen::PolyplugcError;

/// Generator for js-quickjs plugin bundles.
///
/// Produces TypeScript files using lo/hi u32 pairs for 64-bit values
/// (QuickJS uses f64 internally, so bigint is not available).
pub(crate) struct JsQuickjsGenerator;

impl CodeGenerator for JsQuickjsGenerator {
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
            // Emit host/interface_factories.ts if there are host contracts
            files.files.push(GeneratedFile {
                path: PathBuf::from("host/interface_factories.ts"),
                content: generate_js_host_interface_factories_ts(ir),
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
            path: PathBuf::from("guest/interface.ts"),
            content: generate_interface_ts(ir),
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
        // ── peer_callers.ts ────────────────────────────────────────────────────
        let peer_contracts: Vec<&ResolvedContract> = collect_peer_contracts(ir);
        if !peer_contracts.is_empty() {
            files.files.push(GeneratedFile {
                path: PathBuf::from("guest/peer_callers.ts"),
                content: generate_guest_peer_callers_ts(ir, &peer_contracts),
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
    out.push_str(&format!("export const {} = Object.freeze({{\n", e.name));
    for variant in &e.variants {
        let subst_value: String = substitute_variant_refs_js(&e.variants, &variant.value);
        out.push_str(&format!("    {}: {},\n", variant.name, subst_value));
    }
    out.push_str("} as const);\n");
    out.push_str(&format!(
        "export type {} = typeof {}[keyof typeof {}];\n\n",
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
                    render_plugin_interface_quickjs(&mut out, &plugin.name, contract)?;
                }
            }
        }
    }

    Ok(out)
}

fn render_plugin_interface_quickjs(
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

    out.push_str(&format!("\nexport const {plugin_var}_INTERFACE = {{\n"));
    out.push_str(&format!("    contractLo: 0x{:08X},\n", contract_lo));
    out.push_str(&format!("    contractHi: 0x{:08X},\n", contract_hi));
    out.push_str("    dispatchType: DispatchType.VirtualMachine,\n");
    // Instance lifecycle stubs
    out.push_str(&format!(
        "    // Default create_instance stub for {} - returns null instance.\n",
        plugin_name
    ));
    out.push_str("    createInstance: function(rtCtxLo: number, rtCtxHi: number, argsLo: number, argsHi: number): { dataLo: number; dataHi: number } {\n");
    out.push_str(
        "        // Default stub returns null instance - users override for stateful plugins.\n",
    );
    out.push_str("        return { dataLo: 0, dataHi: 0 };  // Null GuestContractInstance.\n");
    out.push_str("    },\n");
    out.push_str(&format!(
        "    // Default destroy_instance stub for {} - no-op.\n",
        plugin_name
    ));
    out.push_str(
        "    destroyInstance: function(rtCtxLo: number, rtCtxHi: number, instanceDataLo: number, instanceDataHi: number): void {\n",
    );
    out.push_str(
        "        // Default stub is no-op - users override for cleanup before hot-reload.\n",
    );
    out.push_str("    },\n");
    out.push_str(&format!("    fnCount: {function_count},\n"));
    out.push_str("    functions: [] as ((args_ptr: number, out_ptr: number) => number)[],\n");
    out.push_str(&format!("    contractName: \"{contract_name_full}\",\n"));
    // Packed contract version: the loader recovers `major = version >> 16`, so the
    // major version is encoded in the high 16 bits. This threads the contract's real
    // version into GuestContractInterface.contract_version / PluginDescriptor.version,
    // instead of the loader's hardcoded 0.0.0.
    out.push_str(&format!("    version: 0x{:08X},\n", version_major << 16));
    out.push_str("};\n");

    out.push_str(&format!("\nexport const {plugin_var}_DESCRIPTOR = {{\n"));
    out.push_str(&format!("    name: \"{plugin_name}\",\n"));
    out.push_str(&format!("    contractName: \"{contract_name_full}\",\n"));
    out.push_str(&format!("    version: {{ major: {version_major}, minor: {version_minor}, patch: {version_patch} }}\n"));
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

        // Generate the ABI wrapper function.
        // The loader dispatches via js_dispatch which passes two f64 values:
        //   args_ptr — full address of the packed args buffer (as Number/f64)
        //   out_ptr  — full address of the output StringView buffer (as Number/f64)
        // User-space addresses are < 2^48 < 2^53 (float64 mantissa), so the
        // usize→f64→usize round-trip is exact. readU32/writeU32 also accept f64.
        out.push_str("\nfunction ");
        out.push_str(&wrapper_name);
        out.push_str("(args_ptr: number, out_ptr: number): number {\n");
        out.push_str("    // SAFETY: args_ptr and out_ptr are valid addresses passed as f64\n");
        out.push_str("    // by the loader. readU32/writeU32 accept f64 and convert to usize.\n");
        out.push_str("    var polyplug = (globalThis as any).polyplug;\n");
        out.push_str("    if (!polyplug) return 1;\n");
        out.push_str("    var impl = ");
        out.push_str(&plugin_var);
        out.push_str("_IMPL;\n");
        out.push_str("    if (!impl) return 1;\n");

        if has_params {
            out.push_str("    if (!args_ptr) return 8;\n");
        }
        if has_return {
            out.push_str("    if (!out_ptr) return 8;\n");
        }

        // StringView in the args buffer: { ptr_lo: u32, ptr_hi: u32, len: u32 } = 12 bytes.
        out.push_str("    // SAFETY: readU32 reads 4 bytes from a valid host-allocated buffer.\n");
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

        // Write output StringView back through out_ptr.
        out.push_str("    // SAFETY: out_ptr is a valid host-allocated StringView buffer.\n");
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
    out.push_str("_IMPL: { [fn: string]: (...args: any[]) => any } | null = null;\n");

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

    // Store ABI wrappers in interface
    out.push_str("    ");
    out.push_str(&plugin_var);
    out.push_str("_INTERFACE.functions = [");
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

fn generate_interface_ts(ir: &ValidatedIr) -> String {
    let bundle: Option<&ResolvedBundle> = ir.bundle.as_ref();

    let mut out: String = String::new();
    out.push_str(
        "// THIS FILE IS AUTO-GENERATED BY polyplugc\n\
         // DO NOT EDIT BY HAND\n\
         // Runtime: js-quickjs\n\n",
    );

    if let Some(bundle) = bundle {
        // Re-export interfaces from contracts.ts
        out.push_str("// Re-export interfaces from contracts.ts\n");
        for plugin in &bundle.plugins {
            let plugin_var: String = plugin.name.to_uppercase().replace(['.', '-'], "_");
            out.push_str(&format!(
                "export {{ {plugin_var}_INTERFACE }} from './contracts';\n"
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
                "export {{ {plugin_var}_INTERFACE }} from './contracts';\n"
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

    // Re-export peer caller classes when the bundle declares dependencies.
    let peer_contracts: Vec<&ResolvedContract> = collect_peer_contracts(ir);
    for contract in &peer_contracts {
        let class_name: String = guest_contract_name_to_ts_peer(&contract.name);
        out.push_str(&format!(
            "export {{ {class_name} }} from './peer_callers';\n"
        ));
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
        out.push_str(&format!("    {plugin_var}_INTERFACE"));
    }
    out.push_str("\n} from './contracts';\n\n");
    out.push_str("// Inline host vtable storage — replaces 'polyplug-guest' import\n");
    out.push_str(
        "// because QuickJS loader exposes 'polyplug' global, not 'polyplug-guest' module.\n",
    );
    out.push_str("function storeHostVtable(lo: number, hi: number): void {\n");
    out.push_str("    (globalThis as any).polyplug._hostVtableLo = lo;\n");
    out.push_str("    (globalThis as any).polyplug._hostVtableHi = hi;\n");
    out.push_str("}\n\n");
    out.push_str("// ABI error codes (match polyplug_abi.AbiErrorCode)\n");
    out.push_str("const AbiErrorCode = {\n");
    out.push_str("    Ok: 0,\n");
    out.push_str("    Generic: 1,\n");
    out.push_str("    InvalidPointer: 8,\n");
    out.push_str("};\n\n");

    out.push_str("interface AbiError {\n");
    out.push_str("    code: number;\n");
    out.push_str("    message: { ptr: number; len: number };\n");
    out.push_str("}\n\n");

    out.push_str("/**\n");
    out.push_str(" * Initialize plugin with host runtime.\n");
    out.push_str(" * @param host_lo - HostApi pointer (low 32 bits)\n");
    out.push_str(" * @param host_hi - HostApi pointer (high 32 bits)\n");
    out.push_str(" * @param ctx_lo - BundleInitContext pointer (low 32 bits)\n");
    out.push_str(" * @param ctx_hi - BundleInitContext pointer (high 32 bits)\n");
    out.push_str(" */\n");
    out.push_str("export function polyplug_init(\n");
    out.push_str("    host_lo: number, host_hi: number,\n");
    out.push_str("    ctx_lo: number, ctx_hi: number\n");
    out.push_str("): AbiError {\n");
    out.push_str("    // Validate parameters\n");
    out.push_str("    if (host_lo === 0 && host_hi === 0) {\n");
    out.push_str("        return { code: AbiErrorCode.Generic, message: { ptr: 0, len: 0 } };\n");
    out.push_str("    }\n");
    out.push_str("    if (ctx_lo === 0 && ctx_hi === 0) {\n");
    out.push_str("        return { code: AbiErrorCode.Generic, message: { ptr: 0, len: 0 } };\n");
    out.push_str("    }\n\n");
    out.push_str("    // Store host interface for later access via getHostVtable()\n");
    out.push_str("    storeHostVtable(host_lo, host_hi);\n\n");
    out.push_str("    // Get polyplug host interface from globalThis\n");
    out.push_str("    const polyplug = (globalThis as any).polyplug;\n");
    out.push_str("    if (!polyplug || !polyplug.registerVtable) {\n");
    out.push_str("        return { code: AbiErrorCode.Generic, message: { ptr: 0, len: 0 } };\n");
    out.push_str("    }\n\n");

    for plugin in &bundle.plugins {
        let plugin_var: String = plugin.name.to_uppercase().replace(['.', '-'], "_");
        out.push_str(&format!(
            "    // Register plugin: {plugin_name}\n",
            plugin_name = plugin.name
        ));
        out.push_str("    polyplug.registerVtable(\n");
        out.push_str(&format!("        {plugin_var}_INTERFACE.contractLo,\n"));
        out.push_str(&format!("        {plugin_var}_INTERFACE.contractHi,\n"));
        out.push_str(&format!("        {plugin_var}_INTERFACE,\n"));
        out.push_str(&format!("        {plugin_var}_INTERFACE.fnCount,\n"));
        out.push_str(&format!("        {plugin_var}_INTERFACE.contractName,\n"));
        out.push_str(&format!("        {plugin_var}_INTERFACE.version\n"));
        out.push_str("    );\n\n");
    }

    out.push_str("    return { code: AbiErrorCode.Ok, message: { ptr: 0, len: 0 } };\n");
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

    let dep_toml: String = super::emit_manifest_dependencies(&bundle.dependencies);

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
         // Runtime: js (Deno host callers over the polyplug Deno FFI SDK)\n\n",
    );

    // The host callers run under Deno against the polyplug Deno FFI host SDK.
    // They resolve a contract via the runtime, decode the raw
    // `GuestContractInterface*` through the SDK's `GuestContractInterfaceView`,
    // and dispatch directly through the interface function pointers (native or VM).
    //
    // The SDK types are declared structurally here (rather than imported from the
    // 'polyplug' module) so the generated file type-checks standalone under
    // `deno check` regardless of the host's import map / directory depth. At
    // runtime the caller is constructed with the real SDK `Runtime` instance.
    out.push_str("// Structural SDK types (see sdks/js/host/polyplug/mod.js).\n");
    out.push_str("interface GuestContractInterfaceView {\n");
    out.push_str("    isValid(): boolean;\n");
    out.push_str("    functionCount(): number;\n");
    out.push_str("    createInstance(): Uint8Array;\n");
    out.push_str("    destroyInstance(instance: Uint8Array): void;\n");
    out.push_str(
        "    dispatch(slot: number, instance: Uint8Array, argsPtr: Deno.PointerValue, outPtr: Deno.PointerValue): number;\n",
    );
    out.push_str("}\n");
    out.push_str("interface Runtime {\n");
    out.push_str("    findGuestContract(contractId: bigint, minVersion?: number): number;\n");
    out.push_str(
        "    resolveGuestContractInterface(handle: number): GuestContractInterfaceView | null;\n",
    );
    out.push_str("    alloc(size: number, align?: number): Deno.PointerValue;\n");
    out.push_str("    free(ptr: Deno.PointerValue, size: number, align?: number): void;\n");
    out.push_str("}\n\n");

    // ABI error codes (match polyplug_abi.AbiErrorCode)
    out.push_str("// ABI error codes (match polyplug_abi.AbiErrorCode)\n");
    out.push_str("export const AbiErrorCode = {\n");
    out.push_str("    Ok: 0,\n");
    out.push_str("    Generic: 1,\n");
    out.push_str("    InvalidPointer: 8,\n");
    out.push_str("} as const;\n\n");

    // StringView is a 16-byte `{ ptr: u64, len: usize }` struct at the ABI boundary.
    out.push_str("// StringView layout: { ptr: u64 @ 0, len: usize @ 8 } = 16 bytes\n");
    out.push_str("const STRING_VIEW_SIZE = 16;\n");
    out.push_str("const _encoder = new TextEncoder();\n");
    out.push_str("const _decoder = new TextDecoder();\n\n");

    // Contract ID constants (full u64 as bigint, matching the SDK's findGuestContract).
    out.push_str("// Contract ID constants (FNV-1a of \"guest_contract:name@major\")\n");
    for contract in &ir.contracts {
        let upper_name: String = contract.name.to_uppercase().replace(['.', '-'], "_");
        out.push_str(&format!(
            "/** Contract ID for `{}@{}` */\n",
            contract.name, contract.version.major
        ));
        out.push_str(&format!(
            "export const {}_CONTRACT_ID = 0x{:016X}n;\n",
            upper_name, contract.contract_id
        ));
    }
    out.push('\n');

    for contract in &ir.contracts {
        generate_host_caller_class_quickjs(&mut out, contract);
    }

    out
}

fn generate_host_caller_class_quickjs(out: &mut String, contract: &ResolvedContract) {
    let class_name: String = contract_to_class_name(&contract.name);
    let contract_upper: String = contract.name.to_uppercase().replace(['.', '-'], "_");
    let contract_id_const: String = format!("{}_CONTRACT_ID", contract_upper);

    out.push_str(&format!(
        "/** Host caller for contract `{}` over the Deno FFI SDK. */\n",
        contract.name
    ));
    out.push_str(&format!("export class {}Contract {{\n", class_name));
    out.push_str("    #rt: Runtime;\n");
    out.push_str("    #view: GuestContractInterfaceView;\n");
    out.push_str("    #instance: Uint8Array;\n\n");

    out.push_str(
        "    private constructor(rt: Runtime, view: GuestContractInterfaceView, instance: Uint8Array) {\n",
    );
    out.push_str("        this.#rt = rt;\n");
    out.push_str("        this.#view = view;\n");
    out.push_str("        this.#instance = instance;\n");
    out.push_str("    }\n\n");

    out.push_str(
        "    /** Resolve the contract and create an instance, or null if unavailable. */\n",
    );
    out.push_str(&format!(
        "    static create(rt: Runtime): {}Contract | null {{\n",
        class_name
    ));
    out.push_str(&format!(
        "        const handle = rt.findGuestContract({}, 0);\n",
        contract_id_const
    ));
    out.push_str("        const view = rt.resolveGuestContractInterface(handle);\n");
    // Validity keys off the interface pointer, never off instance data.
    out.push_str("        if (view === null || !view.isValid()) {\n");
    out.push_str("            return null;\n");
    out.push_str("        }\n");
    // A null instance.data is a VALID dispatch token for stateless contracts.
    out.push_str("        const instance = view.createInstance();\n");
    out.push_str(&format!(
        "        return new {}Contract(rt, view, instance);\n",
        class_name
    ));
    out.push_str("    }\n\n");

    out.push_str("    /** True while the resolved interface pointer is valid. */\n");
    out.push_str("    isValid(): boolean {\n");
    out.push_str("        return this.#view.isValid();\n");
    out.push_str("    }\n\n");

    out.push_str("    /** Destroy the instance via the interface `destroy_instance`. */\n");
    out.push_str("    destroy(): void {\n");
    out.push_str("        this.#view.destroyInstance(this.#instance);\n");
    out.push_str("    }\n\n");

    for func in &contract.functions {
        generate_host_caller_method_deno(out, func);
    }

    out.push_str("}\n\n");
}

/// Generate one Deno host-caller method.
///
/// The example contracts are all `(input: StringView) -> StringView`. This emits
/// a `fn(input: string): string` caller that allocates the input StringView via
/// the host allocator, dispatches through the SDK view, then reads/frees the
/// returned StringView. Functions outside the single-StringView-in/StringView-out
/// shape emit a guarded stub so codegen never produces ABI-incorrect callers.
fn generate_host_caller_method_deno(out: &mut String, func: &ResolvedFunction) {
    let fn_id: u32 = func.function_id;
    let is_string_in_string_out: bool = func.params.len() == 1
        && matches!(
            func.params[0].ty,
            ResolvedTypeRef::AbiType(AbiBuiltin::StringView)
        )
        && matches!(
            func.returns,
            Some(ResolvedTypeRef::AbiType(AbiBuiltin::StringView))
        );

    if !is_string_in_string_out {
        out.push_str(&format!(
            "    /** Call `{}` (unsupported caller shape in this SDK target). */\n",
            func.name
        ));
        out.push_str(&format!("    {}(): never {{\n", func.name));
        out.push_str(&format!(
            "        throw new Error('caller `{}` shape not supported by the Deno host SDK');\n",
            func.name
        ));
        out.push_str("    }\n\n");
        return;
    }

    let param_name: &str = &func.params[0].name;

    out.push_str(&format!("    /** Call `{}` */\n", func.name));
    out.push_str(&format!(
        "    {}({}: string): string {{\n",
        func.name, param_name
    ));

    // Validate function index against the interface's reported function count.
    out.push_str(&format!(
        "        if ({fn_id} >= this.#view.functionCount()) {{\n"
    ));
    out.push_str(&format!(
        "            throw new Error('function `{}` not available in interface');\n",
        func.name
    ));
    out.push_str("        }\n");

    // Encode the input string and copy it into a host-allocated buffer.
    out.push_str(&format!(
        "        const {param_name}Bytes = _encoder.encode({param_name});\n"
    ));
    out.push_str(&format!(
        "        const {param_name}Ptr = this.#rt.alloc({param_name}Bytes.length, 1);\n"
    ));
    out.push_str(&format!("        if ({param_name}Ptr === null) {{\n"));
    out.push_str(&format!(
        "            throw new Error('host_alloc failed for `{}` input');\n",
        func.name
    ));
    out.push_str("        }\n");
    out.push_str(&format!("        if ({param_name}Bytes.length > 0) {{\n"));
    out.push_str(&format!(
        "            const dst = new Uint8Array(Deno.UnsafePointerView.getArrayBuffer({param_name}Ptr, {param_name}Bytes.length));\n"
    ));
    out.push_str(&format!("            dst.set({param_name}Bytes);\n"));
    out.push_str("        }\n");

    // Build the input StringView struct { ptr, len } (16 bytes).
    out.push_str("        const argsBuf = new Uint8Array(STRING_VIEW_SIZE);\n");
    out.push_str("        const argsDv = new DataView(argsBuf.buffer);\n");
    out.push_str(&format!(
        "        argsDv.setBigUint64(0, BigInt(Deno.UnsafePointer.value({param_name}Ptr)), true);\n"
    ));
    out.push_str(&format!(
        "        argsDv.setBigUint64(8, BigInt({param_name}Bytes.length), true);\n"
    ));
    out.push_str("        const argsPtr = Deno.UnsafePointer.of(argsBuf);\n");

    // Build the output StringView buffer (16 bytes, zeroed).
    out.push_str("        const outBuf = new Uint8Array(STRING_VIEW_SIZE);\n");
    out.push_str("        const outPtr = Deno.UnsafePointer.of(outBuf);\n");

    // Dispatch through the SDK view (call_guest_method).
    out.push_str(&format!(
        "        const code = this.#view.dispatch({fn_id}, this.#instance, argsPtr, outPtr);\n"
    ));

    // Release the input buffer regardless of outcome.
    out.push_str(&format!(
        "        this.#rt.free({param_name}Ptr, {param_name}Bytes.length, 1);\n"
    ));

    out.push_str("        if (code !== AbiErrorCode.Ok) {\n");
    out.push_str(&format!(
        "            throw new Error('call `{}` failed: AbiError code ' + code);\n",
        func.name
    ));
    out.push_str("        }\n");

    // Read the returned StringView { ptr, len }, decode it, then free it.
    out.push_str("        const outDv = new DataView(outBuf.buffer);\n");
    out.push_str("        const resPtrRaw = outDv.getBigUint64(0, true);\n");
    out.push_str("        const resLen = Number(outDv.getBigUint64(8, true));\n");
    out.push_str("        if (resPtrRaw === 0n || resLen === 0) {\n");
    out.push_str("            return '';\n");
    out.push_str("        }\n");
    out.push_str("        const resPtr = Deno.UnsafePointer.create(resPtrRaw);\n");
    out.push_str("        if (resPtr === null) {\n");
    out.push_str("            return '';\n");
    out.push_str("        }\n");
    out.push_str(
        "        const resBytes = new Uint8Array(Deno.UnsafePointerView.getArrayBuffer(resPtr, resLen)).slice();\n",
    );
    out.push_str("        const result = _decoder.decode(resBytes);\n");
    // The returned StringView was allocated by the guest via host_alloc (align 1).
    out.push_str("        this.#rt.free(resPtr, resLen, 1);\n");
    out.push_str("        return result;\n");

    out.push_str("    }\n\n");
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

/// Raw ABI return type for peer caller methods — must match the `result` shape
/// that `emit_ts_guest_host_contract_out_setup` initialises in the generated body.
/// Peer callers return raw ABI values; the caller is responsible for decoding.
fn ts_peer_raw_return_type(ty: &ResolvedTypeRef) -> String {
    match ty {
        ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => {
            "{ ptr_lo: number; ptr_hi: number; len: number }".to_owned()
        }
        ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => {
            "{ ptr_lo: number; ptr_hi: number; len: number; cap: number }".to_owned()
        }
        ResolvedTypeRef::AbiType(AbiBuiltin::Ptr) => "{ lo: number; hi: number }".to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::Void) => "void".to_owned(),
        ResolvedTypeRef::Primitive(p) => {
            if matches!(p, PrimitiveType::U64 | PrimitiveType::I64) {
                "{ lo: number; hi: number }".to_owned()
            } else {
                "number".to_owned()
            }
        }
        ResolvedTypeRef::UserDefined(name) => name.clone(),
    }
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
    out.push_str("    private _minVersion: number;\n\n");

    out.push_str("    private constructor(minVersion: number) {\n");
    out.push_str("        this._minVersion = minVersion;\n");
    out.push_str("    }\n\n");

    out.push_str("    /** Factory method - creates caller instance or null if the bridge is unavailable. */\n");
    out.push_str(&format!(
        "    static fromHost(hostPtr: {{ lo: number; hi: number }}, minVersion: number = 0): {} | null {{\n",
        class_name
    ));
    out.push_str("        const polyplug = (globalThis as any).polyplug;\n");
    out.push_str("        if (!polyplug || !polyplug.callHostContract) {\n");
    out.push_str("            return null;\n");
    out.push_str("        }\n");
    out.push_str(&format!("        return new {}(minVersion);\n", class_name));
    out.push_str("    }\n\n");

    out.push_str("    /** Check if the bridge is available. */\n");
    out.push_str("    isValid(): boolean {\n");
    out.push_str("        const polyplug = (globalThis as any).polyplug;\n");
    out.push_str("        return !!(polyplug && polyplug.callHostContract);\n");
    out.push_str("    }\n\n");

    for func in &contract.functions {
        generate_ts_guest_host_contract_method(out, func, contract_id_lo, contract_id_hi);
    }

    out.push_str("}\n\n");
}

/// Generate one method for a guest-side host contract caller.
fn generate_ts_guest_host_contract_method(
    out: &mut String,
    func: &crate::ir::ResolvedFunction,
    contract_id_lo: u32,
    contract_id_hi: u32,
) {
    let fn_id: u32 = func.function_id;
    // Returns are RAW ABI values — declared type must match the shape that
    // emit_ts_guest_host_contract_readback produces (e.g. StringView →
    // {ptr_lo,ptr_hi,len}), not the ergonomic string/Uint8Array.
    let return_type: String = match &func.returns {
        Some(ty) => ts_peer_raw_return_type(ty),
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

    out.push_str("        const polyplug = (globalThis as any).polyplug;\n");
    out.push_str("        if (!polyplug || !polyplug.callHostContract) {\n");
    if has_return {
        out.push_str("            return null as any;\n");
    } else {
        out.push_str("            return;\n");
    }
    out.push_str("        }\n");

    emit_ts_guest_host_contract_args_setup(out, func);
    emit_ts_guest_host_contract_out_setup(out, &func.returns);

    out.push_str(&format!(
        "        const errCode: number = polyplug.callHostContract(0x{:08X}, 0x{:08X}, this._minVersion, {}, argsPtr, outPtr);\n",
        contract_id_lo, contract_id_hi, fn_id
    ));
    out.push_str("        if (errCode !== 0) {\n");
    if has_return {
        out.push_str("            return null as any;\n");
    } else {
        out.push_str("            return;\n");
    }
    out.push_str("        }\n");

    if has_return {
        emit_ts_guest_host_contract_readback(out, func.returns.as_ref());
        out.push_str("        return result;\n");
    }

    out.push_str("    }\n\n");
}

/// Return the packed ABI byte-size of a single resolved type (for args struct packing).
fn abi_sizeof(ty: &ResolvedTypeRef) -> usize {
    match ty {
        ResolvedTypeRef::Primitive(p) => {
            if matches!(p, PrimitiveType::U64 | PrimitiveType::I64) {
                8
            } else {
                4
            }
        }
        ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => 16,
        ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => 24,
        ResolvedTypeRef::AbiType(AbiBuiltin::Ptr) => 8,
        ResolvedTypeRef::AbiType(AbiBuiltin::Void) => 0,
        // UserDefined types in guest caller context are enum-backed scalars (u32).
        ResolvedTypeRef::UserDefined(_) => 4,
    }
}

/// Emit a StringView into `buf` at `offset` using arenaAlloc + writeU32/writeByte.
/// Defines `_<name>DataBuf` and `_<name>DataPtr` locals; `buf` is the combined args pointer.
fn emit_ts_write_string_view(out: &mut String, param_name: &str, args_ptr: &str, offset: usize) {
    let n: &str = param_name;
    let ap: &str = args_ptr;
    out.push_str(&format!(
        "        const _{n}Bytes = new TextEncoder().encode({n});\n"
    ));
    out.push_str(&format!(
        "        const _{n}DataBuf = polyplug.arenaAlloc(_{n}Bytes.length > 0 ? _{n}Bytes.length : 1);\n"
    ));
    out.push_str(&format!(
        "        const _{n}DataPtr = _{n}DataBuf[0] + _{n}DataBuf[1] * 4294967296;\n"
    ));
    out.push_str(&format!(
        "        for (let _i = 0; _i < _{n}Bytes.length; _i++) {{ polyplug.writeByte(_{n}DataPtr + _i, _{n}Bytes[_i]); }}\n"
    ));
    out.push_str(&format!(
        "        polyplug.writeU32({ap} + {offset}, _{n}DataBuf[0]);\n"
    ));
    out.push_str(&format!(
        "        polyplug.writeU32({ap} + {}, _{n}DataBuf[1]);\n",
        offset + 4
    ));
    out.push_str(&format!(
        "        polyplug.writeU32({ap} + {}, _{n}Bytes.length);\n",
        offset + 8
    ));
    out.push_str(&format!(
        "        polyplug.writeU32({ap} + {}, 0);\n",
        offset + 12
    ));
}

/// Emit a Buffer (Uint8Array) into `buf` at `offset` using arenaAlloc + writeU32/writeByte.
fn emit_ts_write_buffer(out: &mut String, param_name: &str, args_ptr: &str, offset: usize) {
    let n: &str = param_name;
    let ap: &str = args_ptr;
    out.push_str(&format!(
        "        const _{n}DataBuf = polyplug.arenaAlloc({n}.length > 0 ? {n}.length : 1);\n"
    ));
    out.push_str(&format!(
        "        const _{n}DataPtr = _{n}DataBuf[0] + _{n}DataBuf[1] * 4294967296;\n"
    ));
    out.push_str(&format!(
        "        for (let _i = 0; _i < {n}.length; _i++) {{ polyplug.writeByte(_{n}DataPtr + _i, {n}[_i]); }}\n"
    ));
    out.push_str(&format!(
        "        polyplug.writeU32({ap} + {offset}, _{n}DataBuf[0]);\n"
    ));
    out.push_str(&format!(
        "        polyplug.writeU32({ap} + {}, _{n}DataBuf[1]);\n",
        offset + 4
    ));
    out.push_str(&format!(
        "        polyplug.writeU32({ap} + {}, {n}.length);\n",
        offset + 8
    ));
    out.push_str(&format!(
        "        polyplug.writeU32({ap} + {}, 0);\n",
        offset + 12
    ));
    out.push_str(&format!(
        "        polyplug.writeU32({ap} + {}, {n}.length);\n",
        offset + 16
    ));
    out.push_str(&format!(
        "        polyplug.writeU32({ap} + {}, 0);\n",
        offset + 20
    ));
}

/// Emit the argsPtr setup for a TypeScript guest host contract / peer method.
///
/// Uses `arenaAlloc` exclusively — no manual free needed; the arena is
/// reclaimed automatically when the guest function returns.
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
                    "        const _{0}Bytes = new TextEncoder().encode({0});\n",
                    param.name
                ));
                out.push_str(&format!(
                    "        const _{0}DataBuf = polyplug.arenaAlloc(_{0}Bytes.length > 0 ? _{0}Bytes.length : 1);\n",
                    param.name
                ));
                out.push_str(&format!(
                    "        const _{0}DataPtr = _{0}DataBuf[0] + _{0}DataBuf[1] * 4294967296;\n",
                    param.name
                ));
                out.push_str(&format!(
                    "        for (let _i = 0; _i < _{0}Bytes.length; _i++) {{ polyplug.writeByte(_{0}DataPtr + _i, _{0}Bytes[_i]); }}\n",
                    param.name
                ));
                out.push_str("        const _argsBuf = polyplug.arenaAlloc(16);\n");
                out.push_str("        const argsPtr = _argsBuf[0] + _argsBuf[1] * 4294967296;\n");
                out.push_str(&format!(
                    "        polyplug.writeU32(argsPtr, _{0}DataBuf[0]);\n",
                    param.name
                ));
                out.push_str(&format!(
                    "        polyplug.writeU32(argsPtr + 4, _{0}DataBuf[1]);\n",
                    param.name
                ));
                out.push_str(&format!(
                    "        polyplug.writeU32(argsPtr + 8, _{0}Bytes.length);\n",
                    param.name
                ));
                out.push_str("        polyplug.writeU32(argsPtr + 12, 0);\n");
            }
            ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => {
                out.push_str(&format!(
                    "        const _{0}DataBuf = polyplug.arenaAlloc({0}.length > 0 ? {0}.length : 1);\n",
                    param.name
                ));
                out.push_str(&format!(
                    "        const _{0}DataPtr = _{0}DataBuf[0] + _{0}DataBuf[1] * 4294967296;\n",
                    param.name
                ));
                out.push_str(&format!(
                    "        for (let _i = 0; _i < {0}.length; _i++) {{ polyplug.writeByte(_{0}DataPtr + _i, {0}[_i]); }}\n",
                    param.name
                ));
                out.push_str("        const _argsBuf = polyplug.arenaAlloc(24);\n");
                out.push_str("        const argsPtr = _argsBuf[0] + _argsBuf[1] * 4294967296;\n");
                out.push_str(&format!(
                    "        polyplug.writeU32(argsPtr, _{0}DataBuf[0]);\n",
                    param.name
                ));
                out.push_str(&format!(
                    "        polyplug.writeU32(argsPtr + 4, _{0}DataBuf[1]);\n",
                    param.name
                ));
                out.push_str(&format!(
                    "        polyplug.writeU32(argsPtr + 8, {0}.length);\n",
                    param.name
                ));
                out.push_str("        polyplug.writeU32(argsPtr + 12, 0);\n");
                out.push_str(&format!(
                    "        polyplug.writeU32(argsPtr + 16, {0}.length);\n",
                    param.name
                ));
                out.push_str("        polyplug.writeU32(argsPtr + 20, 0);\n");
            }
            ResolvedTypeRef::Primitive(p) => {
                if matches!(p, PrimitiveType::U64 | PrimitiveType::I64) {
                    out.push_str("        const _argsBuf = polyplug.arenaAlloc(8);\n");
                    out.push_str(
                        "        const argsPtr = _argsBuf[0] + _argsBuf[1] * 4294967296;\n",
                    );
                    out.push_str(&format!(
                        "        polyplug.writeU32(argsPtr, {}.lo);\n",
                        param.name
                    ));
                    out.push_str(&format!(
                        "        polyplug.writeU32(argsPtr + 4, {}.hi);\n",
                        param.name
                    ));
                } else {
                    out.push_str("        const _argsBuf = polyplug.arenaAlloc(8);\n");
                    out.push_str(
                        "        const argsPtr = _argsBuf[0] + _argsBuf[1] * 4294967296;\n",
                    );
                    out.push_str(&format!(
                        "        polyplug.writeU32(argsPtr, {});\n",
                        param.name
                    ));
                }
            }
            ResolvedTypeRef::AbiType(AbiBuiltin::Ptr) => {
                out.push_str("        const _argsBuf = polyplug.arenaAlloc(8);\n");
                out.push_str("        const argsPtr = _argsBuf[0] + _argsBuf[1] * 4294967296;\n");
                out.push_str(&format!(
                    "        polyplug.writeU32(argsPtr, {}.lo);\n",
                    param.name
                ));
                out.push_str(&format!(
                    "        polyplug.writeU32(argsPtr + 4, {}.hi);\n",
                    param.name
                ));
            }
            ResolvedTypeRef::UserDefined(_) => {
                // UserDefined types in guest caller context are enum-backed scalars
                // (u32 in TypeScript). Pack as a 4-byte value in an 8-byte aligned slot.
                out.push_str("        const _argsBuf = polyplug.arenaAlloc(8);\n");
                out.push_str("        const argsPtr = _argsBuf[0] + _argsBuf[1] * 4294967296;\n");
                out.push_str(&format!(
                    "        polyplug.writeU32(argsPtr, Number({0}));\n",
                    param.name
                ));
                out.push_str("        polyplug.writeU32(argsPtr + 4, 0);\n");
            }
            ResolvedTypeRef::AbiType(AbiBuiltin::Void) => {
                out.push_str("        const argsPtr = 0;\n");
            }
        }
        return;
    }

    // Multiple params: compute total packed size then arenaAlloc once.
    let mut total_size: usize = 0;
    for param in &func.params {
        total_size += abi_sizeof(&param.ty);
    }

    out.push_str(&format!(
        "        const _argsBuf = polyplug.arenaAlloc({});\n",
        total_size
    ));
    out.push_str("        const argsPtr = _argsBuf[0] + _argsBuf[1] * 4294967296;\n");

    let mut offset: usize = 0;
    for param in &func.params {
        match &param.ty {
            ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => {
                emit_ts_write_string_view(out, &param.name, "argsPtr", offset);
                offset += 16;
            }
            ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => {
                emit_ts_write_buffer(out, &param.name, "argsPtr", offset);
                offset += 24;
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
                // UserDefined types in guest caller context are enum-backed scalars (u32).
                out.push_str(&format!(
                    "        polyplug.writeU32(argsPtr + {}, Number({}));\n",
                    offset, param.name
                ));
                offset += 4;
            }
            ResolvedTypeRef::AbiType(AbiBuiltin::Void) => {}
        }
    }
}

/// Emit the outPtr setup for a TypeScript guest host contract / peer method.
///
/// Allocates the correct size via `arenaAlloc` and defines `const outPtr`.
/// Does NOT pre-create `result` — that is done by `emit_ts_guest_host_contract_readback`
/// after the dispatch call succeeds.
fn emit_ts_guest_host_contract_out_setup(out: &mut String, returns: &Option<ResolvedTypeRef>) {
    if let Some(ret_ty) = returns {
        match ret_ty {
            ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => {
                out.push_str("        const _outBuf = polyplug.arenaAlloc(16);\n");
                out.push_str("        const outPtr = _outBuf[0] + _outBuf[1] * 4294967296;\n");
                out.push_str("        polyplug.writeU32(outPtr, 0);\n");
                out.push_str("        polyplug.writeU32(outPtr + 4, 0);\n");
                out.push_str("        polyplug.writeU32(outPtr + 8, 0);\n");
                out.push_str("        polyplug.writeU32(outPtr + 12, 0);\n");
            }
            ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => {
                out.push_str("        const _outBuf = polyplug.arenaAlloc(24);\n");
                out.push_str("        const outPtr = _outBuf[0] + _outBuf[1] * 4294967296;\n");
                out.push_str("        polyplug.writeU32(outPtr, 0);\n");
                out.push_str("        polyplug.writeU32(outPtr + 4, 0);\n");
                out.push_str("        polyplug.writeU32(outPtr + 8, 0);\n");
                out.push_str("        polyplug.writeU32(outPtr + 12, 0);\n");
                out.push_str("        polyplug.writeU32(outPtr + 16, 0);\n");
                out.push_str("        polyplug.writeU32(outPtr + 20, 0);\n");
            }
            ResolvedTypeRef::UserDefined(_) => {
                // User-defined structs: allocate 8 bytes (pointer-sized slot); the host
                // writes the return value here.  Without field-layout info at this call
                // site we cannot allocate the exact struct size, so we use a pointer
                // slot and note the limitation.
                out.push_str("        const _outBuf = polyplug.arenaAlloc(8);\n");
                out.push_str("        const outPtr = _outBuf[0] + _outBuf[1] * 4294967296;\n");
                out.push_str("        polyplug.writeU32(outPtr, 0);\n");
                out.push_str("        polyplug.writeU32(outPtr + 4, 0);\n");
            }
            ResolvedTypeRef::Primitive(p) => {
                if matches!(p, PrimitiveType::U64 | PrimitiveType::I64) {
                    out.push_str("        const _outBuf = polyplug.arenaAlloc(8);\n");
                    out.push_str("        const outPtr = _outBuf[0] + _outBuf[1] * 4294967296;\n");
                    out.push_str("        polyplug.writeU32(outPtr, 0);\n");
                    out.push_str("        polyplug.writeU32(outPtr + 4, 0);\n");
                } else {
                    // Allocate 8 bytes for safety even though the value is 4 bytes.
                    out.push_str("        const _outBuf = polyplug.arenaAlloc(8);\n");
                    out.push_str("        const outPtr = _outBuf[0] + _outBuf[1] * 4294967296;\n");
                    out.push_str("        polyplug.writeU32(outPtr, 0);\n");
                    out.push_str("        polyplug.writeU32(outPtr + 4, 0);\n");
                }
            }
            ResolvedTypeRef::AbiType(AbiBuiltin::Ptr) => {
                out.push_str("        const _outBuf = polyplug.arenaAlloc(8);\n");
                out.push_str("        const outPtr = _outBuf[0] + _outBuf[1] * 4294967296;\n");
                out.push_str("        polyplug.writeU32(outPtr, 0);\n");
                out.push_str("        polyplug.writeU32(outPtr + 4, 0);\n");
            }
            ResolvedTypeRef::AbiType(AbiBuiltin::Void) => {
                out.push_str("        const outPtr = 0;\n");
            }
        }
    } else {
        out.push_str("        const outPtr = 0;\n");
    }
}

/// Emit `const result = ...;` by reading the dispatch result back from `outPtr`.
///
/// Called after the dispatch call succeeds (errCode === 0).  The `returns` value
/// must NOT be `None` or `Void` — callers must guard against that.
fn emit_ts_guest_host_contract_readback(out: &mut String, returns: Option<&ResolvedTypeRef>) {
    let Some(ret_ty) = returns else {
        return;
    };
    match ret_ty {
        ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => {
            out.push_str("        const result = { ptr_lo: polyplug.readU32(outPtr), ptr_hi: polyplug.readU32(outPtr + 4), len: polyplug.readU32(outPtr + 8) };\n");
        }
        ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => {
            out.push_str("        const result = { ptr_lo: polyplug.readU32(outPtr), ptr_hi: polyplug.readU32(outPtr + 4), len: polyplug.readU32(outPtr + 8), cap: polyplug.readU32(outPtr + 16) };\n");
        }
        ResolvedTypeRef::AbiType(AbiBuiltin::Ptr) => {
            out.push_str(
                "        const result = { lo: polyplug.readU32(outPtr), hi: polyplug.readU32(outPtr + 4) };\n",
            );
        }
        ResolvedTypeRef::Primitive(p) => match p {
            PrimitiveType::U64 | PrimitiveType::I64 => {
                out.push_str(
                    "        const result = { lo: polyplug.readU32(outPtr), hi: polyplug.readU32(outPtr + 4) };\n",
                );
            }
            PrimitiveType::I8 | PrimitiveType::I16 | PrimitiveType::I32 => {
                out.push_str("        const result: number = polyplug.readI32(outPtr);\n");
            }
            PrimitiveType::F32 => {
                out.push_str("        const result: number = polyplug.readF32(outPtr);\n");
            }
            PrimitiveType::F64 => {
                out.push_str("        const result: number = polyplug.readF64(outPtr);\n");
            }
            PrimitiveType::Bool => {
                out.push_str("        const result: number = polyplug.readU32(outPtr);\n");
            }
            _ => {
                out.push_str("        const result: number = polyplug.readU32(outPtr);\n");
            }
        },
        ResolvedTypeRef::UserDefined(_) => {
            out.push_str(
                "        const result = { lo: polyplug.readU32(outPtr), hi: polyplug.readU32(outPtr + 4) } as any;\n",
            );
        }
        ResolvedTypeRef::AbiType(AbiBuiltin::Void) => {}
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

    let type_imports: BTreeSet<String> = collect_ts_guest_host_contract_type_imports(ir);
    if !type_imports.is_empty() {
        let import_list: String = type_imports.into_iter().collect::<Vec<String>>().join(", ");
        out.push_str(&format!("import {{ {} }} from './types';\n\n", import_list));
    }

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

/// Collect user-defined type names (enums, structs) referenced in guest-side
/// host-contract caller signatures, so `host_contracts.ts` can import them from
/// `./types`. Without this, a host-contract method that takes an enum parameter
/// (e.g. `log_with_level(level: LogLevel, ...)`) references an undeclared name
/// and fails `deno check`.
fn collect_ts_guest_host_contract_type_imports(ir: &ValidatedIr) -> BTreeSet<String> {
    let mut imports: BTreeSet<String> = BTreeSet::new();
    for contract in &ir.host_contracts {
        for func in &contract.functions {
            for param in &func.params {
                if let ResolvedTypeRef::UserDefined(name) = &param.ty {
                    imports.insert(name.clone());
                }
            }
            if let Some(ResolvedTypeRef::UserDefined(name)) = &func.returns {
                imports.insert(name.clone());
            }
        }
    }
    imports
}

// ─── Host Interface Factories Generation ────────────────────────────────────────

/// Generate all host-side interface factories into a single file.
fn generate_js_host_interface_factories_ts(ir: &ValidatedIr) -> String {
    let mut out: String = String::new();
    out.push_str(
        "// THIS FILE IS AUTO-GENERATED BY polyplugc\n\
         // DO NOT EDIT BY HAND\n\
         // Runtime: js-quickjs (host-side interface factories)\n\n",
    );

    out.push_str("import type { HostContractVTable } from 'polyplug';\n");
    out.push_str("import { DispatchType } from 'polyplug';\n");
    out.push_str("import type * as contracts from './contracts';\n\n");

    out.push_str("// ABI error codes (match polyplug_abi.AbiErrorCode)\n");
    out.push_str("const AbiErrorCode = {\n");
    out.push_str("    Ok: 0,\n");
    out.push_str("    Panic: 5,\n");
    out.push_str("};\n\n");

    for contract in &ir.host_contracts {
        generate_js_host_interface_factory(&mut out, contract);
    }

    out
}

/// Generate interface factories for one host contract.
fn generate_js_host_interface_factory(out: &mut String, contract: &ResolvedHostContract) {
    let iface_name: String = host_contract_name_to_ts_interface(&contract.name);
    let factory_name: String = format!("create{}Vtable", iface_name);
    let factory_vm_name: String = format!("create{}VtableVm", iface_name);
    let fn_count: usize = contract.functions.len();
    let contract_id: u64 = contract.contract_id;
    let contract_id_lo: u32 = (contract_id & 0xFFFFFFFF) as u32;
    let contract_id_hi: u32 = (contract_id >> 32) as u32;
    let major: u32 = contract.version.major;
    let minor: u32 = contract.version.minor;
    let singleton: bool = contract.singleton;

    // NATIVE dispatch factory
    out.push_str(&format!(
        "/** Create a host contract interface for `{}` with NATIVE dispatch. */\n",
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
    out.push_str("    const functions: (() => number)[] = [\n");
    for func in &contract.functions {
        let thunk_name: String = format!(
            "_{}_{}_thunk",
            contract.name.replace('.', "_").to_lowercase(),
            func.name
        );
        out.push_str(&format!("        {thunk_name},\n"));
    }
    out.push_str("    ];\n\n");

    // Create the interface
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
    out.push_str(&format!("            singleton: {singleton},\n"));
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
        "/** Create a host contract interface for `{}` with VM dispatch. */\n",
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
    out.push_str(&format!("            singleton: {singleton},\n"));
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
    out.push_str("                return AbiErrorCode.Panic;\n");
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

    out.push_str("            return AbiErrorCode.Ok;\n");
    out.push_str("        } catch (e) {\n");
    out.push_str("            return AbiErrorCode.Panic;\n");
    out.push_str("        }\n");
    out.push_str("    }\n\n");
}

/// Generate argument extraction for a host thunk.
fn generate_js_host_thunk_args(out: &mut String, func: &ResolvedFunction) {
    if func.params.len() == 1 {
        let param: &ResolvedParam = &func.params[0];
        match &param.ty {
            ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => {
                out.push_str("            // Extract StringView from args pointer\n");
                out.push_str(&format!(
                    "            const {name} = '';\n",
                    name = param.name
                ));
            }
            ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => {
                out.push_str("            // Extract Buffer from args pointer\n");
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
        // Multiple params - placeholder defaults. Each must use a value assignable
        // to its TS type (a bare `0` is not assignable to `string`/`Uint8Array`).
        for param in &func.params {
            match &param.ty {
                ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => {
                    out.push_str(&format!(
                        "            const {name} = '';\n",
                        name = param.name
                    ));
                }
                ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => {
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

// ─── Peer Caller Generation ───────────────────────────────────────────────────

/// Collect every contract in `ir.contracts` whose contract_id appears in the
/// bundle's declared dependencies.  Returns an empty vec when there is no bundle
/// or when no dependency matches any known contract.
fn collect_peer_contracts(ir: &ValidatedIr) -> Vec<&ResolvedContract> {
    let deps: &[ResolvedDependency] = match ir.bundle.as_ref() {
        Some(b) => &b.dependencies,
        None => return Vec::new(),
    };

    ir.contracts
        .iter()
        .filter(|c: &&ResolvedContract| {
            deps.iter().any(|d: &ResolvedDependency| {
                let dep_contract_id: u64 = match d {
                    ResolvedDependency::ByContract { contract_id, .. } => *contract_id,
                    ResolvedDependency::ByBundle { contract_id, .. } => *contract_id,
                };
                dep_contract_id == c.contract_id
            })
        })
        .collect()
}

/// Look up the `min_version` (packed u32 — major in high 16 bits) declared for
/// `target_contract_id` in the bundle dependencies.  Returns 0 when not found.
fn peer_min_version(ir: &ValidatedIr, target_contract_id: u64) -> u32 {
    let deps: &[ResolvedDependency] = match ir.bundle.as_ref() {
        Some(b) => &b.dependencies,
        None => return 0,
    };
    for d in deps {
        match d {
            ResolvedDependency::ByContract {
                contract_id,
                min_version,
                ..
            } if *contract_id == target_contract_id => return *min_version,
            ResolvedDependency::ByBundle {
                contract_id,
                min_version,
                ..
            } if *contract_id == target_contract_id => return *min_version,
            _ => {}
        }
    }
    0
}

/// Convert a guest contract name (e.g. `pipeline.Validator`) to a TypeScript
/// peer-caller class name (e.g. `ValidatorPeer`).
fn guest_contract_name_to_ts_peer(name: &str) -> String {
    let last: &str = name.split('.').next_back().unwrap_or(name);
    let mut chars: core::str::Chars<'_> = last.chars();
    let pascal: String = match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    };
    pascal + "Peer"
}

/// Generate `guest/peer_callers.ts` — one typed peer class per matched dependency.
fn generate_guest_peer_callers_ts(ir: &ValidatedIr, peers: &[&ResolvedContract]) -> String {
    let mut out: String = String::new();
    out.push_str(
        "// THIS FILE IS AUTO-GENERATED BY polyplugc\n\
         // DO NOT EDIT BY HAND\n\
         // Runtime: js-quickjs (guest-side peer callers)\n\n",
    );

    // Collect user-defined type imports needed by peer method signatures.
    let mut type_imports: BTreeSet<String> = BTreeSet::new();
    for contract in peers {
        for func in &contract.functions {
            for param in &func.params {
                if let ResolvedTypeRef::UserDefined(name) = &param.ty {
                    type_imports.insert(name.clone());
                }
            }
            if let Some(ResolvedTypeRef::UserDefined(name)) = &func.returns {
                type_imports.insert(name.clone());
            }
        }
    }
    if !type_imports.is_empty() {
        let import_list: String = type_imports.into_iter().collect::<Vec<String>>().join(", ");
        out.push_str(&format!("import {{ {} }} from './types';\n\n", import_list));
    }

    for contract in peers {
        let min_ver: u32 = peer_min_version(ir, contract.contract_id);
        generate_ts_peer_caller_class(&mut out, contract, min_ver);
    }

    out
}

/// Generate one `<Name>Peer` class for a single peer contract.
fn generate_ts_peer_caller_class(out: &mut String, contract: &ResolvedContract, min_version: u32) {
    let class_name: String = guest_contract_name_to_ts_peer(&contract.name);
    let contract_id_lo: u32 = (contract.contract_id & 0xFFFF_FFFF) as u32;
    let contract_id_hi: u32 = (contract.contract_id >> 32) as u32;

    out.push_str(&format!(
        "/**\n * Peer caller for guest contract `{}` (id=0x{:016X})\n *\n\
         * Dispatches through the host-mediated `callGuestMethod` bridge.\n\
         * Uses per-call create+destroy (stateless contract model).\n\
         * Stateful peers would require a retained instance-handle API.\n */\n",
        contract.name, contract.contract_id
    ));
    out.push_str(&format!("export class {} {{\n", class_name));
    out.push_str(&format!(
        "    /** Contract ID for `{}@{}` (FNV-1a). */\n",
        contract.name, contract.version.major
    ));
    out.push_str(&format!(
        "    static readonly CONTRACT_ID_LO = 0x{:08X};\n",
        contract_id_lo
    ));
    out.push_str(&format!(
        "    static readonly CONTRACT_ID_HI = 0x{:08X};\n",
        contract_id_hi
    ));
    out.push_str(&format!(
        "    static readonly MIN_VERSION = {};\n\n",
        min_version
    ));

    out.push_str("    private constructor() {}\n\n");

    out.push_str("    /**\n");
    out.push_str("     * Verify the peer contract is reachable via the host.\n");
    out.push_str("     * Returns a `");
    out.push_str(&class_name);
    out.push_str("` instance or `null` if not found.\n");
    out.push_str("     */\n");
    out.push_str(&format!("    static resolve(): {} | null {{\n", class_name));
    out.push_str("        const polyplug = (globalThis as any).polyplug;\n");
    out.push_str("        if (!polyplug || !polyplug.findByContract) {\n");
    out.push_str("            return null;\n");
    out.push_str("        }\n");
    out.push_str(&format!(
        "        const handle = polyplug.findByContract(0x{:08X}, 0x{:08X}, {});\n",
        contract_id_lo, contract_id_hi, min_version
    ));
    out.push_str("        if (handle === null || handle === undefined || handle === 0) {\n");
    out.push_str("            return null;\n");
    out.push_str("        }\n");
    out.push_str(&format!("        return new {}();\n", class_name));
    out.push_str("    }\n\n");

    for func in &contract.functions {
        generate_ts_peer_caller_method(out, func, contract_id_lo, contract_id_hi, min_version);
    }

    out.push_str("}\n\n");
}

/// Generate one method for a peer caller class.
///
/// Reuses `emit_ts_guest_host_contract_args_setup` and
/// `emit_ts_guest_host_contract_out_setup` for identical StringView/Buffer/u64
/// marshalling.  The call itself routes through `polyplug.callGuestMethod`
/// instead of reading a vtable header — that is the key simplification vs the
/// host-contract method.
fn generate_ts_peer_caller_method(
    out: &mut String,
    func: &ResolvedFunction,
    contract_id_lo: u32,
    contract_id_hi: u32,
    min_version: u32,
) {
    let fn_id: u32 = func.function_id;
    let has_return: bool = func.returns.is_some();
    // Use the raw ABI return type — it must match the `result` shape that
    // emit_ts_guest_host_contract_out_setup initialises (StringView → {ptr_lo,ptr_hi,len}, etc.).
    let return_type: String = match &func.returns {
        Some(ty) => ts_peer_raw_return_type(ty),
        None => "void".to_owned(),
    };

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

    out.push_str(&format!("    /** Call peer `{}` */\n", func.name));
    out.push_str(&format!(
        "    {}({}): {} {{\n",
        func.name, params_str, return_type
    ));

    out.push_str("        const polyplug = (globalThis as any).polyplug;\n");
    out.push_str("        if (!polyplug || !polyplug.callGuestMethod) {\n");
    if has_return {
        out.push_str("            return null as any;\n");
    } else {
        out.push_str("            return;\n");
    }
    out.push_str("        }\n");

    // Marshal args and out buffer using the same helpers as host-contract methods.
    emit_ts_guest_host_contract_args_setup(out, func);
    emit_ts_guest_host_contract_out_setup(out, &func.returns);

    // Call the bridge primitive.  It returns the u32 error code directly.
    out.push_str(&format!(
        "        const errCode: number = polyplug.callGuestMethod(0x{:08X}, 0x{:08X}, {}, {}, argsPtr, outPtr);\n",
        contract_id_lo, contract_id_hi, min_version, fn_id
    ));
    out.push_str("        if (errCode !== 0) {\n");
    if has_return {
        out.push_str("            return null as any;\n");
    } else {
        out.push_str("            return;\n");
    }
    out.push_str("        }\n");

    if has_return {
        emit_ts_guest_host_contract_readback(out, func.returns.as_ref());
        out.push_str("        return result;\n");
    }

    out.push_str("    }\n\n");
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
            singleton: false,
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
                singleton: false,
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
                singleton: false,
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
    fn generate_ts_guest_host_contract_caller_produces_class() {
        let contract: ResolvedHostContract = ResolvedHostContract {
            name: "host.logger".to_owned(),
            contract_id: 0x1234_5678_9ABC_DEF0_u64,
            version: Version {
                major: 1,
                minor: 0,
                patch: 0,
            },
            singleton: false,
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
            out.contains("private constructor(minVersion: number)"),
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
        // New shape: uses callHostContract, arenaAlloc, not the old dispatch machinery.
        assert!(
            out.contains("polyplug.callHostContract("),
            "must use callHostContract: {out}"
        );
        assert!(
            !out.contains("readHostContractHeader"),
            "must not use readHostContractHeader: {out}"
        );
        assert!(
            !out.contains("callDispatchFn"),
            "must not use callDispatchFn: {out}"
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
            singleton: false,
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
            out.contains(
                "read(path: string): { ptr_lo: number; ptr_hi: number; len: number; cap: number }"
            ),
            "missing read method with raw Buffer return: {out}"
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
                singleton: false,
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
                singleton: false,
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
    fn enum_is_exported_for_cross_module_use() {
        // Regression: `host_contracts.ts` references enum types (e.g. `LogLevel`)
        // declared in `types.ts`. The enum const and its type must be `export`ed
        // or the cross-module reference fails `deno check` with TS2304.
        let e: EnumDef = EnumDef {
            name: "LogLevel".to_owned(),
            repr: ReprType::U32,
            bitflag: false,
            variants: vec![EnumVariant {
                name: "Debug".to_owned(),
                value: "0".to_owned(),
            }],
        };
        let mut out: String = String::new();
        generate_js_quickjs_enum(&mut out, &e);
        assert!(
            out.contains("export const LogLevel = Object.freeze({"),
            "enum const must be exported: {out}"
        );
        assert!(
            out.contains("export type LogLevel = typeof LogLevel"),
            "enum type must be exported: {out}"
        );
    }

    #[test]
    fn guest_host_contracts_imports_enum_param_types() {
        // Regression: a host-contract method taking an enum parameter must import
        // that enum from `./types`, otherwise the name is undeclared (TS2304).
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
                singleton: false,
                functions: vec![ResolvedFunction {
                    name: "log_with_level".to_owned(),
                    function_id: 0,
                    params: vec![ResolvedParam {
                        name: "level".to_owned(),
                        ty: ResolvedTypeRef::UserDefined("LogLevel".to_owned()),
                    }],
                    returns: None,
                }],
            }],
            bundle: None,
        };
        let out: String = generate_guest_host_contracts_ts(&ir);
        assert!(
            out.contains("import { LogLevel } from './types';"),
            "host_contracts.ts must import enum param type from ./types: {out}"
        );
    }

    #[test]
    fn guest_host_contracts_no_type_import_when_no_user_types() {
        // A host contract using only ABI builtins must NOT emit a spurious import.
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
                singleton: false,
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
        assert!(
            !out.contains("from './types'"),
            "no type import expected for ABI-only host contract: {out}"
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

    #[test]
    fn peer_caller_emitted_for_declared_dependency() {
        use crate::ir::ResolvedDependency;
        use crate::ir::Version;
        // Build an IR where the bundle declares a dependency on a contract that
        // IS present in ir.contracts — the generator must emit peer_callers.ts.
        let validator_id: u64 = polyplug_utils::guest_contract_id("pipeline.Validator", 1);
        let ir: ValidatedIr = ValidatedIr {
            types: vec![],
            enums: vec![],
            contracts: vec![crate::ir::ResolvedContract {
                name: "pipeline.Validator".to_owned(),
                contract_id: validator_id,
                version: Version {
                    major: 1,
                    minor: 0,
                    patch: 0,
                },
                functions: vec![],
            }],
            host_contracts: vec![],
            bundle: Some(crate::ir::ResolvedBundle {
                name: "transformer".to_owned(),
                version: Version {
                    major: 1,
                    minor: 0,
                    patch: 0,
                },
                runtime: "js-quickjs".to_owned(),
                file: polyplug_codegen::ResolvedBundleFile::Single("libtransformer.so".to_owned()),
                plugins: vec![],
                bundle_id: 0,
                dependencies: vec![ResolvedDependency::ByContract {
                    contract: "pipeline.Validator".to_owned(),
                    contract_id: validator_id,
                    min_version: 1 << 16,
                }],
                needs_reinit_on_dep_reload: false,
            }),
        };
        let generator: JsQuickjsGenerator = JsQuickjsGenerator;
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
            names.contains(&"guest/peer_callers.ts".to_owned()),
            "expected guest/peer_callers.ts in {names:?}"
        );
        // The generated file must mention callGuestMethod.
        let peer_file: &GeneratedFile = files
            .files
            .iter()
            .find(|f: &&GeneratedFile| f.path.to_string_lossy() == "guest/peer_callers.ts")
            .expect("peer_callers.ts");
        assert!(
            peer_file.content.contains("callGuestMethod"),
            "peer_callers.ts must call callGuestMethod"
        );
        assert!(
            peer_file.content.contains("ValidatorPeer"),
            "peer_callers.ts must contain ValidatorPeer class"
        );
        // guest/index.ts must re-export ValidatorPeer.
        let index_file: &GeneratedFile = files
            .files
            .iter()
            .find(|f: &&GeneratedFile| f.path.to_string_lossy() == "guest/index.ts")
            .expect("guest/index.ts");
        assert!(
            index_file.content.contains("ValidatorPeer"),
            "guest/index.ts must re-export ValidatorPeer"
        );
    }

    #[test]
    fn guest_host_contract_stringview_return_uses_new_dispatch() {
        // A StringView-returning host-contract method must use arenaAlloc(16) for the
        // out buffer, emit polyplug.callHostContract, and read back via readU32(outPtr+8).
        let contract: ResolvedHostContract = ResolvedHostContract {
            name: "host.svc".to_owned(),
            contract_id: 0xABCD_1234_u64,
            version: Version {
                major: 1,
                minor: 0,
                patch: 0,
            },
            singleton: false,
            functions: vec![ResolvedFunction {
                name: "describe".to_owned(),
                function_id: 1,
                params: vec![ResolvedParam {
                    name: "key".to_owned(),
                    ty: ResolvedTypeRef::AbiType(AbiBuiltin::StringView),
                }],
                returns: Some(ResolvedTypeRef::AbiType(AbiBuiltin::StringView)),
            }],
        };
        let mut out: String = String::new();
        generate_ts_guest_host_contract_caller(&mut out, &contract);
        assert!(
            out.contains("polyplug.arenaAlloc(16)"),
            "out buffer must be arenaAlloc(16) for StringView: {out}"
        );
        assert!(
            out.contains("polyplug.callHostContract("),
            "must use callHostContract: {out}"
        );
        assert!(
            out.contains("polyplug.readU32(outPtr + 8)"),
            "must read len from outPtr+8: {out}"
        );
    }

    #[test]
    fn guest_host_contract_buffer_out_uses_arena_alloc_24() {
        // A Buffer-returning host-contract method must allocate 24 bytes for the out slot.
        let contract: ResolvedHostContract = ResolvedHostContract {
            name: "host.store".to_owned(),
            contract_id: 0x1111_2222_u64,
            version: Version {
                major: 1,
                minor: 0,
                patch: 0,
            },
            singleton: false,
            functions: vec![ResolvedFunction {
                name: "snapshot".to_owned(),
                function_id: 2,
                params: vec![],
                returns: Some(ResolvedTypeRef::AbiType(AbiBuiltin::Buffer)),
            }],
        };
        let mut out: String = String::new();
        generate_ts_guest_host_contract_caller(&mut out, &contract);
        assert!(
            out.contains("polyplug.arenaAlloc(24)"),
            "out buffer must be arenaAlloc(24) for Buffer: {out}"
        );
        assert!(
            out.contains("polyplug.callHostContract("),
            "must use callHostContract: {out}"
        );
    }

    #[test]
    fn no_peer_callers_without_dependencies() {
        // A bundle with no [[dependency]] entries must NOT emit peer_callers.ts.
        let ir: ValidatedIr = ValidatedIr {
            types: vec![],
            enums: vec![],
            contracts: vec![crate::ir::ResolvedContract {
                name: "pipeline.Validator".to_owned(),
                contract_id: polyplug_utils::guest_contract_id("pipeline.Validator", 1),
                version: crate::ir::Version {
                    major: 1,
                    minor: 0,
                    patch: 0,
                },
                functions: vec![],
            }],
            host_contracts: vec![],
            bundle: None,
        };
        let generator: JsQuickjsGenerator = JsQuickjsGenerator;
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
            !names.contains(&"guest/peer_callers.ts".to_owned()),
            "unexpected guest/peer_callers.ts: {names:?}"
        );
    }
}
