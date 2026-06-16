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
use crate::ir::ReprType;
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
            content: generate_callers_ts(ir)?,
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
                content: generate_js_host_interface_factories_ts(ir)?,
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
                content: generate_guest_host_contracts_ts(ir)?,
                force_regenerate: false,
            });
        }
        // ── peer_callers.ts ────────────────────────────────────────────────────
        let peer_contracts: Vec<&ResolvedContract> = collect_peer_contracts(ir);
        if !peer_contracts.is_empty() {
            files.files.push(GeneratedFile {
                path: PathBuf::from("guest/peer_callers.ts"),
                content: generate_guest_peer_callers_ts(ir, &peer_contracts)?,
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
    // Collect the user-defined type names referenced by the rendered plugin
    // interfaces (setXFactory signatures use them) — an empty type import leaves
    // `AddArgs`-style references dangling under `deno check`.
    let mut type_imports: BTreeSet<String> = BTreeSet::new();
    if let Some(bundle) = &ir.bundle {
        for plugin in &bundle.plugins {
            for contract_impl in &plugin.implements {
                if let Some(contract) = ir.contracts.iter().find(|c| {
                    let contract_full =
                        format!("{}@{}.{}", c.name, c.version.major, c.version.minor);
                    &contract_full == contract_impl
                }) {
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
            }
        }
    }
    if type_imports.is_empty() {
        out.push_str("import type { } from './types';\n\n");
    } else {
        out.push_str(&format!(
            "import type {{ {} }} from './types';\n\n",
            type_imports.into_iter().collect::<Vec<String>>().join(", ")
        ));
    }
    out.push_str("/** Dispatch mechanism type — determines how function calls are routed. */\n");
    out.push_str("const DispatchType = Object.freeze({\n");
    out.push_str("    Native: 0,\n");
    out.push_str("    VirtualMachine: 1,\n");
    out.push_str("} as const);\n\n");
    emit_ts_utf8_encoder_helper(&mut out);

    if let Some(bundle) = &ir.bundle {
        for plugin in &bundle.plugins {
            for contract_impl in &plugin.implements {
                if let Some(contract) = ir.contracts.iter().find(|c| {
                    let contract_full =
                        format!("{}@{}.{}", c.name, c.version.major, c.version.minor);
                    &contract_full == contract_impl
                }) {
                    render_plugin_interface_quickjs(&mut out, &plugin.name, contract, ir)?;
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
    ir: &ValidatedIr,
) -> Result<(), PolyplugcError> {
    let plugin_var: String = plugin_name.to_uppercase().replace(['.', '-'], "_");
    let contract_name_full: String = format!("{}@{}", contract.name, contract.version.major);
    let contract_id: u64 =
        polyplug_utils::guest_contract_id(&contract.name, contract.version.major);
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
    // No createInstance/destroyInstance stubs: the loader reads this interface from
    // polyplug_init's returned registrations, extracts only `functions`, and
    // substitutes its own instance lifecycle — stubs here would be dead code with a
    // misleading "override" promise.
    out.push_str(&format!("    fnCount: {function_count},\n"));
    out.push_str(
        "    functions: [] as ((impl: any, args_ptr: number, out_ptr: number, arena_ptr: number, bridge: any) => number)[],\n",
    );
    // The factory builds a fresh impl object per instance. setXFactory installs it
    // before registration; the loader reads `vtable.factory` and calls it with the
    // bridge + host vtable lo/hi for the load-time default impl and once per
    // create_instance.
    out.push_str(
        "    factory: null as null | ((bridge: any, hostLo: number, hostHi: number) => any),\n",
    );
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

    let set_factory_name: String = format!(
        "set{}Factory",
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
        // The loader dispatches via js_dispatch, which passes:
        //   impl      — the per-instance impl object the loader resolved
        //   args_ptr  — full address of the packed args buffer (as Number/f64)
        //   out_ptr   — full address of the output buffer (as Number/f64)
        //   arena_ptr — this call's CallArena pointer (Number); threaded to
        //               `bridge.arenaAlloc(size, arena_ptr)` for StringView returns
        //   bridge    — the host-capability bridge (NOT read from any global — Rule 12)
        // User-space addresses are < 2^48 < 2^53 (float64 mantissa), so the
        // usize→f64→usize round-trip is exact. readU32/writeU32 also accept f64.
        out.push_str("\nfunction ");
        out.push_str(&wrapper_name);
        out.push_str(
            "(impl: any, args_ptr: number, out_ptr: number, arena_ptr: number, bridge: any): number {\n",
        );
        out.push_str("    // SAFETY: args_ptr and out_ptr are valid addresses passed as f64\n");
        out.push_str("    // by the loader. readU32/writeU32 accept f64 and convert to usize.\n");
        out.push_str(
            "    // `impl` is the per-instance impl object the loader resolved for this\n",
        );
        out.push_str(
            "    // call (built by the factory); the loader passes it as the first argument.\n",
        );
        out.push_str("    const polyplug = bridge;\n");
        out.push_str("    if (!polyplug) return 1;\n");
        out.push_str("    if (!impl) return 1;\n");

        if has_params {
            out.push_str("    if (!args_ptr) return 8;\n");
        }
        if has_return {
            out.push_str("    if (!out_ptr) return 8;\n");
        }

        // Per-signature argument unpacking. The host-side caller passes a
        // pointer to the bare value for a single parameter and a
        // #[repr(C)]-equivalent arg-pack struct for 2+ parameters (mirrors the
        // rust generator's emit_args_setup), so reads use C-layout offsets
        // (natural alignment), NOT a fixed StringView shape.
        let mut arg_names: Vec<String> = Vec::new();
        if func.params.len() == 1 {
            let param: &ResolvedParam = &func.params[0];
            // SAFETY (emitted reads): args_ptr is a valid host-allocated
            // buffer holding exactly one ABI value of the parameter type.
            let expr: String = js_read_expr(&param.ty, "args_ptr", 0, ir)?;
            out.push_str(&format!("    var arg_{} = {};\n", param.name, expr));
            arg_names.push(format!("arg_{}", param.name));
        } else if func.params.len() >= 2 {
            let mut offset: usize = 0;
            for param in &func.params {
                let align: usize = js_c_align(&param.ty, ir)?;
                offset = align_up(offset, align);
                // SAFETY (emitted reads): args_ptr points at the C-layout
                // arg-pack struct written by the host caller; each field is
                // read at its natural-alignment offset.
                let expr: String = js_read_expr(&param.ty, "args_ptr", offset, ir)?;
                out.push_str(&format!("    var arg_{} = {};\n", param.name, expr));
                offset += js_c_size(&param.ty, ir)?;
                arg_names.push(format!("arg_{}", param.name));
            }
        }

        // Call the implementation
        if has_return {
            out.push_str(&format!(
                "    var result = impl.fn{idx}({});\n",
                arg_names.join(", ")
            ));
        } else {
            out.push_str(&format!("    impl.fn{idx}({});\n", arg_names.join(", ")));
        }

        // Write the return value back through out_ptr per the contract's
        // declared return type (the host pre-allocates an out slot of exactly
        // that C-layout size and reads it back after dispatch).
        if let Some(ret_ty) = &func.returns {
            // SAFETY (emitted writes): out_ptr is a valid host-allocated out
            // slot sized for the declared return type.
            emit_js_guest_return_write(out, ret_ty, ir)?;
        }
        out.push_str("    return 0;\n");
        out.push_str("}\n");

        abi_wrappers.push(wrapper_name);
    }

    // Install the per-instance factory + ABI wrappers. The factory receives the
    // bridge and the host vtable lo/hi (threaded explicitly — no global, Rule 12)
    // and returns a fresh impl object ({ fn0, fn1, ... }) per instance; the loader
    // calls it once at load (default impl) and once per create_instance, then
    // passes the resolved impl as each wrapper's first argument. No module-global
    // impl or host pointer: state lives on the per-instance object the factory
    // builds, and the bridge/host travel as explicit arguments.
    //
    // A StringView return is typed `string`: the author returns a plain string and
    // the generated wrapper arena-allocates it (mirrors lua/python).
    let impl_members: Vec<String> = contract
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
                Some(ResolvedTypeRef::AbiType(AbiBuiltin::StringView)) => "string".to_owned(),
                Some(ty) => ts_type_ref(ty),
            };
            format!("fn{idx}: ({}) => {}", params, ret)
        })
        .collect();
    let impl_shape: String = format!("{{ {} }}", impl_members.join("; "));

    out.push_str(&format!(
        "\nexport function {set_factory_name}(factory: (bridge: any, hostLo: number, hostHi: number) => {impl_shape}): void {{\n"
    ));

    out.push_str("    ");
    out.push_str(&plugin_var);
    out.push_str("_INTERFACE.factory = factory;\n");

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
            let set_factory_name: String = format!(
                "set{}Factory",
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
                "export {{ {set_factory_name} }} from './contracts';\n"
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
    out.push_str("// ABI error codes (match polyplug_abi.AbiErrorCode)\n");
    out.push_str("const AbiErrorCode = {\n");
    out.push_str("    Ok: 0,\n");
    out.push_str("    Generic: 1,\n");
    out.push_str("    InvalidPointer: 8,\n");
    out.push_str("};\n\n");

    out.push_str("interface AbiError {\n");
    out.push_str("    code: number;\n");
    out.push_str("    message: string;\n");
    out.push_str("}\n\n");

    out.push_str("// One registration entry per implemented contract. The loader reads this\n");
    out.push_str("// array from polyplug_init's return value (nothing is deposited into any\n");
    out.push_str("// global — Rule 12) and registers one GuestContractInterface per entry.\n");
    out.push_str("interface Registration {\n");
    out.push_str("    contractLo: number;\n");
    out.push_str("    contractHi: number;\n");
    out.push_str("    interface: any;\n");
    out.push_str("    fnCount: number;\n");
    out.push_str("    contractName: string;\n");
    out.push_str("    version: number;\n");
    out.push_str("}\n\n");

    out.push_str("/**\n");
    out.push_str(" * Initialize plugin with host runtime.\n");
    out.push_str(" *\n");
    out.push_str(" * Returns `[registrations, abiError]`: the per-contract registration array\n");
    out.push_str(" * the loader consumes, plus the canonical AbiError ({ code, message }).\n");
    out.push_str(
        " * Nothing is deposited into any global namespace (Rule 12) — the loader reads\n",
    );
    out.push_str(
        " * BOTH return values. The host vtable and the `bridge` are threaded explicitly\n",
    );
    out.push_str(" * to each author factory; no host pointer or bridge is stored in any module.\n");
    out.push_str(" *\n");
    out.push_str(" * @param host_lo - HostApi pointer (low 32 bits)\n");
    out.push_str(" * @param host_hi - HostApi pointer (high 32 bits)\n");
    out.push_str(" * @param ctx_lo - BundleInitContext pointer (low 32 bits)\n");
    out.push_str(" * @param ctx_hi - BundleInitContext pointer (high 32 bits)\n");
    out.push_str(" * @param bridge - Host-capability bridge passed in by the loader\n");
    out.push_str(" */\n");
    out.push_str("export function polyplug_init(\n");
    out.push_str("    host_lo: number, host_hi: number,\n");
    out.push_str("    ctx_lo: number, ctx_hi: number,\n");
    out.push_str("    bridge: any\n");
    out.push_str("): [Registration[], AbiError] {\n");
    out.push_str("    // Validate parameters\n");
    out.push_str("    if (host_lo === 0 && host_hi === 0) {\n");
    out.push_str(
        "        return [[], { code: AbiErrorCode.Generic, message: \"null host pointer in polyplug_init\" }];\n",
    );
    out.push_str("    }\n");
    out.push_str("    if (ctx_lo === 0 && ctx_hi === 0) {\n");
    out.push_str(
        "        return [[], { code: AbiErrorCode.Generic, message: \"null ctx pointer in polyplug_init\" }];\n",
    );
    out.push_str("    }\n");
    out.push_str("    if (!bridge || !bridge.alloc) {\n");
    out.push_str(
        "        return [[], { code: AbiErrorCode.Generic, message: \"missing bridge in polyplug_init\" }];\n",
    );
    out.push_str("    }\n\n");

    out.push_str("    const registrations: Registration[] = [];\n");
    for plugin in &bundle.plugins {
        let plugin_var: String = plugin.name.to_uppercase().replace(['.', '-'], "_");
        out.push_str(&format!(
            "    // Register plugin: {plugin_name}\n",
            plugin_name = plugin.name
        ));
        out.push_str("    registrations.push({\n");
        out.push_str(&format!(
            "        contractLo: {plugin_var}_INTERFACE.contractLo,\n"
        ));
        out.push_str(&format!(
            "        contractHi: {plugin_var}_INTERFACE.contractHi,\n"
        ));
        out.push_str(&format!("        interface: {plugin_var}_INTERFACE,\n"));
        out.push_str(&format!(
            "        fnCount: {plugin_var}_INTERFACE.fnCount,\n"
        ));
        out.push_str(&format!(
            "        contractName: {plugin_var}_INTERFACE.contractName,\n"
        ));
        out.push_str(&format!(
            "        version: {plugin_var}_INTERFACE.version,\n"
        ));
        out.push_str("    });\n\n");
    }

    out.push_str("    return [registrations, { code: AbiErrorCode.Ok, message: \"\" }];\n");
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
    let loader: &str = &bundle.loader;

    let dep_toml: String = super::emit_manifest_dependencies(&bundle.dependencies);

    format!(
        "# THIS FILE IS AUTO-GENERATED BY polyplugc. DO NOT EDIT.\n\
         name = \"{name}\"\n\
         id = {bundle_id}\n\
         version = \"{version}\"\n\
         loader = \"{loader}\"\n\
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

fn generate_callers_ts(ir: &ValidatedIr) -> Result<String, PolyplugcError> {
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

    // Argument/return marshalling allocates StringView/Buffer payloads through
    // the host allocator and packs values into C-layout buffers (see the per-
    // method callers below).
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
        generate_host_caller_class_quickjs(&mut out, ir, contract)?;
    }

    Ok(out)
}

fn generate_host_caller_class_quickjs(
    out: &mut String,
    ir: &ValidatedIr,
    contract: &ResolvedContract,
) -> Result<(), PolyplugcError> {
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
        generate_host_caller_method_deno(out, ir, func)?;
    }

    out.push_str("}\n\n");
    Ok(())
}

/// Generate one Deno host-caller method with full ABI marshalling.
///
/// Supports every caller shape the IR can express: primitives (incl. `u64`/`i64`
/// as `bigint`), `bool`, enums (read and written at their repr width, UNSIGNED),
/// `StringView` (`string`), `Buffer` (`Uint8Array`), and one-level structs whose
/// fields are any of the above. Arguments are packed into a single C-layout
/// struct buffer; the return value is read back from a C-layout out buffer.
/// `StringView`/`Buffer` payloads (arguments and struct fields) are allocated
/// through the host allocator and freed after dispatch; returned
/// `StringView`/`Buffer` payloads are copied out and freed.
fn generate_host_caller_method_deno(
    out: &mut String,
    ir: &ValidatedIr,
    func: &ResolvedFunction,
) -> Result<(), PolyplugcError> {
    let fn_id: u32 = func.function_id;

    let mut param_decls: Vec<String> = Vec::with_capacity(func.params.len());
    for param in &func.params {
        let ts_ty: String = deno_caller_ts_type(&param.ty, ir)?;
        param_decls.push(format!("{}: {}", param.name, ts_ty));
    }
    let ret_ts: String = match &func.returns {
        Some(ty) => deno_caller_ts_type(ty, ir)?,
        None => "void".to_owned(),
    };

    out.push_str(&format!("    /** Call `{}` */\n", func.name));
    out.push_str(&format!(
        "    {}({}): {} {{\n",
        func.name,
        param_decls.join(", "),
        ret_ts
    ));

    // Validate the function index against the interface's reported function count.
    // VM-dispatch interfaces report a count of 0 (the VM routes by fn_id itself),
    // so only enforce the bound for native interfaces that report a real count.
    out.push_str(&format!(
        "        if (this.#view.functionCount() > 0 && {fn_id} >= this.#view.functionCount()) {{\n"
    ));
    out.push_str(&format!(
        "            throw new Error('function `{}` not available in interface');\n",
        func.name
    ));
    out.push_str("        }\n");

    out.push_str("        const rt = this.#rt;\n");
    // Every host_alloc made while packing arguments, freed after dispatch.
    out.push_str("        const _allocs: [Deno.PointerValue, number][] = [];\n");

    // Pack arguments into a single C-layout struct buffer.
    let args_size: usize = deno_args_total_size(func, ir)?;
    out.push_str(&format!(
        "        const argsBuf = new Uint8Array({});\n",
        args_size.max(1)
    ));
    if !func.params.is_empty() {
        out.push_str("        const argsDv = new DataView(argsBuf.buffer);\n");
        let mut offset: usize = 0;
        let mut alloc_idx: u32 = 0;
        for param in &func.params {
            let align: usize = js_c_align(&param.ty, ir)?;
            offset = align_up(offset, align);
            emit_deno_write_value(
                out,
                &param.ty,
                "argsDv",
                offset,
                &param.name,
                ir,
                &mut alloc_idx,
            )?;
            offset += js_c_size(&param.ty, ir)?;
        }
    }
    out.push_str("        const argsPtr = Deno.UnsafePointer.of(argsBuf);\n");

    // Out buffer sized to the return type (or one byte for void).
    let ret_size: usize = match &func.returns {
        Some(ty) => js_c_size(ty, ir)?,
        None => 0,
    };
    out.push_str(&format!(
        "        const outBuf = new Uint8Array({});\n",
        ret_size.max(1)
    ));
    out.push_str("        const outPtr = Deno.UnsafePointer.of(outBuf);\n");

    // Dispatch through the resolved interface (native or VM).
    out.push_str(&format!(
        "        const code = this.#view.dispatch({fn_id}, this.#instance, argsPtr, outPtr);\n"
    ));
    // Release argument payloads regardless of outcome.
    out.push_str("        for (const [_p, _s] of _allocs) { rt.free(_p, _s, 1); }\n");
    out.push_str("        if (code !== AbiErrorCode.Ok) {\n");
    out.push_str(&format!(
        "            throw new Error('call `{}` failed: AbiError code ' + code);\n",
        func.name
    ));
    out.push_str("        }\n");

    if let Some(ret_ty) = &func.returns {
        out.push_str("        const outDv = new DataView(outBuf.buffer);\n");
        let mut read_idx: u32 = 0;
        // The caller OWNS the guest's return payload (the guest host-allocated it
        // for us), so the read frees it after copying out.
        let local: String = emit_deno_read_local(out, ret_ty, "outDv", 0, ir, &mut read_idx, true)?;
        out.push_str(&format!("        return {local};\n"));
    }

    out.push_str("    }\n\n");
    Ok(())
}

/// Ergonomic TypeScript type for a Deno host-caller parameter or return value.
/// Enums travel as their numeric repr value (`number`); structs become an inline
/// object type so the generated file stays import-free and `deno check`-clean.
fn deno_caller_ts_type(ty: &ResolvedTypeRef, ir: &ValidatedIr) -> Result<String, PolyplugcError> {
    match ty {
        ResolvedTypeRef::Primitive(p) => Ok(match p {
            PrimitiveType::U8
            | PrimitiveType::U16
            | PrimitiveType::U32
            | PrimitiveType::I8
            | PrimitiveType::I16
            | PrimitiveType::I32
            | PrimitiveType::F32
            | PrimitiveType::F64 => "number".to_owned(),
            PrimitiveType::Bool => "boolean".to_owned(),
            PrimitiveType::U64 | PrimitiveType::I64 => "bigint".to_owned(),
        }),
        ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => Ok("string".to_owned()),
        ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => Ok("Uint8Array".to_owned()),
        ResolvedTypeRef::AbiType(AbiBuiltin::Ptr) => Ok("bigint".to_owned()),
        ResolvedTypeRef::AbiType(AbiBuiltin::Void) => Ok("void".to_owned()),
        ResolvedTypeRef::UserDefined(name) => {
            if js_enum_for_type(ty, &ir.enums).is_some() {
                return Ok("number".to_owned());
            }
            if let Some(s) = js_struct_for_type(ty, &ir.types) {
                let mut fields: Vec<String> = Vec::with_capacity(s.fields.len());
                for field in &s.fields {
                    // Struct-typed fields recurse into a nested inline object type
                    // ({ a: { b: number } }); the C layout is computed by
                    // js_c_size/js_c_align, so nesting depth is unbounded.
                    let field_ts: String = deno_caller_ts_type(&field.ty, ir)?;
                    fields.push(format!("{}: {}", field.name, field_ts));
                }
                return Ok(format!("{{ {} }}", fields.join("; ")));
            }
            Err(PolyplugcError::UnsupportedType {
                type_name: name.clone(),
                lang: "js-quickjs".to_owned(),
            })
        }
    }
}

/// Total C-layout size of a function's argument struct (natural alignment, tail
/// padded to the struct's own alignment) — matches every other host caller.
fn deno_args_total_size(
    func: &ResolvedFunction,
    ir: &ValidatedIr,
) -> Result<usize, PolyplugcError> {
    let mut offset: usize = 0;
    let mut max_align: usize = 1;
    for param in &func.params {
        let align: usize = js_c_align(&param.ty, ir)?;
        if align > max_align {
            max_align = align;
        }
        offset = align_up(offset, align);
        offset += js_c_size(&param.ty, ir)?;
    }
    Ok(align_up(offset, max_align))
}

/// Emit statements writing `value` (a JS expression of `ty`'s ergonomic shape)
/// into `dv` at `off` using Deno-FFI primitives. StringView/Buffer payloads are
/// host-allocated and pushed onto `_allocs` for later release; structs recurse
/// field-by-field to unbounded nesting depth. Integers/enums are written at their
/// exact repr width (enums UNSIGNED).
fn emit_deno_write_value(
    out: &mut String,
    ty: &ResolvedTypeRef,
    dv: &str,
    off: usize,
    value: &str,
    ir: &ValidatedIr,
    alloc_idx: &mut u32,
) -> Result<(), PolyplugcError> {
    match ty {
        ResolvedTypeRef::Primitive(p) => {
            match p {
                PrimitiveType::U8 => out.push_str(&format!(
                    "        {dv}.setUint8({off}, Number({value}) & 0xFF);\n"
                )),
                PrimitiveType::I8 => {
                    out.push_str(&format!("        {dv}.setInt8({off}, Number({value}));\n"))
                }
                PrimitiveType::U16 => out.push_str(&format!(
                    "        {dv}.setUint16({off}, Number({value}) & 0xFFFF, true);\n"
                )),
                PrimitiveType::I16 => out.push_str(&format!(
                    "        {dv}.setInt16({off}, Number({value}), true);\n"
                )),
                PrimitiveType::U32 => out.push_str(&format!(
                    "        {dv}.setUint32({off}, Number({value}) >>> 0, true);\n"
                )),
                PrimitiveType::I32 => out.push_str(&format!(
                    "        {dv}.setInt32({off}, Number({value}) | 0, true);\n"
                )),
                PrimitiveType::F32 => out.push_str(&format!(
                    "        {dv}.setFloat32({off}, Number({value}), true);\n"
                )),
                PrimitiveType::F64 => out.push_str(&format!(
                    "        {dv}.setFloat64({off}, Number({value}), true);\n"
                )),
                PrimitiveType::Bool => {
                    out.push_str(&format!("        {dv}.setUint8({off}, {value} ? 1 : 0);\n"))
                }
                PrimitiveType::U64 => out.push_str(&format!(
                    "        {dv}.setBigUint64({off}, BigInt({value}), true);\n"
                )),
                PrimitiveType::I64 => out.push_str(&format!(
                    "        {dv}.setBigInt64({off}, BigInt({value}), true);\n"
                )),
            }
            Ok(())
        }
        ResolvedTypeRef::AbiType(AbiBuiltin::Ptr) => {
            out.push_str(&format!(
                "        {dv}.setBigUint64({off}, BigInt({value}), true);\n"
            ));
            Ok(())
        }
        ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => {
            emit_deno_write_stringview(out, dv, off, value, alloc_idx);
            Ok(())
        }
        ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => {
            emit_deno_write_buffer(out, dv, off, value, alloc_idx);
            Ok(())
        }
        ResolvedTypeRef::AbiType(AbiBuiltin::Void) => Ok(()),
        ResolvedTypeRef::UserDefined(name) => {
            if let Some(e) = js_enum_for_type(ty, &ir.enums) {
                match e.repr {
                    ReprType::U8 => out.push_str(&format!(
                        "        {dv}.setUint8({off}, Number({value}) & 0xFF);\n"
                    )),
                    ReprType::U16 => out.push_str(&format!(
                        "        {dv}.setUint16({off}, Number({value}) & 0xFFFF, true);\n"
                    )),
                    ReprType::U32 => out.push_str(&format!(
                        "        {dv}.setUint32({off}, Number({value}) >>> 0, true);\n"
                    )),
                    ReprType::U64 => out.push_str(&format!(
                        "        {dv}.setBigUint64({off}, BigInt({value}), true);\n"
                    )),
                }
                return Ok(());
            }
            if let Some(s) = js_struct_for_type(ty, &ir.types) {
                let mut field_off: usize = off;
                for field in &s.fields {
                    let align: usize = js_c_align(&field.ty, ir)?;
                    field_off = align_up(field_off, align);
                    let field_value: String = format!("{value}.{}", field.name);
                    emit_deno_write_value(
                        out,
                        &field.ty,
                        dv,
                        field_off,
                        &field_value,
                        ir,
                        alloc_idx,
                    )?;
                    field_off += js_c_size(&field.ty, ir)?;
                }
                return Ok(());
            }
            Err(PolyplugcError::UnsupportedType {
                type_name: name.clone(),
                lang: "js-quickjs".to_owned(),
            })
        }
    }
}

/// Host-allocate a StringView payload, copy `value` into it, write the
/// `{ ptr, len }` slot at `off`, and record the allocation in `_allocs`.
fn emit_deno_write_stringview(out: &mut String, dv: &str, off: usize, value: &str, idx: &mut u32) {
    let n: u32 = *idx;
    *idx += 1;
    out.push_str(&format!(
        "        const _sv{n}Bytes = _encoder.encode({value});\n"
    ));
    out.push_str(&format!(
        "        const _sv{n}Alloc = _sv{n}Bytes.length > 0 ? _sv{n}Bytes.length : 1;\n"
    ));
    out.push_str(&format!(
        "        const _sv{n}Ptr = rt.alloc(_sv{n}Alloc, 1);\n"
    ));
    out.push_str(&format!(
        "        if (_sv{n}Ptr === null) {{ throw new Error('host_alloc failed'); }}\n"
    ));
    out.push_str(&format!(
        "        if (_sv{n}Bytes.length > 0) {{ new Uint8Array(Deno.UnsafePointerView.getArrayBuffer(_sv{n}Ptr, _sv{n}Bytes.length)).set(_sv{n}Bytes); }}\n"
    ));
    out.push_str(&format!(
        "        {dv}.setBigUint64({off}, BigInt(Deno.UnsafePointer.value(_sv{n}Ptr)), true);\n"
    ));
    out.push_str(&format!(
        "        {dv}.setBigUint64({}, BigInt(_sv{n}Bytes.length), true);\n",
        off + 8
    ));
    out.push_str(&format!(
        "        _allocs.push([_sv{n}Ptr, _sv{n}Alloc]);\n"
    ));
}

/// Host-allocate a Buffer payload, copy `value` (a Uint8Array) into it, write the
/// `{ ptr, len, cap }` slot at `off` (cap == len), and record the allocation.
fn emit_deno_write_buffer(out: &mut String, dv: &str, off: usize, value: &str, idx: &mut u32) {
    let n: u32 = *idx;
    *idx += 1;
    out.push_str(&format!("        const _bf{n}Data = {value};\n"));
    out.push_str(&format!(
        "        const _bf{n}Alloc = _bf{n}Data.length > 0 ? _bf{n}Data.length : 1;\n"
    ));
    out.push_str(&format!(
        "        const _bf{n}Ptr = rt.alloc(_bf{n}Alloc, 1);\n"
    ));
    out.push_str(&format!(
        "        if (_bf{n}Ptr === null) {{ throw new Error('host_alloc failed'); }}\n"
    ));
    out.push_str(&format!(
        "        if (_bf{n}Data.length > 0) {{ new Uint8Array(Deno.UnsafePointerView.getArrayBuffer(_bf{n}Ptr, _bf{n}Data.length)).set(_bf{n}Data); }}\n"
    ));
    out.push_str(&format!(
        "        {dv}.setBigUint64({off}, BigInt(Deno.UnsafePointer.value(_bf{n}Ptr)), true);\n"
    ));
    out.push_str(&format!(
        "        {dv}.setBigUint64({}, BigInt(_bf{n}Data.length), true);\n",
        off + 8
    ));
    out.push_str(&format!(
        "        {dv}.setBigUint64({}, BigInt(_bf{n}Data.length), true);\n",
        off + 16
    ));
    out.push_str(&format!(
        "        _allocs.push([_bf{n}Ptr, _bf{n}Alloc]);\n"
    ));
}

/// Emit statements reading one ABI value of `ty` from `dv` at `off` into a
/// freshly-declared local, returning that local's name. Scalars/enums read at
/// their exact repr width (enums UNSIGNED); StringView/Buffer payloads are copied
/// out and, when `owns_payload`, freed; structs recurse field-by-field into a
/// nested object literal to unbounded depth.
// Mirrors emit_deno_write_value's irreducible marshalling context (out, type,
// view, offset, ir, counter) plus the ownership flag.
fn emit_deno_read_local(
    out: &mut String,
    ty: &ResolvedTypeRef,
    dv: &str,
    off: usize,
    ir: &ValidatedIr,
    idx: &mut u32,
    owns_payload: bool,
) -> Result<String, PolyplugcError> {
    let n: u32 = *idx;
    *idx += 1;
    let name: String = format!("_r{n}");
    match ty {
        ResolvedTypeRef::Primitive(p) => {
            let expr: String = match p {
                PrimitiveType::U8 => format!("{dv}.getUint8({off})"),
                PrimitiveType::I8 => format!("{dv}.getInt8({off})"),
                PrimitiveType::U16 => format!("{dv}.getUint16({off}, true)"),
                PrimitiveType::I16 => format!("{dv}.getInt16({off}, true)"),
                PrimitiveType::U32 => format!("{dv}.getUint32({off}, true)"),
                PrimitiveType::I32 => format!("{dv}.getInt32({off}, true)"),
                PrimitiveType::F32 => format!("{dv}.getFloat32({off}, true)"),
                PrimitiveType::F64 => format!("{dv}.getFloat64({off}, true)"),
                PrimitiveType::Bool => format!("({dv}.getUint8({off}) !== 0)"),
                PrimitiveType::U64 => format!("{dv}.getBigUint64({off}, true)"),
                PrimitiveType::I64 => format!("{dv}.getBigInt64({off}, true)"),
            };
            out.push_str(&format!("        const {name} = {expr};\n"));
            Ok(name)
        }
        ResolvedTypeRef::AbiType(AbiBuiltin::Ptr) => {
            out.push_str(&format!(
                "        const {name} = {dv}.getBigUint64({off}, true);\n"
            ));
            Ok(name)
        }
        ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => {
            emit_deno_read_stringview(out, dv, off, &name, owns_payload);
            Ok(name)
        }
        ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => {
            emit_deno_read_buffer(out, dv, off, &name, owns_payload);
            Ok(name)
        }
        ResolvedTypeRef::AbiType(AbiBuiltin::Void) => Err(PolyplugcError::UnsupportedType {
            type_name: "void".to_owned(),
            lang: "js-quickjs".to_owned(),
        }),
        ResolvedTypeRef::UserDefined(type_name) => {
            if let Some(e) = js_enum_for_type(ty, &ir.enums) {
                let expr: String = match e.repr {
                    ReprType::U8 => format!("{dv}.getUint8({off})"),
                    ReprType::U16 => format!("{dv}.getUint16({off}, true)"),
                    ReprType::U32 => format!("{dv}.getUint32({off}, true)"),
                    ReprType::U64 => format!("Number({dv}.getBigUint64({off}, true))"),
                };
                out.push_str(&format!("        const {name} = {expr};\n"));
                return Ok(name);
            }
            if let Some(s) = js_struct_for_type(ty, &ir.types) {
                let mut field_off: usize = off;
                let mut members: Vec<String> = Vec::with_capacity(s.fields.len());
                for field in &s.fields {
                    let align: usize = js_c_align(&field.ty, ir)?;
                    field_off = align_up(field_off, align);
                    let field_local: String =
                        emit_deno_read_local(out, &field.ty, dv, field_off, ir, idx, owns_payload)?;
                    members.push(format!("{}: {field_local}", field.name));
                    field_off += js_c_size(&field.ty, ir)?;
                }
                out.push_str(&format!(
                    "        const {name} = {{ {} }};\n",
                    members.join(", ")
                ));
                return Ok(name);
            }
            Err(PolyplugcError::UnsupportedType {
                type_name: type_name.clone(),
                lang: "js-quickjs".to_owned(),
            })
        }
    }
}

/// Read a StringView at `off`, decode it to `name` (a `let` string). Null/empty
/// views decode to `''`. `owns_payload` encodes WHO owns the pointed-to bytes:
/// when true the reader owns them and frees them after copying (a caller reading a
/// guest RETURN it host-allocated for us); when false the reader only borrows them
/// and must not free (a host-contract PROVIDER reading a caller-owned ARG — freeing
/// it would be a use-after-free).
fn emit_deno_read_stringview(
    out: &mut String,
    dv: &str,
    off: usize,
    name: &str,
    owns_payload: bool,
) {
    out.push_str(&format!("        let {name} = '';\n"));
    out.push_str("        {\n");
    out.push_str(&format!(
        "            const _p = {dv}.getBigUint64({off}, true);\n"
    ));
    out.push_str(&format!(
        "            const _l = Number({dv}.getBigUint64({}, true));\n",
        off + 8
    ));
    out.push_str("            if (_p !== 0n && _l > 0) {\n");
    out.push_str("                const _ptr = Deno.UnsafePointer.create(_p);\n");
    out.push_str("                if (_ptr !== null) {\n");
    out.push_str(&format!(
        "                    {name} = _decoder.decode(new Uint8Array(Deno.UnsafePointerView.getArrayBuffer(_ptr, _l)).slice());\n"
    ));
    if owns_payload {
        out.push_str("                    rt.free(_ptr, _l, 1);\n");
    }
    out.push_str("                }\n");
    out.push_str("            }\n");
    out.push_str("        }\n");
}

/// Read a Buffer at `off`, copy it into `name` (a `let` Uint8Array). Null/empty
/// buffers decode to an empty array. `owns_payload` follows the same ownership
/// rule as `emit_deno_read_stringview` (own a guest RETURN → free by capacity;
/// borrow a caller-owned ARG → leave intact).
fn emit_deno_read_buffer(out: &mut String, dv: &str, off: usize, name: &str, owns_payload: bool) {
    out.push_str(&format!("        let {name} = new Uint8Array(0);\n"));
    out.push_str("        {\n");
    out.push_str(&format!(
        "            const _p = {dv}.getBigUint64({off}, true);\n"
    ));
    out.push_str(&format!(
        "            const _l = Number({dv}.getBigUint64({}, true));\n",
        off + 8
    ));
    out.push_str(&format!(
        "            const _c = Number({dv}.getBigUint64({}, true));\n",
        off + 16
    ));
    out.push_str("            if (_p !== 0n && _l > 0) {\n");
    out.push_str("                const _ptr = Deno.UnsafePointer.create(_p);\n");
    out.push_str("                if (_ptr !== null) {\n");
    out.push_str(&format!(
        "                    {name} = new Uint8Array(Deno.UnsafePointerView.getArrayBuffer(_ptr, _l)).slice();\n"
    ));
    if owns_payload {
        out.push_str("                    rt.free(_ptr, _c > 0 ? _c : _l, 1);\n");
    }
    out.push_str("                }\n");
    out.push_str("            }\n");
    out.push_str("        }\n");
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
fn generate_ts_guest_host_contract_caller(
    out: &mut String,
    contract: &ResolvedHostContract,
    ir: &ValidatedIr,
) -> Result<(), PolyplugcError> {
    let class_name: String = host_contract_name_to_ts_caller(&contract.name);
    let contract_id_lo: u32 = (contract.contract_id & 0xFFFFFFFF) as u32;
    let contract_id_hi: u32 = (contract.contract_id >> 32) as u32;

    out.push_str(&format!(
        "/**\n * Guest caller for host contract `{}` (id=0x{:016X})\n */\n",
        contract.name, contract.contract_id
    ));
    out.push_str(&format!("export class {} {{\n", class_name));
    out.push_str("    private _minVersion: number;\n");
    // The bridge and host pointer are threaded in explicitly (no global — Rule 12)
    // and stored as instance state so every method reaches the host through them.
    out.push_str("    private _bridge: any;\n");
    out.push_str("    private _hostPtr: { lo: number; hi: number };\n\n");

    out.push_str(
        "    private constructor(bridge: any, hostPtr: { lo: number; hi: number }, minVersion: number) {\n",
    );
    out.push_str("        this._bridge = bridge;\n");
    out.push_str("        this._hostPtr = hostPtr;\n");
    out.push_str("        this._minVersion = minVersion;\n");
    out.push_str("    }\n\n");

    out.push_str("    /** Factory method - creates caller instance or null if the bridge is unavailable. */\n");
    out.push_str(&format!(
        "    static fromHost(bridge: any, hostPtr: {{ lo: number; hi: number }}, minVersion: number = 0): {} | null {{\n",
        class_name
    ));
    out.push_str("        if (!bridge || !bridge.callHostContract) {\n");
    out.push_str("            return null;\n");
    out.push_str("        }\n");
    out.push_str(&format!(
        "        return new {}(bridge, hostPtr, minVersion);\n",
        class_name
    ));
    out.push_str("    }\n\n");

    out.push_str("    /** Check if the bridge is available. */\n");
    out.push_str("    isValid(): boolean {\n");
    out.push_str("        return !!(this._bridge && this._bridge.callHostContract);\n");
    out.push_str("    }\n\n");

    for func in &contract.functions {
        generate_ts_guest_host_contract_method(out, func, contract_id_lo, contract_id_hi, ir)?;
    }

    out.push_str("}\n\n");
    Ok(())
}

/// Generate one method for a guest-side host contract caller.
fn generate_ts_guest_host_contract_method(
    out: &mut String,
    func: &crate::ir::ResolvedFunction,
    contract_id_lo: u32,
    contract_id_hi: u32,
    ir: &ValidatedIr,
) -> Result<(), PolyplugcError> {
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

    out.push_str("        const polyplug = this._bridge;\n");
    out.push_str("        if (!polyplug || !polyplug.callHostContract) {\n");
    if has_return {
        out.push_str("            return null as any;\n");
    } else {
        out.push_str("            return;\n");
    }
    out.push_str("        }\n");
    emit_ts_caller_alloc_shim(out);
    out.push_str("        try {\n");

    emit_ts_guest_host_contract_args_setup(out, func, ir)?;
    emit_ts_guest_host_contract_out_setup(out, &func.returns, ir)?;

    out.push_str(&format!(
        "        const errCode: number = polyplug.callHostContract(0x{:08X}, 0x{:08X}, this._minVersion, {}, argsPtr, outPtr);\n",
        contract_id_lo, contract_id_hi, fn_id
    ));
    // Throw with the ABI error code (JS host SDK convention) instead of
    // silently returning null and discarding the code.
    out.push_str("        if (errCode !== 0) {\n");
    out.push_str(&format!(
        "            throw new Error(`host contract call {} failed (code ${{errCode}})`);\n",
        func.name
    ));
    out.push_str("        }\n");

    if has_return {
        emit_ts_guest_host_contract_readback(out, func.returns.as_ref(), ir)?;
        out.push_str("        return result;\n");
    }

    emit_ts_caller_free_shim(out);
    out.push_str("    }\n\n");
    Ok(())
}

/// Emit the caller-local host allocator + free-list used for a guest→host /
/// peer-caller method's transient arg/out buffers.
///
/// The caller runs inside a guest dispatch but cannot reach that dispatch's
/// per-call arena (its method signature is author-defined), so — mirroring the
/// lua/python callers, which use FFI stack memory — JS allocates these transient
/// buffers from the HOST allocator and frees them explicitly when the method
/// returns. `_callerAlloc(size)` returns the same `[lo, hi]` pair `arenaAlloc`
/// did, so the offset-writing emitters are unchanged; it records `[lo, hi, size]`
/// on `_frees` so `emit_ts_caller_free_shim`'s `finally` releases every region
/// even if the call throws. A returned StringView/Buffer points at HOST-owned
/// memory (not these buffers), so freeing them after read-back is sound.
fn emit_ts_caller_alloc_shim(out: &mut String) {
    out.push_str("        const _frees: number[][] = [];\n");
    out.push_str(
        "        const _callerAlloc = (sz: number): number[] => { const _a = polyplug.alloc(sz); _frees.push([_a[0], _a[1], sz]); return _a; };\n",
    );
}

/// Emit the `finally` block that frees every region recorded by
/// `_callerAlloc`. Closes the `try` opened by `emit_ts_caller_alloc_shim`.
fn emit_ts_caller_free_shim(out: &mut String) {
    out.push_str("        } finally {\n");
    out.push_str(
        "            for (const _f of _frees) { polyplug.free(_f[0], _f[1], _f[2], 1); }\n",
    );
    out.push_str("        }\n");
}

/// `(size, align)` of one caller-pack slot for the guest→host / peer caller
/// args pack. User-defined ENUM types use their repr width; user-defined
/// non-enum (struct-by-value) params use their full C-layout size/alignment
/// (computed by `js_c_size` / `js_c_align`) and are marshalled field-by-field
/// by `emit_ts_caller_pack_value`, matching the guest-side wrappers and every
/// other host caller.
fn js_caller_slot_layout(
    ty: &ResolvedTypeRef,
    ir: &ValidatedIr,
) -> Result<(usize, usize), PolyplugcError> {
    match ty {
        ResolvedTypeRef::Primitive(p) => Ok(match p {
            PrimitiveType::U8 | PrimitiveType::I8 | PrimitiveType::Bool => (1, 1),
            PrimitiveType::U16 | PrimitiveType::I16 => (2, 2),
            PrimitiveType::U32 | PrimitiveType::I32 | PrimitiveType::F32 => (4, 4),
            PrimitiveType::U64 | PrimitiveType::I64 | PrimitiveType::F64 => (8, 8),
        }),
        ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => Ok((16, 8)),
        ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => Ok((24, 8)),
        ResolvedTypeRef::AbiType(AbiBuiltin::Ptr) => Ok((8, 8)),
        ResolvedTypeRef::AbiType(AbiBuiltin::Void) => Ok((0, 1)),
        ResolvedTypeRef::UserDefined(_) => match js_enum_for_type(ty, &ir.enums) {
            Some(e) => {
                let size: usize = js_repr_size(&e.repr);
                Ok((size, size))
            }
            None => Ok((js_c_size(ty, ir)?, js_c_align(ty, ir)?)),
        },
    }
}

/// Round `offset` up to the next multiple of `align` (C struct layout).
fn align_up(offset: usize, align: usize) -> usize {
    if align == 0 {
        return offset;
    }
    offset.div_ceil(align) * align
}

/// Byte width of an enum's declared repr.
fn js_repr_size(repr: &ReprType) -> usize {
    match repr {
        ReprType::U8 => 1,
        ReprType::U16 => 2,
        ReprType::U32 => 4,
        ReprType::U64 => 8,
    }
}

/// Find the enum definition backing a user-defined type, if it is an enum.
fn js_enum_for_type<'a>(ty: &ResolvedTypeRef, enums: &'a [EnumDef]) -> Option<&'a EnumDef> {
    match ty {
        ResolvedTypeRef::UserDefined(name) => enums.iter().find(|e: &&EnumDef| &e.name == name),
        _ => None,
    }
}

/// Find the struct definition backing a user-defined type, if it is a struct.
fn js_struct_for_type<'a>(
    ty: &ResolvedTypeRef,
    types: &'a [ResolvedType],
) -> Option<&'a ResolvedType> {
    match ty {
        ResolvedTypeRef::UserDefined(name) => {
            types.iter().find(|t: &&ResolvedType| &t.name == name)
        }
        _ => None,
    }
}

/// C-layout alignment of one ABI value as packed by every host-side caller
/// (`#[repr(C)]` / ctypes / LuaJIT cdef / LayoutKind.Sequential all use
/// natural alignment).
fn js_c_align(ty: &ResolvedTypeRef, ir: &ValidatedIr) -> Result<usize, PolyplugcError> {
    match ty {
        ResolvedTypeRef::Primitive(p) => Ok(match p {
            PrimitiveType::U8 | PrimitiveType::I8 | PrimitiveType::Bool => 1,
            PrimitiveType::U16 | PrimitiveType::I16 => 2,
            PrimitiveType::U32 | PrimitiveType::I32 | PrimitiveType::F32 => 4,
            PrimitiveType::U64 | PrimitiveType::I64 | PrimitiveType::F64 => 8,
        }),
        ResolvedTypeRef::AbiType(AbiBuiltin::StringView)
        | ResolvedTypeRef::AbiType(AbiBuiltin::Buffer)
        | ResolvedTypeRef::AbiType(AbiBuiltin::Ptr) => Ok(8),
        ResolvedTypeRef::AbiType(AbiBuiltin::Void) => Ok(1),
        ResolvedTypeRef::UserDefined(name) => {
            if let Some(e) = js_enum_for_type(ty, &ir.enums) {
                return Ok(js_repr_size(&e.repr));
            }
            if let Some(s) = js_struct_for_type(ty, &ir.types) {
                let mut max_align: usize = 1;
                for field in &s.fields {
                    let a: usize = js_c_align(&field.ty, ir)?;
                    if a > max_align {
                        max_align = a;
                    }
                }
                return Ok(max_align);
            }
            Err(PolyplugcError::UnsupportedType {
                type_name: name.clone(),
                lang: "js-quickjs".to_owned(),
            })
        }
    }
}

/// C-layout size of one ABI value as packed by every host-side caller.
fn js_c_size(ty: &ResolvedTypeRef, ir: &ValidatedIr) -> Result<usize, PolyplugcError> {
    match ty {
        ResolvedTypeRef::Primitive(p) => Ok(match p {
            PrimitiveType::U8 | PrimitiveType::I8 | PrimitiveType::Bool => 1,
            PrimitiveType::U16 | PrimitiveType::I16 => 2,
            PrimitiveType::U32 | PrimitiveType::I32 | PrimitiveType::F32 => 4,
            PrimitiveType::U64 | PrimitiveType::I64 | PrimitiveType::F64 => 8,
        }),
        ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => Ok(16),
        ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => Ok(24),
        ResolvedTypeRef::AbiType(AbiBuiltin::Ptr) => Ok(8),
        ResolvedTypeRef::AbiType(AbiBuiltin::Void) => Ok(0),
        ResolvedTypeRef::UserDefined(name) => {
            if let Some(e) = js_enum_for_type(ty, &ir.enums) {
                return Ok(js_repr_size(&e.repr));
            }
            if let Some(s) = js_struct_for_type(ty, &ir.types) {
                let mut offset: usize = 0;
                let mut max_align: usize = 1;
                for field in &s.fields {
                    let a: usize = js_c_align(&field.ty, ir)?;
                    if a > max_align {
                        max_align = a;
                    }
                    offset = align_up(offset, a);
                    offset += js_c_size(&field.ty, ir)?;
                }
                return Ok(align_up(offset, max_align));
            }
            Err(PolyplugcError::UnsupportedType {
                type_name: name.clone(),
                lang: "js-quickjs".to_owned(),
            })
        }
    }
}

/// Render `base + off` as a JS pointer expression (omits `+ 0`).
fn js_ptr_at(base: &str, off: usize) -> String {
    if off == 0 {
        base.to_owned()
    } else {
        format!("{base} + {off}")
    }
}

/// JS expression that reads one ABI value of `ty` at `base + off` through the
/// loader bridge (readByte/readI32/readU32/readF32/readF64). Enums read their
/// repr-width raw integer; user structs read field-by-field into a (possibly
/// nested) object literal to unbounded depth. Narrow signed integers are
/// sign-extended with shift pairs because the bridge only exposes byte and
/// 32-bit reads.
fn js_read_expr(
    ty: &ResolvedTypeRef,
    base: &str,
    off: usize,
    ir: &ValidatedIr,
) -> Result<String, PolyplugcError> {
    let p: String = js_ptr_at(base, off);
    match ty {
        ResolvedTypeRef::Primitive(prim) => Ok(match prim {
            PrimitiveType::U8 => format!("polyplug.readByte({p})"),
            PrimitiveType::I8 => format!("((polyplug.readByte({p}) << 24) >> 24)"),
            PrimitiveType::U16 => format!(
                "(polyplug.readByte({p}) | (polyplug.readByte({p1}) << 8))",
                p1 = js_ptr_at(base, off + 1)
            ),
            PrimitiveType::I16 => format!(
                "(((polyplug.readByte({p}) | (polyplug.readByte({p1}) << 8)) << 16) >> 16)",
                p1 = js_ptr_at(base, off + 1)
            ),
            PrimitiveType::U32 => format!("polyplug.readU32({p})"),
            PrimitiveType::I32 => format!("polyplug.readI32({p})"),
            PrimitiveType::F32 => format!("polyplug.readF32({p})"),
            PrimitiveType::F64 => format!("polyplug.readF64({p})"),
            PrimitiveType::Bool => format!("(polyplug.readByte({p}) !== 0)"),
            PrimitiveType::U64 | PrimitiveType::I64 => format!(
                "{{ lo: polyplug.readU32({p}), hi: polyplug.readU32({p4}) }}",
                p4 = js_ptr_at(base, off + 4)
            ),
        }),
        ResolvedTypeRef::AbiType(AbiBuiltin::Ptr) => Ok(format!(
            "{{ lo: polyplug.readU32({p}), hi: polyplug.readU32({p4}) }}",
            p4 = js_ptr_at(base, off + 4)
        )),
        ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => Ok(format!(
            "{{ ptr_lo: polyplug.readU32({p}), ptr_hi: polyplug.readU32({p4}), len: polyplug.readU32({p8}) }}",
            p4 = js_ptr_at(base, off + 4),
            p8 = js_ptr_at(base, off + 8)
        )),
        ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => Ok(format!(
            "{{ ptr_lo: polyplug.readU32({p}), ptr_hi: polyplug.readU32({p4}), len: polyplug.readU32({p8}), cap: polyplug.readU32({p16}) }}",
            p4 = js_ptr_at(base, off + 4),
            p8 = js_ptr_at(base, off + 8),
            p16 = js_ptr_at(base, off + 16)
        )),
        ResolvedTypeRef::AbiType(AbiBuiltin::Void) => Err(PolyplugcError::UnsupportedType {
            type_name: "void".to_owned(),
            lang: "js-quickjs".to_owned(),
        }),
        ResolvedTypeRef::UserDefined(name) => {
            if let Some(e) = js_enum_for_type(ty, &ir.enums) {
                return Ok(match e.repr {
                    ReprType::U8 => format!("polyplug.readByte({p})"),
                    ReprType::U16 => format!(
                        "(polyplug.readByte({p}) | (polyplug.readByte({p1}) << 8))",
                        p1 = js_ptr_at(base, off + 1)
                    ),
                    ReprType::U32 => format!("polyplug.readU32({p})"),
                    // Exact for values < 2^53 (QuickJS numbers are f64; the
                    // generator's lo/hi convention applies to declared u64
                    // PRIMITIVES, while enum VALUES are plain numbers).
                    ReprType::U64 => format!(
                        "(polyplug.readU32({p}) + polyplug.readU32({p4}) * 4294967296)",
                        p4 = js_ptr_at(base, off + 4)
                    ),
                });
            }
            if let Some(s) = js_struct_for_type(ty, &ir.types) {
                let mut offset: usize = off;
                let mut fields: Vec<String> = Vec::new();
                for field in &s.fields {
                    let a: usize = js_c_align(&field.ty, ir)?;
                    offset = align_up(offset, a);
                    let expr: String = js_read_expr(&field.ty, base, offset, ir)?;
                    fields.push(format!("{}: {}", field.name, expr));
                    offset += js_c_size(&field.ty, ir)?;
                }
                return Ok(format!("{{ {} }}", fields.join(", ")));
            }
            Err(PolyplugcError::UnsupportedType {
                type_name: name.clone(),
                lang: "js-quickjs".to_owned(),
            })
        }
    }
}

/// Emit the dispatch wrapper's return write into `out_ptr` (offset 0).
///
/// For a top-level `StringView` return the author returns a plain JS string and
/// the GENERATED wrapper arena-allocates it via `bridge.arenaAlloc(size,
/// arena_ptr)` using the per-call arena pointer the loader threads in (no per-VM
/// global, no author-side arena — mirrors the lua/python reference). Every other
/// return shape (scalars, Buffer, struct, enum) is written by `emit_js_write_value`
/// from the value the author returns directly.
fn emit_js_guest_return_write(
    out: &mut String,
    ret_ty: &ResolvedTypeRef,
    ir: &ValidatedIr,
) -> Result<(), PolyplugcError> {
    if matches!(ret_ty, ResolvedTypeRef::AbiType(AbiBuiltin::StringView)) {
        // The author returns a plain string; encode it, arena-allocate the bytes
        // from THIS call's arena, and write the StringView into the out slot.
        out.push_str("    const _retBytes = _ppEncodeUtf8(result);\n");
        out.push_str(
            "    const _retBuf = polyplug.arenaAlloc(_retBytes.length > 0 ? _retBytes.length : 1, arena_ptr);\n",
        );
        out.push_str("    const _retPtr = _retBuf[0] + _retBuf[1] * 4294967296;\n");
        out.push_str(
            "    for (let _i = 0; _i < _retBytes.length; _i++) { polyplug.writeByte(_retPtr + _i, _retBytes[_i]); }\n",
        );
        out.push_str("    polyplug.writeU32(out_ptr, _retBuf[0]);\n");
        out.push_str("    polyplug.writeU32(out_ptr + 4, _retBuf[1]);\n");
        out.push_str("    polyplug.writeU32(out_ptr + 8, _retBytes.length);\n");
        out.push_str("    polyplug.writeU32(out_ptr + 12, 0);\n");
        return Ok(());
    }
    emit_js_write_value(out, "    ", ret_ty, "out_ptr", 0, "result", ir)
}

/// Emit statements writing `value` (a JS expression of `ty`'s shape) into the
/// out slot at `base + off` through the loader bridge. The mirror of
/// `js_read_expr` — same C layout, same lo/hi and {ptr_lo,ptr_hi,len} shapes.
fn emit_js_write_value(
    out: &mut String,
    indent: &str,
    ty: &ResolvedTypeRef,
    base: &str,
    off: usize,
    value: &str,
    ir: &ValidatedIr,
) -> Result<(), PolyplugcError> {
    let p: String = js_ptr_at(base, off);
    match ty {
        ResolvedTypeRef::Primitive(prim) => {
            match prim {
                PrimitiveType::U8 | PrimitiveType::I8 => {
                    out.push_str(&format!(
                        "{indent}polyplug.writeByte({p}, {value} & 0xFF);\n"
                    ));
                }
                PrimitiveType::U16 | PrimitiveType::I16 => {
                    out.push_str(&format!(
                        "{indent}polyplug.writeByte({p}, {value} & 0xFF);\n"
                    ));
                    out.push_str(&format!(
                        "{indent}polyplug.writeByte({p1}, ({value} >> 8) & 0xFF);\n",
                        p1 = js_ptr_at(base, off + 1)
                    ));
                }
                PrimitiveType::U32 => {
                    out.push_str(&format!("{indent}polyplug.writeU32({p}, {value});\n"));
                }
                PrimitiveType::I32 => {
                    out.push_str(&format!("{indent}polyplug.writeI32({p}, {value});\n"));
                }
                PrimitiveType::F32 => {
                    out.push_str(&format!("{indent}polyplug.writeF32({p}, {value});\n"));
                }
                PrimitiveType::F64 => {
                    out.push_str(&format!("{indent}polyplug.writeF64({p}, {value});\n"));
                }
                PrimitiveType::Bool => {
                    out.push_str(&format!(
                        "{indent}polyplug.writeByte({p}, {value} ? 1 : 0);\n"
                    ));
                }
                PrimitiveType::U64 | PrimitiveType::I64 => {
                    out.push_str(&format!("{indent}polyplug.writeU32({p}, {value}.lo);\n"));
                    out.push_str(&format!(
                        "{indent}polyplug.writeU32({p4}, {value}.hi);\n",
                        p4 = js_ptr_at(base, off + 4)
                    ));
                }
            }
            Ok(())
        }
        ResolvedTypeRef::AbiType(AbiBuiltin::Ptr) => {
            out.push_str(&format!("{indent}polyplug.writeU32({p}, {value}.lo);\n"));
            out.push_str(&format!(
                "{indent}polyplug.writeU32({p4}, {value}.hi);\n",
                p4 = js_ptr_at(base, off + 4)
            ));
            Ok(())
        }
        ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => {
            out.push_str(&format!(
                "{indent}polyplug.writeU32({p}, {value}.ptr_lo);\n"
            ));
            out.push_str(&format!(
                "{indent}polyplug.writeU32({p4}, {value}.ptr_hi);\n",
                p4 = js_ptr_at(base, off + 4)
            ));
            out.push_str(&format!(
                "{indent}polyplug.writeU32({p8}, {value}.len);\n",
                p8 = js_ptr_at(base, off + 8)
            ));
            // High half of the usize len: zero (lengths are < 2^32).
            out.push_str(&format!(
                "{indent}polyplug.writeU32({p12}, 0);\n",
                p12 = js_ptr_at(base, off + 12)
            ));
            Ok(())
        }
        ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => {
            out.push_str(&format!(
                "{indent}polyplug.writeU32({p}, {value}.ptr_lo);\n"
            ));
            out.push_str(&format!(
                "{indent}polyplug.writeU32({p4}, {value}.ptr_hi);\n",
                p4 = js_ptr_at(base, off + 4)
            ));
            out.push_str(&format!(
                "{indent}polyplug.writeU32({p8}, {value}.len);\n",
                p8 = js_ptr_at(base, off + 8)
            ));
            out.push_str(&format!(
                "{indent}polyplug.writeU32({p12}, 0);\n",
                p12 = js_ptr_at(base, off + 12)
            ));
            out.push_str(&format!(
                "{indent}polyplug.writeU32({p16}, {value}.cap);\n",
                p16 = js_ptr_at(base, off + 16)
            ));
            out.push_str(&format!(
                "{indent}polyplug.writeU32({p20}, 0);\n",
                p20 = js_ptr_at(base, off + 20)
            ));
            Ok(())
        }
        ResolvedTypeRef::AbiType(AbiBuiltin::Void) => Ok(()),
        ResolvedTypeRef::UserDefined(name) => {
            if let Some(e) = js_enum_for_type(ty, &ir.enums) {
                match e.repr {
                    ReprType::U8 => {
                        out.push_str(&format!(
                            "{indent}polyplug.writeByte({p}, Number({value}) & 0xFF);\n"
                        ));
                    }
                    ReprType::U16 => {
                        out.push_str(&format!(
                            "{indent}polyplug.writeByte({p}, Number({value}) & 0xFF);\n"
                        ));
                        out.push_str(&format!(
                            "{indent}polyplug.writeByte({p1}, (Number({value}) >> 8) & 0xFF);\n",
                            p1 = js_ptr_at(base, off + 1)
                        ));
                    }
                    ReprType::U32 => {
                        out.push_str(&format!(
                            "{indent}polyplug.writeU32({p}, Number({value}));\n"
                        ));
                    }
                    ReprType::U64 => {
                        out.push_str(&format!(
                            "{indent}polyplug.writeU32({p}, Number({value}) >>> 0);\n"
                        ));
                        out.push_str(&format!(
                            "{indent}polyplug.writeU32({p4}, Math.floor(Number({value}) / 4294967296));\n",
                            p4 = js_ptr_at(base, off + 4)
                        ));
                    }
                }
                return Ok(());
            }
            if let Some(s) = js_struct_for_type(ty, &ir.types) {
                let mut offset: usize = off;
                for field in &s.fields {
                    let a: usize = js_c_align(&field.ty, ir)?;
                    offset = align_up(offset, a);
                    let field_value: String = format!("{value}.{}", field.name);
                    emit_js_write_value(out, indent, &field.ty, base, offset, &field_value, ir)?;
                    offset += js_c_size(&field.ty, ir)?;
                }
                return Ok(());
            }
            Err(PolyplugcError::UnsupportedType {
                type_name: name.clone(),
                lang: "js-quickjs".to_owned(),
            })
        }
    }
}

/// Emit the file-level `_ppEncodeUtf8(str)` helper.
///
/// The QuickJS loader does NOT provide `TextEncoder` (it is absent in QuickJS —
/// see `sdks/js/guest/polyplug_guest.js`), so generated guest code must never
/// call `new TextEncoder()` unconditionally. This emits a manual UTF-8 encoder
/// (mirroring the SDK's `_encodeUtf8`) guarded by a `typeof TextEncoder` check,
/// so emitted marshalling works in both QuickJS and TextEncoder-bearing runtimes.
fn emit_ts_utf8_encoder_helper(out: &mut String) {
    out.push_str("// UTF-8 encoder usable in QuickJS (where TextEncoder is absent).\n");
    out.push_str("function _ppEncodeUtf8(str: string): Uint8Array {\n");
    out.push_str(
        "    if (typeof TextEncoder !== 'undefined') { return new TextEncoder().encode(str); }\n",
    );
    out.push_str("    const out: number[] = [];\n");
    out.push_str("    for (let i = 0; i < str.length; i++) {\n");
    out.push_str("        let code = str.charCodeAt(i);\n");
    out.push_str("        if (code >= 0xD800 && code <= 0xDBFF) {\n");
    out.push_str("            const low = str.charCodeAt(++i);\n");
    out.push_str("            code = 0x10000 + ((code - 0xD800) << 10) + (low - 0xDC00);\n");
    out.push_str("        }\n");
    out.push_str("        if (code < 0x80) { out.push(code); }\n");
    out.push_str(
        "        else if (code < 0x800) { out.push(0xC0 | (code >> 6), 0x80 | (code & 0x3F)); }\n",
    );
    out.push_str("        else if (code < 0x10000) { out.push(0xE0 | (code >> 12), 0x80 | ((code >> 6) & 0x3F), 0x80 | (code & 0x3F)); }\n");
    out.push_str("        else { out.push(0xF0 | (code >> 18), 0x80 | ((code >> 12) & 0x3F), 0x80 | ((code >> 6) & 0x3F), 0x80 | (code & 0x3F)); }\n");
    out.push_str("    }\n");
    out.push_str("    return new Uint8Array(out);\n");
    out.push_str("}\n\n");
}

/// Emit a StringView into `buf` at `offset` using `_callerAlloc` + writeU32/writeByte.
/// `value` is the JS string EXPRESSION to encode (e.g. `name` or `o.inner.s`);
/// `local` is a valid identifier base for the temporaries (e.g. `name` or `sv0`),
/// kept separate so nested struct fields (whose value expression contains `.`)
/// can still name their temporaries.
fn emit_ts_write_string_view(
    out: &mut String,
    value: &str,
    local: &str,
    args_ptr: &str,
    offset: usize,
) {
    let n: &str = local;
    let ap: &str = args_ptr;
    out.push_str(&format!(
        "        const _{n}Bytes = _ppEncodeUtf8({value});\n"
    ));
    out.push_str(&format!(
        "        const _{n}DataBuf = _callerAlloc(_{n}Bytes.length > 0 ? _{n}Bytes.length : 1);\n"
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

/// Emit a Buffer (Uint8Array) into `buf` at `offset` using `_callerAlloc` + writeU32/writeByte.
/// `value` is the JS Uint8Array EXPRESSION; `local` is the identifier base for the
/// temporaries (see `emit_ts_write_string_view`).
fn emit_ts_write_buffer(out: &mut String, value: &str, local: &str, args_ptr: &str, offset: usize) {
    let n: &str = local;
    let ap: &str = args_ptr;
    out.push_str(&format!(
        "        const _{n}DataBuf = _callerAlloc({value}.length > 0 ? {value}.length : 1);\n"
    ));
    out.push_str(&format!(
        "        const _{n}DataPtr = _{n}DataBuf[0] + _{n}DataBuf[1] * 4294967296;\n"
    ));
    out.push_str(&format!(
        "        for (let _i = 0; _i < {value}.length; _i++) {{ polyplug.writeByte(_{n}DataPtr + _i, {value}[_i]); }}\n"
    ));
    out.push_str(&format!(
        "        polyplug.writeU32({ap} + {offset}, _{n}DataBuf[0]);\n"
    ));
    out.push_str(&format!(
        "        polyplug.writeU32({ap} + {}, _{n}DataBuf[1]);\n",
        offset + 4
    ));
    out.push_str(&format!(
        "        polyplug.writeU32({ap} + {}, {value}.length);\n",
        offset + 8
    ));
    out.push_str(&format!(
        "        polyplug.writeU32({ap} + {}, 0);\n",
        offset + 12
    ));
    out.push_str(&format!(
        "        polyplug.writeU32({ap} + {}, {value}.length);\n",
        offset + 16
    ));
    out.push_str(&format!(
        "        polyplug.writeU32({ap} + {}, 0);\n",
        offset + 20
    ));
}

/// Recursively pack `value` (a JS expression of `ty`'s ergonomic shape) into the
/// caller args buffer at `{args_ptr} + {offset}` through the QuickJS bridge.
///
/// StringView/Buffer leaves are arena-allocated from their string/Uint8Array;
/// struct fields recurse field-by-field at their C-layout offsets to unbounded
/// depth; every other leaf (primitive, Ptr, enum) delegates to
/// `emit_js_write_value`. This mirrors the Deno caller's `emit_deno_write_value`,
/// so the JS guest caller marshals struct-by-value params with no type-support
/// limitation. `sv_idx` yields unique identifier bases for the arena temporaries.
fn emit_ts_caller_pack_value(
    out: &mut String,
    ty: &ResolvedTypeRef,
    args_ptr: &str,
    offset: usize,
    value: &str,
    ir: &ValidatedIr,
    sv_idx: &mut u32,
) -> Result<(), PolyplugcError> {
    match ty {
        ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => {
            let local: String = format!("sv{}", *sv_idx);
            *sv_idx += 1;
            emit_ts_write_string_view(out, value, &local, args_ptr, offset);
            Ok(())
        }
        ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => {
            let local: String = format!("buf{}", *sv_idx);
            *sv_idx += 1;
            emit_ts_write_buffer(out, value, &local, args_ptr, offset);
            Ok(())
        }
        ResolvedTypeRef::UserDefined(_) if js_struct_for_type(ty, &ir.types).is_some() => {
            let s: &ResolvedType = js_struct_for_type(ty, &ir.types).ok_or_else(|| {
                PolyplugcError::UnsupportedType {
                    type_name: "struct".to_owned(),
                    lang: "js-quickjs".to_owned(),
                }
            })?;
            let mut field_off: usize = offset;
            for field in &s.fields {
                let align: usize = js_c_align(&field.ty, ir)?;
                field_off = align_up(field_off, align);
                let field_value: String = format!("{value}.{}", field.name);
                emit_ts_caller_pack_value(
                    out,
                    &field.ty,
                    args_ptr,
                    field_off,
                    &field_value,
                    ir,
                    sv_idx,
                )?;
                field_off += js_c_size(&field.ty, ir)?;
            }
            Ok(())
        }
        _ => emit_js_write_value(out, "        ", ty, args_ptr, offset, value, ir),
    }
}

/// Emit the argsPtr setup for a TypeScript guest host contract / peer method.
///
/// Allocates every transient buffer through `_callerAlloc` (a host-allocator
/// shim that records each region on the method's `_frees` list); the method's
/// `finally` block frees them all. The caller cannot reach the dispatch's
/// per-call arena, so it uses host alloc+free here, mirroring the lua/python
/// callers' FFI-stack buffers (Rule 12 — no per-call global arena read).
fn emit_ts_guest_host_contract_args_setup(
    out: &mut String,
    func: &crate::ir::ResolvedFunction,
    ir: &ValidatedIr,
) -> Result<(), PolyplugcError> {
    let enums: &[EnumDef] = &ir.enums;
    if func.params.is_empty() {
        out.push_str("        const argsPtr = 0;\n");
        return Ok(());
    }

    if func.params.len() == 1 {
        let param: &ResolvedParam = &func.params[0];
        match &param.ty {
            ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => {
                out.push_str(&format!(
                    "        const _{0}Bytes = _ppEncodeUtf8({0});\n",
                    param.name
                ));
                out.push_str(&format!(
                    "        const _{0}DataBuf = _callerAlloc(_{0}Bytes.length > 0 ? _{0}Bytes.length : 1);\n",
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
                out.push_str("        const _argsBuf = _callerAlloc(16);\n");
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
                    "        const _{0}DataBuf = _callerAlloc({0}.length > 0 ? {0}.length : 1);\n",
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
                out.push_str("        const _argsBuf = _callerAlloc(24);\n");
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
                    out.push_str("        const _argsBuf = _callerAlloc(8);\n");
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
                } else if matches!(p, PrimitiveType::F64) {
                    // Floats must keep their bit pattern: writeU32 would
                    // integer-truncate the value (and undersize f64 as 4 bytes).
                    out.push_str("        const _argsBuf = _callerAlloc(8);\n");
                    out.push_str(
                        "        const argsPtr = _argsBuf[0] + _argsBuf[1] * 4294967296;\n",
                    );
                    out.push_str(&format!(
                        "        polyplug.writeF64(argsPtr, {});\n",
                        param.name
                    ));
                } else if matches!(p, PrimitiveType::F32) {
                    out.push_str("        const _argsBuf = _callerAlloc(8);\n");
                    out.push_str(
                        "        const argsPtr = _argsBuf[0] + _argsBuf[1] * 4294967296;\n",
                    );
                    out.push_str(&format!(
                        "        polyplug.writeF32(argsPtr, {});\n",
                        param.name
                    ));
                } else {
                    out.push_str("        const _argsBuf = _callerAlloc(8);\n");
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
                out.push_str("        const _argsBuf = _callerAlloc(8);\n");
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
            ResolvedTypeRef::UserDefined(_)
                if js_struct_for_type(&param.ty, &ir.types).is_some() =>
            {
                // Struct-by-value param: allocate its full C-layout size and pack
                // field-by-field (matches the Deno caller and every host thunk).
                let size: usize = js_c_size(&param.ty, ir)?;
                out.push_str(&format!("        const _argsBuf = _callerAlloc({size});\n"));
                out.push_str("        const argsPtr = _argsBuf[0] + _argsBuf[1] * 4294967296;\n");
                let mut sv_idx: u32 = 0;
                emit_ts_caller_pack_value(
                    out,
                    &param.ty,
                    "argsPtr",
                    0,
                    &param.name,
                    ir,
                    &mut sv_idx,
                )?;
            }
            ResolvedTypeRef::UserDefined(_) => {
                // Enum-backed scalar param. Pack through the repr width in an
                // 8-byte aligned slot: a plain writeU32 would truncate a u64-repr
                // enum's high word. The callee reads exactly its repr width from
                // offset 0 (little-endian), so narrower reprs are covered by the
                // low bytes of the u32 write.
                out.push_str("        const _argsBuf = _callerAlloc(8);\n");
                out.push_str("        const argsPtr = _argsBuf[0] + _argsBuf[1] * 4294967296;\n");
                match js_enum_for_type(&param.ty, enums).map(|e: &EnumDef| &e.repr) {
                    Some(ReprType::U64) => {
                        out.push_str(&format!(
                            "        polyplug.writeU32(argsPtr, Number({0}) >>> 0);\n",
                            param.name
                        ));
                        out.push_str(&format!(
                            "        polyplug.writeU32(argsPtr + 4, Math.floor(Number({0}) / 4294967296));\n",
                            param.name
                        ));
                    }
                    _ => {
                        out.push_str(&format!(
                            "        polyplug.writeU32(argsPtr, Number({0}));\n",
                            param.name
                        ));
                        out.push_str("        polyplug.writeU32(argsPtr + 4, 0);\n");
                    }
                }
            }
            ResolvedTypeRef::AbiType(AbiBuiltin::Void) => {
                out.push_str("        const argsPtr = 0;\n");
            }
        }
        return Ok(());
    }

    // Multiple params: pack with C-layout (repr(C)) offsets. Every host-side
    // thunk unpacks the args through a natural-alignment struct (ctypes /
    // LuaJIT cdef / #[repr(C)] / LayoutKind.Sequential), so the pack must
    // match byte-for-byte — e.g. a u32 followed by a StringView places the
    // view at offset 8 (4 bytes of padding), not offset 4.
    let mut total_size: usize = 0;
    let mut max_align: usize = 1;
    for param in &func.params {
        let (size, align): (usize, usize) = js_caller_slot_layout(&param.ty, ir)?;
        if align > max_align {
            max_align = align;
        }
        total_size = align_up(total_size, align) + size;
    }
    total_size = align_up(total_size, max_align);

    out.push_str(&format!(
        "        const _argsBuf = _callerAlloc({});\n",
        total_size
    ));
    out.push_str("        const argsPtr = _argsBuf[0] + _argsBuf[1] * 4294967296;\n");

    let mut offset: usize = 0;
    let mut sv_idx: u32 = 0;
    for param in &func.params {
        let (size, align): (usize, usize) = js_caller_slot_layout(&param.ty, ir)?;
        offset = align_up(offset, align);
        match &param.ty {
            ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => {
                emit_ts_write_string_view(out, &param.name, &param.name, "argsPtr", offset);
            }
            ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => {
                emit_ts_write_buffer(out, &param.name, &param.name, "argsPtr", offset);
            }
            ResolvedTypeRef::Primitive(p) => match p {
                PrimitiveType::U64 | PrimitiveType::I64 => {
                    out.push_str(&format!(
                        "        polyplug.writeU32({}, {}.lo);\n",
                        js_ptr_at("argsPtr", offset),
                        param.name
                    ));
                    out.push_str(&format!(
                        "        polyplug.writeU32({}, {}.hi);\n",
                        js_ptr_at("argsPtr", offset + 4),
                        param.name
                    ));
                }
                PrimitiveType::F64 => {
                    // Floats keep their bit pattern (writeU32 would truncate)
                    // and f64 occupies a full 8-byte slot in the args pack.
                    out.push_str(&format!(
                        "        polyplug.writeF64({}, {});\n",
                        js_ptr_at("argsPtr", offset),
                        param.name
                    ));
                }
                PrimitiveType::F32 => {
                    out.push_str(&format!(
                        "        polyplug.writeF32({}, {});\n",
                        js_ptr_at("argsPtr", offset),
                        param.name
                    ));
                }
                PrimitiveType::I32 => {
                    out.push_str(&format!(
                        "        polyplug.writeI32({}, {});\n",
                        js_ptr_at("argsPtr", offset),
                        param.name
                    ));
                }
                PrimitiveType::U32 => {
                    out.push_str(&format!(
                        "        polyplug.writeU32({}, {});\n",
                        js_ptr_at("argsPtr", offset),
                        param.name
                    ));
                }
                PrimitiveType::U8 | PrimitiveType::I8 => {
                    out.push_str(&format!(
                        "        polyplug.writeByte({}, {} & 0xFF);\n",
                        js_ptr_at("argsPtr", offset),
                        param.name
                    ));
                }
                PrimitiveType::U16 | PrimitiveType::I16 => {
                    out.push_str(&format!(
                        "        polyplug.writeByte({}, {} & 0xFF);\n",
                        js_ptr_at("argsPtr", offset),
                        param.name
                    ));
                    out.push_str(&format!(
                        "        polyplug.writeByte({}, ({} >> 8) & 0xFF);\n",
                        js_ptr_at("argsPtr", offset + 1),
                        param.name
                    ));
                }
                PrimitiveType::Bool => {
                    out.push_str(&format!(
                        "        polyplug.writeByte({}, {} ? 1 : 0);\n",
                        js_ptr_at("argsPtr", offset),
                        param.name
                    ));
                }
            },
            ResolvedTypeRef::AbiType(AbiBuiltin::Ptr) => {
                out.push_str(&format!(
                    "        polyplug.writeU32({}, {}.lo);\n",
                    js_ptr_at("argsPtr", offset),
                    param.name
                ));
                out.push_str(&format!(
                    "        polyplug.writeU32({}, {}.hi);\n",
                    js_ptr_at("argsPtr", offset + 4),
                    param.name
                ));
            }
            ResolvedTypeRef::UserDefined(_) => {
                match js_enum_for_type(&param.ty, enums) {
                    Some(e) => match e.repr {
                        ReprType::U8 => {
                            out.push_str(&format!(
                                "        polyplug.writeByte({}, Number({}) & 0xFF);\n",
                                js_ptr_at("argsPtr", offset),
                                param.name
                            ));
                        }
                        ReprType::U16 => {
                            out.push_str(&format!(
                                "        polyplug.writeByte({}, Number({}) & 0xFF);\n",
                                js_ptr_at("argsPtr", offset),
                                param.name
                            ));
                            out.push_str(&format!(
                                "        polyplug.writeByte({}, (Number({}) >> 8) & 0xFF);\n",
                                js_ptr_at("argsPtr", offset + 1),
                                param.name
                            ));
                        }
                        ReprType::U32 => {
                            out.push_str(&format!(
                                "        polyplug.writeU32({}, Number({}));\n",
                                js_ptr_at("argsPtr", offset),
                                param.name
                            ));
                        }
                        ReprType::U64 => {
                            out.push_str(&format!(
                                "        polyplug.writeU32({}, Number({}) >>> 0);\n",
                                js_ptr_at("argsPtr", offset),
                                param.name
                            ));
                            out.push_str(&format!(
                                "        polyplug.writeU32({}, Math.floor(Number({}) / 4294967296));\n",
                                js_ptr_at("argsPtr", offset + 4),
                                param.name
                            ));
                        }
                    },
                    None => {
                        // Struct-by-value caller param: pack field-by-field at its
                        // C-layout offsets (matches the Deno caller and host thunks).
                        emit_ts_caller_pack_value(
                            out,
                            &param.ty,
                            "argsPtr",
                            offset,
                            &param.name,
                            ir,
                            &mut sv_idx,
                        )?;
                    }
                }
            }
            ResolvedTypeRef::AbiType(AbiBuiltin::Void) => {}
        }
        offset += size;
    }
    Ok(())
}

/// Emit the outPtr setup for a TypeScript guest host contract / peer method.
///
/// Allocates the correct size via `_callerAlloc` (host alloc, freed by the
/// method's `finally`) and defines `const outPtr`.
/// Does NOT pre-create `result` — that is done by `emit_ts_guest_host_contract_readback`
/// after the dispatch call succeeds.
fn emit_ts_guest_host_contract_out_setup(
    out: &mut String,
    returns: &Option<ResolvedTypeRef>,
    ir: &ValidatedIr,
) -> Result<(), PolyplugcError> {
    if let Some(ret_ty) = returns {
        match ret_ty {
            ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => {
                out.push_str("        const _outBuf = _callerAlloc(16);\n");
                out.push_str("        const outPtr = _outBuf[0] + _outBuf[1] * 4294967296;\n");
                out.push_str("        polyplug.writeU32(outPtr, 0);\n");
                out.push_str("        polyplug.writeU32(outPtr + 4, 0);\n");
                out.push_str("        polyplug.writeU32(outPtr + 8, 0);\n");
                out.push_str("        polyplug.writeU32(outPtr + 12, 0);\n");
            }
            ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => {
                out.push_str("        const _outBuf = _callerAlloc(24);\n");
                out.push_str("        const outPtr = _outBuf[0] + _outBuf[1] * 4294967296;\n");
                out.push_str("        polyplug.writeU32(outPtr, 0);\n");
                out.push_str("        polyplug.writeU32(outPtr + 4, 0);\n");
                out.push_str("        polyplug.writeU32(outPtr + 8, 0);\n");
                out.push_str("        polyplug.writeU32(outPtr + 12, 0);\n");
                out.push_str("        polyplug.writeU32(outPtr + 16, 0);\n");
                out.push_str("        polyplug.writeU32(outPtr + 20, 0);\n");
            }
            ResolvedTypeRef::UserDefined(_) if js_struct_for_type(ret_ty, &ir.types).is_some() => {
                // Struct-by-value return: allocate the exact C-layout size and
                // zero the slot word-by-word (arena memory is not pre-zeroed); the
                // host writes each field, and the readback reads them at their C
                // offsets via js_read_expr.
                let size: usize = js_c_size(ret_ty, ir)?;
                out.push_str(&format!("        const _outBuf = _callerAlloc({size});\n"));
                out.push_str("        const outPtr = _outBuf[0] + _outBuf[1] * 4294967296;\n");
                for w in 0..size.div_ceil(4) {
                    out.push_str(&format!(
                        "        polyplug.writeU32({}, 0);\n",
                        js_ptr_at("outPtr", w * 4)
                    ));
                }
            }
            ResolvedTypeRef::UserDefined(_) => {
                // Enum-backed return: 8-byte slot, pre-zeroed; the host writes the
                // repr-width integer and the readback reads it back.
                out.push_str("        const _outBuf = _callerAlloc(8);\n");
                out.push_str("        const outPtr = _outBuf[0] + _outBuf[1] * 4294967296;\n");
                out.push_str("        polyplug.writeU32(outPtr, 0);\n");
                out.push_str("        polyplug.writeU32(outPtr + 4, 0);\n");
            }
            ResolvedTypeRef::Primitive(p) => {
                if matches!(p, PrimitiveType::U64 | PrimitiveType::I64) {
                    out.push_str("        const _outBuf = _callerAlloc(8);\n");
                    out.push_str("        const outPtr = _outBuf[0] + _outBuf[1] * 4294967296;\n");
                    out.push_str("        polyplug.writeU32(outPtr, 0);\n");
                    out.push_str("        polyplug.writeU32(outPtr + 4, 0);\n");
                } else {
                    // Allocate 8 bytes for safety even though the value is 4 bytes.
                    out.push_str("        const _outBuf = _callerAlloc(8);\n");
                    out.push_str("        const outPtr = _outBuf[0] + _outBuf[1] * 4294967296;\n");
                    out.push_str("        polyplug.writeU32(outPtr, 0);\n");
                    out.push_str("        polyplug.writeU32(outPtr + 4, 0);\n");
                }
            }
            ResolvedTypeRef::AbiType(AbiBuiltin::Ptr) => {
                out.push_str("        const _outBuf = _callerAlloc(8);\n");
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
    Ok(())
}

/// Emit `const result = ...;` by reading the dispatch result back from `outPtr`.
///
/// Called after the dispatch call succeeds (errCode === 0).  The `returns` value
/// must NOT be `None` or `Void` — callers must guard against that.
fn emit_ts_guest_host_contract_readback(
    out: &mut String,
    returns: Option<&ResolvedTypeRef>,
    ir: &ValidatedIr,
) -> Result<(), PolyplugcError> {
    let enums: &[EnumDef] = &ir.enums;
    let Some(ret_ty) = returns else {
        return Ok(());
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
            PrimitiveType::I32 => {
                out.push_str("        const result: number = polyplug.readI32(outPtr);\n");
            }
            PrimitiveType::I8 => {
                // The host writes 1 byte into the (pre-zeroed) out slot; a
                // 32-bit read loses the sign — sign-extend the low byte.
                out.push_str(
                    "        const result: number = ((polyplug.readByte(outPtr) << 24) >> 24);\n",
                );
            }
            PrimitiveType::I16 => {
                out.push_str(
                    "        const result: number = (((polyplug.readByte(outPtr) | (polyplug.readByte(outPtr + 1) << 8)) << 16) >> 16);\n",
                );
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
        ResolvedTypeRef::UserDefined(_) if js_struct_for_type(ret_ty, &ir.types).is_some() => {
            // Struct-by-value return: read field-by-field from outPtr into a
            // (possibly nested) object literal via js_read_expr — the same reader
            // the guest wrappers use, so the shape matches the raw return type.
            let expr: String = js_read_expr(ret_ty, "outPtr", 0, ir)?;
            out.push_str(&format!("        const result = {expr};\n"));
        }
        ResolvedTypeRef::UserDefined(_) => {
            // Enum returns read back as their repr integer — the declared TS
            // return type is the numeric enum itself, NOT the {lo, hi} object.
            // The out slot is pre-zeroed by the out-setup, so narrower reprs
            // read correctly through readU32.
            match js_enum_for_type(ret_ty, enums).map(|e: &EnumDef| &e.repr) {
                Some(ReprType::U64) => {
                    out.push_str(
                        "        const result = (polyplug.readU32(outPtr) + polyplug.readU32(outPtr + 4) * 4294967296) as any;\n",
                    );
                }
                Some(_) => {
                    out.push_str("        const result = polyplug.readU32(outPtr) as any;\n");
                }
                None => {
                    out.push_str(
                        "        const result = { lo: polyplug.readU32(outPtr), hi: polyplug.readU32(outPtr + 4) } as any;\n",
                    );
                }
            }
        }
        ResolvedTypeRef::AbiType(AbiBuiltin::Void) => {}
    }
    Ok(())
}

/// Generate `guest/host_contracts.ts` — caller classes for guest-side host contract callers.
fn generate_guest_host_contracts_ts(ir: &ValidatedIr) -> Result<String, PolyplugcError> {
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
    emit_ts_utf8_encoder_helper(&mut out);

    for contract in &ir.host_contracts {
        generate_ts_guest_host_contract_caller(&mut out, contract, ir)?;
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

    Ok(out)
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
fn generate_js_host_interface_factories_ts(ir: &ValidatedIr) -> Result<String, PolyplugcError> {
    let mut out: String = String::new();
    out.push_str(
        "// THIS FILE IS AUTO-GENERATED BY polyplugc\n\
         // DO NOT EDIT BY HAND\n\
         // Runtime: js-quickjs (host-side interface factories)\n\n",
    );

    out.push_str("import { buildHostContractInterface } from 'polyplug';\n");
    out.push_str("import type { Runtime } from 'polyplug';\n");
    out.push_str("import type * as contracts from './contracts';\n\n");

    out.push_str("// ABI error codes (match polyplug_abi.AbiErrorCode)\n");
    out.push_str("const AbiErrorCode = {\n");
    out.push_str("    Ok: 0,\n");
    out.push_str("};\n\n");

    // Shared by the generated arg/return marshalling (StringView encode/decode).
    out.push_str("const _encoder = new TextEncoder();\n");
    out.push_str("const _decoder = new TextDecoder();\n\n");

    for contract in &ir.host_contracts {
        generate_js_host_interface_factory(&mut out, contract, ir)?;
    }

    Ok(out)
}

/// Generate the per-contract host interface factory.
///
/// Emits a factory-based provider: `create<Iface>Vtable(factory)` builds a real
/// C `HostContractInterface` (native dispatch via `Deno.UnsafeCallback`) with
/// per-instance state through the SDK's `buildHostContractInterface`. The
/// `factory` constructs a fresh implementation per instance — no module-level
/// impl storage (Rule 12). Each method is emitted as a thunk that reads its
/// packed args at C-layout offsets and calls the impl.
fn generate_js_host_interface_factory(
    out: &mut String,
    contract: &ResolvedHostContract,
    ir: &ValidatedIr,
) -> Result<(), PolyplugcError> {
    let iface_name: String = host_contract_name_to_ts_interface(&contract.name);
    let factory_name: String = format!("create{}Vtable", iface_name);
    let contract_id: u64 = contract.contract_id;
    let contract_id_lo: u32 = (contract_id & 0xFFFFFFFF) as u32;
    let contract_id_hi: u32 = (contract_id >> 32) as u32;
    let major: u32 = contract.version.major;
    let minor: u32 = contract.version.minor;
    let singleton: bool = contract.singleton;

    out.push_str(&format!(
        "/**\n \
         * Build the host contract interface for `{}` (native dispatch, per-instance).\n \
         * `factory` builds a fresh implementation per instance; the runtime calls it\n \
         * once per non-singleton caller (independent state) or once for a singleton.\n \
         */\n",
        contract.name
    ));
    out.push_str(&format!(
        "export function {factory_name}(rt: Runtime, factory: () => contracts.{iface_name}) {{\n"
    ));
    out.push_str("    return buildHostContractInterface({\n");
    out.push_str(&format!("        contractIdLo: 0x{contract_id_lo:08X},\n"));
    out.push_str(&format!("        contractIdHi: 0x{contract_id_hi:08X},\n"));
    out.push_str(&format!("        major: {major},\n"));
    out.push_str(&format!("        minor: {minor},\n"));
    out.push_str(&format!("        singleton: {singleton},\n"));
    out.push_str("        factory,\n");
    out.push_str("        methods: [\n");
    for func in &contract.functions {
        generate_js_host_method_thunk(out, func, &iface_name, ir)?;
    }
    out.push_str("        ],\n");
    out.push_str("    });\n");
    out.push_str("}\n\n");
    Ok(())
}

/// Emit one native-dispatch thunk for a host contract method, as an entry in the
/// `methods` array passed to `buildHostContractInterface`. The thunk receives the
/// resolved impl and the packed-args pointer, reads each argument at its
/// natural-alignment (C-layout) offset — matching how the guest caller packs them
/// — calls the impl method, and returns `AbiErrorCode.Ok`.
fn generate_js_host_method_thunk(
    out: &mut String,
    func: &ResolvedFunction,
    iface_name: &str,
    ir: &ValidatedIr,
) -> Result<(), PolyplugcError> {
    let method_name: String = host_method_pascal_case(&func.name);

    out.push_str(&format!(
        "            (impl: contracts.{iface_name}, argsPtr: Deno.PointerValue, outPtr: Deno.PointerValue): number => {{\n"
    ));

    // Read every argument at its natural-alignment (C-layout) offset — the exact
    // packing the guest caller produced — using the shared full-universe reader.
    // `owns_payload = false`: the args belong to the caller, so StringView/Buffer
    // payloads are borrowed (copied out, never freed).
    let mut arg_names: Vec<String> = Vec::new();
    if !func.params.is_empty() {
        let args_size: usize = deno_args_total_size(func, ir)?;
        out.push_str(&format!(
            "                const _argsDv = new DataView(new Deno.UnsafePointerView(argsPtr!).getArrayBuffer({args_size}));\n"
        ));
        let mut read_idx: u32 = 0;
        let mut offset: usize = 0;
        for param in &func.params {
            let align: usize = js_c_align(&param.ty, ir)?;
            offset = align_up(offset, align);
            let local: String =
                emit_deno_read_local(out, &param.ty, "_argsDv", offset, ir, &mut read_idx, false)?;
            arg_names.push(local);
            offset += js_c_size(&param.ty, ir)?;
        }
    }

    match &func.returns {
        Some(ret_ty) => {
            out.push_str(&format!(
                "                const _result = impl.{method_name}({});\n",
                arg_names.join(", ")
            ));
            // Write the return through outPtr with the shared full-universe writer.
            // StringView/Buffer payloads are host-allocated here and freed later by
            // the guest caller, so the per-call `_allocs` list is intentionally NOT
            // freed by the provider (freeing it would be a use-after-free).
            let ret_size: usize = js_c_size(ret_ty, ir)?;
            out.push_str(&format!(
                "                const _outDv = new DataView(new Deno.UnsafePointerView(outPtr!).getArrayBuffer({ret_size}));\n"
            ));
            out.push_str("                const _allocs: [Deno.PointerValue, number][] = [];\n");
            let mut alloc_idx: u32 = 0;
            emit_deno_write_value(out, ret_ty, "_outDv", 0, "_result", ir, &mut alloc_idx)?;
            out.push_str("                void _allocs;\n");
        }
        None => {
            out.push_str(&format!(
                "                impl.{method_name}({});\n",
                arg_names.join(", ")
            ));
        }
    }
    out.push_str("                return AbiErrorCode.Ok;\n");
    out.push_str("            },\n");
    Ok(())
}

/// Convert a `snake_case` (or dotted) method name to the `PascalCase` form the
/// generated TypeScript host interface declares.
fn host_method_pascal_case(name: &str) -> String {
    name.split(['_', '.'])
        .filter(|seg: &&str| !seg.is_empty())
        .map(|seg: &str| {
            let mut chars: core::str::Chars<'_> = seg.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().to_string() + chars.as_str(),
            }
        })
        .collect::<Vec<String>>()
        .join("")
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
fn generate_guest_peer_callers_ts(
    ir: &ValidatedIr,
    peers: &[&ResolvedContract],
) -> Result<String, PolyplugcError> {
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
    emit_ts_utf8_encoder_helper(&mut out);

    for contract in peers {
        let min_ver: u32 = peer_min_version(ir, contract.contract_id);
        generate_ts_peer_caller_class(&mut out, contract, min_ver, ir)?;
    }

    Ok(out)
}

/// Generate one `<Name>Peer` class for a single peer contract.
fn generate_ts_peer_caller_class(
    out: &mut String,
    contract: &ResolvedContract,
    min_version: u32,
    ir: &ValidatedIr,
) -> Result<(), PolyplugcError> {
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

    // The bridge and host pointer are threaded in explicitly (no global — Rule 12)
    // and stored as instance state so every method reaches the host through them.
    out.push_str("    private _bridge: any;\n");
    out.push_str("    private _hostPtr: { lo: number; hi: number };\n\n");

    out.push_str("    private constructor(bridge: any, hostPtr: { lo: number; hi: number }) {\n");
    out.push_str("        this._bridge = bridge;\n");
    out.push_str("        this._hostPtr = hostPtr;\n");
    out.push_str("    }\n\n");

    out.push_str("    /**\n");
    out.push_str("     * Verify the peer contract is reachable via the host.\n");
    out.push_str("     * Returns a `");
    out.push_str(&class_name);
    out.push_str("` instance or `null` if not found.\n");
    out.push_str("     *\n");
    out.push_str("     * `bridge` and `hostPtr` are threaded in explicitly by the caller\n");
    out.push_str("     * (the author factory captured them); no per-VM global is read.\n");
    out.push_str("     */\n");
    out.push_str(&format!(
        "    static resolve(bridge: any, hostPtr: {{ lo: number; hi: number }}): {} | null {{\n",
        class_name
    ));
    out.push_str("        if (!bridge || !bridge.findByContract) {\n");
    out.push_str("            return null;\n");
    out.push_str("        }\n");
    out.push_str(&format!(
        "        const handle = bridge.findByContract(0x{:08X}, 0x{:08X}, {});\n",
        contract_id_lo, contract_id_hi, min_version
    ));
    // Only null/undefined mean "not found": the loader's pack_handle already
    // returns null for the null handle, and slot 0 generation 0 legitimately
    // packs to 0 — testing `=== 0` would falsely reject a valid handle.
    out.push_str("        if (handle === null || handle === undefined) {\n");
    out.push_str("            return null;\n");
    out.push_str("        }\n");
    out.push_str(&format!(
        "        return new {}(bridge, hostPtr);\n",
        class_name
    ));
    out.push_str("    }\n\n");

    for func in &contract.functions {
        generate_ts_peer_caller_method(out, func, contract_id_lo, contract_id_hi, min_version, ir)?;
    }

    out.push_str("}\n\n");
    Ok(())
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
    ir: &ValidatedIr,
) -> Result<(), PolyplugcError> {
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

    out.push_str("        const polyplug = this._bridge;\n");
    out.push_str("        if (!polyplug || !polyplug.callGuestMethod) {\n");
    if has_return {
        out.push_str("            return null as any;\n");
    } else {
        out.push_str("            return;\n");
    }
    out.push_str("        }\n");
    emit_ts_caller_alloc_shim(out);
    out.push_str("        try {\n");

    // Marshal args and out buffer using the same helpers as host-contract methods.
    emit_ts_guest_host_contract_args_setup(out, func, ir)?;
    emit_ts_guest_host_contract_out_setup(out, &func.returns, ir)?;

    // Call the bridge primitive.  It returns the u32 error code directly.
    out.push_str(&format!(
        "        const errCode: number = polyplug.callGuestMethod(0x{:08X}, 0x{:08X}, {}, {}, argsPtr, outPtr);\n",
        contract_id_lo, contract_id_hi, min_version, fn_id
    ));
    // Throw with the ABI error code (JS host SDK convention) instead of
    // silently returning null and discarding the code.
    out.push_str("        if (errCode !== 0) {\n");
    out.push_str(&format!(
        "            throw new Error(`peer call {} failed (code ${{errCode}})`);\n",
        func.name
    ));
    out.push_str("        }\n");

    if has_return {
        emit_ts_guest_host_contract_readback(out, func.returns.as_ref(), ir)?;
        out.push_str("        return result;\n");
    }

    emit_ts_caller_free_shim(out);

    out.push_str("    }\n\n");
    Ok(())
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
        generate_ts_guest_host_contract_caller(&mut out, &contract, &wrapper_ir(vec![], vec![]))
            .expect("generate host-contract caller");
        assert!(
            out.contains("export class HostLoggerContract"),
            "missing class: {out}"
        );
        assert!(
            out.contains(
                "private constructor(bridge: any, hostPtr: { lo: number; hi: number }, minVersion: number)"
            ),
            "missing private constructor: {out}"
        );
        assert!(
            out.contains(
                "static fromHost(bridge: any, hostPtr: { lo: number; hi: number }, minVersion: number = 0)"
            ),
            "missing fromHost: {out}"
        );
        assert!(out.contains("isValid(): boolean"), "missing isValid: {out}");
        // The bridge is threaded in, not read from a global (Rule 12).
        assert!(
            !out.contains("(globalThis as any).polyplug"),
            "host-contract caller must not read a global bridge: {out}"
        );
        assert!(
            out.contains("const polyplug = this._bridge;"),
            "host-contract caller methods must use the threaded bridge: {out}"
        );
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
        generate_ts_guest_host_contract_caller(&mut out, &contract, &wrapper_ir(vec![], vec![]))
            .expect("generate host-contract caller");
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
        let out: String =
            generate_guest_host_contracts_ts(&ir).expect("generate guest host contracts");
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
        let out: String =
            generate_guest_host_contracts_ts(&ir).expect("generate guest host contracts");
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
        let out: String =
            generate_guest_host_contracts_ts(&ir).expect("generate guest host contracts");
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
                loader: "js-quickjs".to_owned(),
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
        // A StringView-returning host-contract method must use _callerAlloc(16) (host
        // alloc, freed by the method's finally) for the out buffer, emit
        // polyplug.callHostContract, and read back via readU32(outPtr+8).
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
        generate_ts_guest_host_contract_caller(&mut out, &contract, &wrapper_ir(vec![], vec![]))
            .expect("generate host-contract caller");
        assert!(
            out.contains("const _outBuf = _callerAlloc(16);"),
            "out buffer must be _callerAlloc(16) for StringView: {out}"
        );
        assert!(
            out.contains("polyplug.callHostContract("),
            "must use callHostContract: {out}"
        );
        assert!(
            out.contains("polyplug.readU32(outPtr + 8)"),
            "must read len from outPtr+8: {out}"
        );
        // Caller buffers are host-allocated and explicitly freed (no global arena).
        assert!(
            out.contains("for (const _f of _frees) { polyplug.free("),
            "caller must free its host-allocated buffers: {out}"
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
        generate_ts_guest_host_contract_caller(&mut out, &contract, &wrapper_ir(vec![], vec![]))
            .expect("generate host-contract caller");
        assert!(
            out.contains("const _outBuf = _callerAlloc(24);"),
            "out buffer must be _callerAlloc(24) for Buffer: {out}"
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

    // ─── Guest ABI wrapper marshalling (per-signature, C layout) ──────────────
    //
    // The wrappers were historically hardcoded for StringView→StringView; these
    // tests pin the per-signature marshalling for every other shape.

    fn wrapper_ir(types: Vec<crate::ir::ResolvedType>, enums: Vec<EnumDef>) -> ValidatedIr {
        ValidatedIr {
            types,
            enums,
            contracts: vec![],
            host_contracts: vec![],
            bundle: None,
        }
    }

    fn wrapper_contract(functions: Vec<ResolvedFunction>) -> ResolvedContract {
        ResolvedContract {
            name: "test.shapes".to_owned(),
            contract_id: polyplug_utils::guest_contract_id("test.shapes", 1),
            version: crate::ir::Version {
                major: 1,
                minor: 0,
                patch: 0,
            },
            functions,
        }
    }

    fn render_wrapper(contract: &ResolvedContract, ir: &ValidatedIr) -> String {
        let mut out: String = String::new();
        render_plugin_interface_quickjs(&mut out, "shapes_plugin", contract, ir)
            .expect("render_plugin_interface_quickjs");
        out
    }

    #[test]
    fn quickjs_guest_wrapper_scalar_pair_is_not_stringview_hardcoded() {
        let contract: ResolvedContract = wrapper_contract(vec![ResolvedFunction {
            name: "add_primitive".to_owned(),
            function_id: 0,
            params: vec![
                ResolvedParam {
                    name: "a".to_owned(),
                    ty: ResolvedTypeRef::Primitive(PrimitiveType::U32),
                },
                ResolvedParam {
                    name: "b".to_owned(),
                    ty: ResolvedTypeRef::Primitive(PrimitiveType::U32),
                },
            ],
            returns: Some(ResolvedTypeRef::Primitive(PrimitiveType::U32)),
        }]);
        let ir: ValidatedIr = wrapper_ir(vec![], vec![]);
        let out: String = render_wrapper(&contract, &ir);
        assert!(
            out.contains("var arg_a = polyplug.readU32(args_ptr);"),
            "first u32 param must be read at offset 0: {out}"
        );
        assert!(
            out.contains("var arg_b = polyplug.readU32(args_ptr + 4);"),
            "second u32 param must be read at offset 4: {out}"
        );
        assert!(
            out.contains("polyplug.writeU32(out_ptr, result);"),
            "u32 return must be written with writeU32: {out}"
        );
        assert!(
            !out.contains("input_ptr_lo") && !out.contains("result.ptr_lo"),
            "scalar signature must not use the StringView hardcode: {out}"
        );
    }

    #[test]
    fn quickjs_guest_wrapper_mixed_pack_uses_c_layout_alignment() {
        // (u32, StringView): the StringView sits at offset 8 in a repr(C)
        // pack (4 bytes of padding after the u32), NOT at offset 4.
        let contract: ResolvedContract = wrapper_contract(vec![ResolvedFunction {
            name: "log_with_count".to_owned(),
            function_id: 0,
            params: vec![
                ResolvedParam {
                    name: "count".to_owned(),
                    ty: ResolvedTypeRef::Primitive(PrimitiveType::U32),
                },
                ResolvedParam {
                    name: "message".to_owned(),
                    ty: ResolvedTypeRef::AbiType(AbiBuiltin::StringView),
                },
            ],
            returns: None,
        }]);
        let ir: ValidatedIr = wrapper_ir(vec![], vec![]);
        let out: String = render_wrapper(&contract, &ir);
        assert!(
            out.contains("var arg_message = { ptr_lo: polyplug.readU32(args_ptr + 8), ptr_hi: polyplug.readU32(args_ptr + 12), len: polyplug.readU32(args_ptr + 16) };"),
            "StringView after u32 must be read at C-layout offset 8: {out}"
        );
    }

    #[test]
    fn quickjs_guest_wrapper_stringview_return_arena_allocates() {
        let contract: ResolvedContract = wrapper_contract(vec![ResolvedFunction {
            name: "decode".to_owned(),
            function_id: 0,
            params: vec![ResolvedParam {
                name: "input".to_owned(),
                ty: ResolvedTypeRef::AbiType(AbiBuiltin::StringView),
            }],
            returns: Some(ResolvedTypeRef::AbiType(AbiBuiltin::StringView)),
        }]);
        let ir: ValidatedIr = wrapper_ir(vec![], vec![]);
        let out: String = render_wrapper(&contract, &ir);
        // The wrapper takes the threaded arena_ptr + bridge and reads `polyplug`
        // from the bridge param (no global — Rule 12).
        assert!(
            out.contains(
                "(impl: any, args_ptr: number, out_ptr: number, arena_ptr: number, bridge: any): number"
            ),
            "wrapper must take the threaded arena_ptr + bridge: {out}"
        );
        assert!(
            out.contains("const polyplug = bridge;"),
            "wrapper must use the threaded bridge, not a global: {out}"
        );
        assert!(
            out.contains("var arg_input = { ptr_lo: polyplug.readU32(args_ptr), ptr_hi: polyplug.readU32(args_ptr + 4), len: polyplug.readU32(args_ptr + 8) };"),
            "single StringView param read at offset 0: {out}"
        );
        // The author returns a plain string; the wrapper arena-allocates it via the
        // threaded arena_ptr and writes the StringView back through out_ptr.
        assert!(
            out.contains("const _retBytes = _ppEncodeUtf8(result);")
                && out.contains(
                    "const _retBuf = polyplug.arenaAlloc(_retBytes.length > 0 ? _retBytes.length : 1, arena_ptr);"
                ),
            "StringView return must arena-allocate a plain string via the threaded arena: {out}"
        );
        assert!(
            out.contains("polyplug.writeU32(out_ptr, _retBuf[0]);")
                && out.contains("polyplug.writeU32(out_ptr + 8, _retBytes.length);"),
            "StringView return written back through out_ptr: {out}"
        );
        assert!(
            out.contains("polyplug.writeU32(out_ptr + 12, 0);"),
            "high half of the usize len must be zeroed: {out}"
        );
    }

    #[test]
    fn quickjs_guest_wrapper_struct_param_reads_fields() {
        let types: Vec<crate::ir::ResolvedType> = vec![crate::ir::ResolvedType {
            name: "AddArgs".to_owned(),
            fields: vec![
                crate::ir::ResolvedField {
                    name: "a".to_owned(),
                    ty: ResolvedTypeRef::Primitive(PrimitiveType::U32),
                },
                crate::ir::ResolvedField {
                    name: "b".to_owned(),
                    ty: ResolvedTypeRef::Primitive(PrimitiveType::U32),
                },
            ],
        }];
        let contract: ResolvedContract = wrapper_contract(vec![ResolvedFunction {
            name: "add".to_owned(),
            function_id: 0,
            params: vec![ResolvedParam {
                name: "args".to_owned(),
                ty: ResolvedTypeRef::UserDefined("AddArgs".to_owned()),
            }],
            returns: Some(ResolvedTypeRef::Primitive(PrimitiveType::U32)),
        }]);
        let ir: ValidatedIr = wrapper_ir(types, vec![]);
        let out: String = render_wrapper(&contract, &ir);
        assert!(
            out.contains("var arg_args = { a: polyplug.readU32(args_ptr), b: polyplug.readU32(args_ptr + 4) };"),
            "struct param must be read field-by-field at C offsets: {out}"
        );
    }

    #[test]
    fn quickjs_guest_wrapper_nested_struct_param_and_return_recurse() {
        // A struct field that is itself a struct (depth > 1) must marshal through
        // the QuickJS guest wrapper, not be rejected. Layout:
        //   Inner { a: u32, b: u32 }            -> a@0, b@4 ; size 8, align 4
        //   Boxed { tag: u64, inner: Inner }    -> tag@0, inner@8 (8-aligned past
        //                                          the u64) ; size 16, align 8
        // proving both depth-2 recursion AND that the nested struct sits at the
        // C-layout offset its alignment demands.
        let types: Vec<crate::ir::ResolvedType> = vec![
            crate::ir::ResolvedType {
                name: "Inner".to_owned(),
                fields: vec![
                    crate::ir::ResolvedField {
                        name: "a".to_owned(),
                        ty: ResolvedTypeRef::Primitive(PrimitiveType::U32),
                    },
                    crate::ir::ResolvedField {
                        name: "b".to_owned(),
                        ty: ResolvedTypeRef::Primitive(PrimitiveType::U32),
                    },
                ],
            },
            crate::ir::ResolvedType {
                name: "Boxed".to_owned(),
                fields: vec![
                    crate::ir::ResolvedField {
                        name: "tag".to_owned(),
                        ty: ResolvedTypeRef::Primitive(PrimitiveType::U64),
                    },
                    crate::ir::ResolvedField {
                        name: "inner".to_owned(),
                        ty: ResolvedTypeRef::UserDefined("Inner".to_owned()),
                    },
                ],
            },
        ];
        let contract: ResolvedContract = wrapper_contract(vec![ResolvedFunction {
            name: "roundtrip".to_owned(),
            function_id: 0,
            params: vec![ResolvedParam {
                name: "o".to_owned(),
                ty: ResolvedTypeRef::UserDefined("Boxed".to_owned()),
            }],
            returns: Some(ResolvedTypeRef::UserDefined("Boxed".to_owned())),
        }]);
        let ir: ValidatedIr = wrapper_ir(types, vec![]);
        let out: String = render_wrapper(&contract, &ir);

        // The depth guard is gone: no UnsupportedType / "nested struct" rejection.
        assert!(
            !out.contains("nested struct") && !out.contains("UnsupportedType"),
            "nested struct must not be rejected: {out}"
        );
        // READ (js_read_expr) of the nested param descends field-by-field, with the
        // inner struct read at the 8-aligned offset (8/12), not 4.
        assert!(
            out.contains("var arg_o = { tag: { lo: polyplug.readU32(args_ptr), hi: polyplug.readU32(args_ptr + 4) }, inner: { a: polyplug.readU32(args_ptr + 8), b: polyplug.readU32(args_ptr + 12) } };"),
            "nested struct param must read as a nested object literal at C offsets: {out}"
        );
        // WRITE (emit_js_write_value) of the nested return descends into the inner
        // struct's fields at the same 8-aligned offsets.
        assert!(
            out.contains("polyplug.writeU32(out_ptr + 8, result.inner.a);"),
            "nested return inner.a must write at offset 8: {out}"
        );
        assert!(
            out.contains("polyplug.writeU32(out_ptr + 12, result.inner.b);"),
            "nested return inner.b must write at offset 12: {out}"
        );
    }

    #[test]
    fn quickjs_guest_wrapper_f64_uses_float_bridge() {
        let contract: ResolvedContract = wrapper_contract(vec![ResolvedFunction {
            name: "scale".to_owned(),
            function_id: 0,
            params: vec![ResolvedParam {
                name: "factor".to_owned(),
                ty: ResolvedTypeRef::Primitive(PrimitiveType::F64),
            }],
            returns: Some(ResolvedTypeRef::Primitive(PrimitiveType::F64)),
        }]);
        let ir: ValidatedIr = wrapper_ir(vec![], vec![]);
        let out: String = render_wrapper(&contract, &ir);
        assert!(
            out.contains("var arg_factor = polyplug.readF64(args_ptr);"),
            "f64 param must use readF64: {out}"
        );
        assert!(
            out.contains("polyplug.writeF64(out_ptr, result);"),
            "f64 return must use writeF64: {out}"
        );
    }

    #[test]
    fn quickjs_guest_wrapper_enum_param_reads_repr_integer() {
        let enums: Vec<EnumDef> = vec![EnumDef {
            name: "LogLevel".to_owned(),
            repr: ReprType::U32,
            bitflag: false,
            variants: vec![EnumVariant {
                name: "Info".to_owned(),
                value: "1".to_owned(),
            }],
        }];
        let contract: ResolvedContract = wrapper_contract(vec![ResolvedFunction {
            name: "set_level".to_owned(),
            function_id: 0,
            params: vec![ResolvedParam {
                name: "level".to_owned(),
                ty: ResolvedTypeRef::UserDefined("LogLevel".to_owned()),
            }],
            returns: None,
        }]);
        let ir: ValidatedIr = wrapper_ir(vec![], enums);
        let out: String = render_wrapper(&contract, &ir);
        assert!(
            out.contains("var arg_level = polyplug.readU32(args_ptr);"),
            "u32-repr enum param must be read as the raw repr integer: {out}"
        );
    }

    #[test]
    fn quickjs_guest_wrapper_void_void_emits_no_marshalling() {
        let contract: ResolvedContract = wrapper_contract(vec![ResolvedFunction {
            name: "reset".to_owned(),
            function_id: 0,
            params: vec![],
            returns: None,
        }]);
        let ir: ValidatedIr = wrapper_ir(vec![], vec![]);
        let out: String = render_wrapper(&contract, &ir);
        assert!(
            out.contains("impl.fn0();"),
            "void/void function must call the impl with no args: {out}"
        );
        assert!(
            !out.contains("readU32(args_ptr") && !out.contains("writeU32(out_ptr"),
            "void/void function must not touch args/out buffers: {out}"
        );
    }

    #[test]
    fn quickjs_caller_pack_uses_c_layout_alignment() {
        // Guest→host caller side of the same convention: (LogLevel u32,
        // StringView) packs to 24 bytes with the view at offset 8.
        let enums: Vec<EnumDef> = vec![EnumDef {
            name: "LogLevel".to_owned(),
            repr: ReprType::U32,
            bitflag: false,
            variants: vec![EnumVariant {
                name: "Info".to_owned(),
                value: "1".to_owned(),
            }],
        }];
        let func: ResolvedFunction = ResolvedFunction {
            name: "log_with_level".to_owned(),
            function_id: 1,
            params: vec![
                ResolvedParam {
                    name: "level".to_owned(),
                    ty: ResolvedTypeRef::UserDefined("LogLevel".to_owned()),
                },
                ResolvedParam {
                    name: "message".to_owned(),
                    ty: ResolvedTypeRef::AbiType(AbiBuiltin::StringView),
                },
            ],
            returns: None,
        };
        let mut out: String = String::new();
        emit_ts_guest_host_contract_args_setup(&mut out, &func, &wrapper_ir(vec![], enums))
            .expect("args setup");
        assert!(
            out.contains("_callerAlloc(24)"),
            "pack must be 24 bytes (u32 + pad + 16-byte StringView): {out}"
        );
        assert!(
            out.contains("polyplug.writeU32(argsPtr + 8, _messageDataBuf[0]);"),
            "StringView after u32 must be written at C-layout offset 8: {out}"
        );
    }

    // ─── Caller-side enum marshalling (repr-integer slots) ──────────────────────

    fn pixel_format_enums() -> Vec<EnumDef> {
        vec![EnumDef {
            name: "PixelFormat".to_owned(),
            repr: ReprType::U32,
            bitflag: false,
            variants: vec![EnumVariant {
                name: "Rgba8".to_owned(),
                value: "1".to_owned(),
            }],
        }]
    }

    #[test]
    fn quickjs_caller_single_enum_param_writes_repr_integer() {
        let func: ResolvedFunction = ResolvedFunction {
            name: "set_format".to_owned(),
            function_id: 0,
            params: vec![ResolvedParam {
                name: "fmt".to_owned(),
                ty: ResolvedTypeRef::UserDefined("PixelFormat".to_owned()),
            }],
            returns: None,
        };
        let mut out: String = String::new();
        emit_ts_guest_host_contract_args_setup(
            &mut out,
            &func,
            &wrapper_ir(vec![], pixel_format_enums()),
        )
        .expect("args setup");
        assert!(
            out.contains("polyplug.writeU32(argsPtr, Number(fmt));"),
            "u32-repr enum param must be written into the arena slot: {out}"
        );
        assert!(
            out.contains("const argsPtr = _argsBuf[0] + _argsBuf[1] * 4294967296;"),
            "the slot's ADDRESS must be passed as argsPtr: {out}"
        );
    }

    #[test]
    fn quickjs_caller_enum_return_reads_repr_integer() {
        let returns: Option<ResolvedTypeRef> =
            Some(ResolvedTypeRef::UserDefined("PixelFormat".to_owned()));
        let mut out: String = String::new();
        emit_ts_guest_host_contract_readback(
            &mut out,
            returns.as_ref(),
            &wrapper_ir(vec![], pixel_format_enums()),
        )
        .expect("readback");
        assert!(
            out.contains("const result = polyplug.readU32(outPtr) as any;"),
            "u32-repr enum return must read back the repr integer: {out}"
        );
        assert!(
            !out.contains("{ lo:"),
            "enum return must NOT fall back to the {{lo, hi}} object shape: {out}"
        );
    }

    #[test]
    fn quickjs_caller_u64_enum_splits_words() {
        let enums: Vec<EnumDef> = vec![EnumDef {
            name: "BigFlags".to_owned(),
            repr: ReprType::U64,
            bitflag: false,
            variants: vec![EnumVariant {
                name: "A".to_owned(),
                value: "1".to_owned(),
            }],
        }];
        let func: ResolvedFunction = ResolvedFunction {
            name: "set_flags".to_owned(),
            function_id: 0,
            params: vec![ResolvedParam {
                name: "flags".to_owned(),
                ty: ResolvedTypeRef::UserDefined("BigFlags".to_owned()),
            }],
            returns: None,
        };
        let ir: ValidatedIr = wrapper_ir(vec![], enums);
        let mut out: String = String::new();
        emit_ts_guest_host_contract_args_setup(&mut out, &func, &ir).expect("args setup");
        assert!(
            out.contains("polyplug.writeU32(argsPtr, Number(flags) >>> 0);")
                && out.contains(
                    "polyplug.writeU32(argsPtr + 4, Math.floor(Number(flags) / 4294967296));"
                ),
            "u64-repr enum param must split into lo/hi words (writeU32 alone truncates): {out}"
        );

        let returns: Option<ResolvedTypeRef> =
            Some(ResolvedTypeRef::UserDefined("BigFlags".to_owned()));
        let mut ret: String = String::new();
        emit_ts_guest_host_contract_readback(&mut ret, returns.as_ref(), &ir).expect("readback");
        assert!(
            ret.contains(
                "(polyplug.readU32(outPtr) + polyplug.readU32(outPtr + 4) * 4294967296) as any"
            ),
            "u64-repr enum return must combine lo/hi words: {ret}"
        );
    }

    #[test]
    fn quickjs_caller_struct_param_and_return_marshal_field_by_field() {
        // Struct-by-value param/return on the guest→host caller path must marshal
        // field-by-field (NOT Number(structObj) = NaN into a 4-byte slot).
        // Pair { a: u32, b: u32 } -> a@0, b@4 ; size 8, align 4.
        let types: Vec<crate::ir::ResolvedType> = vec![crate::ir::ResolvedType {
            name: "Pair".to_owned(),
            fields: vec![
                crate::ir::ResolvedField {
                    name: "a".to_owned(),
                    ty: ResolvedTypeRef::Primitive(PrimitiveType::U32),
                },
                crate::ir::ResolvedField {
                    name: "b".to_owned(),
                    ty: ResolvedTypeRef::Primitive(PrimitiveType::U32),
                },
            ],
        }];
        let ir: ValidatedIr = wrapper_ir(types, vec![]);
        let func: ResolvedFunction = ResolvedFunction {
            name: "compute".to_owned(),
            function_id: 0,
            params: vec![ResolvedParam {
                name: "args".to_owned(),
                ty: ResolvedTypeRef::UserDefined("Pair".to_owned()),
            }],
            returns: Some(ResolvedTypeRef::UserDefined("Pair".to_owned())),
        };

        // PARAM: allocate the struct's C size and pack each field — no NaN.
        let mut args: String = String::new();
        emit_ts_guest_host_contract_args_setup(&mut args, &func, &ir).expect("args setup");
        assert!(
            args.contains("_callerAlloc(8)"),
            "struct param slot must be the C-layout size (8), not a 4-byte slot: {args}"
        );
        assert!(
            args.contains("polyplug.writeU32(argsPtr, args.a);")
                && args.contains("polyplug.writeU32(argsPtr + 4, args.b);"),
            "struct param must be packed field-by-field at C offsets: {args}"
        );
        assert!(
            !args.contains("Number(args)"),
            "struct param must NOT degrade to Number(structObj) = NaN: {args}"
        );

        // RETURN out-setup: allocate the exact struct size (not an 8-byte pointer slot).
        let mut outs: String = String::new();
        emit_ts_guest_host_contract_out_setup(&mut outs, &func.returns, &ir).expect("out setup");
        assert!(
            outs.contains("_callerAlloc(8)"),
            "struct return out slot must be the C-layout size: {outs}"
        );

        // RETURN readback: read the struct field-by-field into an object literal.
        let mut rb: String = String::new();
        emit_ts_guest_host_contract_readback(&mut rb, func.returns.as_ref(), &ir)
            .expect("readback");
        assert!(
            rb.contains(
                "const result = { a: polyplug.readU32(outPtr), b: polyplug.readU32(outPtr + 4) };"
            ),
            "struct return must be read field-by-field, not as a {{lo,hi}} pointer: {rb}"
        );
    }

    #[test]
    fn quickjs_caller_struct_with_stringview_field_arena_allocates() {
        // A struct param whose field is a StringView must arena-allocate the
        // string bytes (matching the Deno caller) — no per-language limitation.
        // Holder { name: StringView, code: u32 } -> name@0 (16,8), code@16 ; size 24.
        let types: Vec<crate::ir::ResolvedType> = vec![crate::ir::ResolvedType {
            name: "Holder".to_owned(),
            fields: vec![
                crate::ir::ResolvedField {
                    name: "name".to_owned(),
                    ty: ResolvedTypeRef::AbiType(AbiBuiltin::StringView),
                },
                crate::ir::ResolvedField {
                    name: "code".to_owned(),
                    ty: ResolvedTypeRef::Primitive(PrimitiveType::U32),
                },
            ],
        }];
        let ir: ValidatedIr = wrapper_ir(types, vec![]);
        let func: ResolvedFunction = ResolvedFunction {
            name: "store".to_owned(),
            function_id: 0,
            params: vec![ResolvedParam {
                name: "args".to_owned(),
                ty: ResolvedTypeRef::UserDefined("Holder".to_owned()),
            }],
            returns: None,
        };
        let mut out: String = String::new();
        emit_ts_guest_host_contract_args_setup(&mut out, &func, &ir).expect("args setup");
        assert!(
            out.contains("_callerAlloc(24)"),
            "Holder slot must be 24 bytes (16-byte StringView + u32, 8-aligned): {out}"
        );
        assert!(
            out.contains("const _sv0Bytes = _ppEncodeUtf8(args.name);"),
            "StringView struct field must encode from the field expression (TextEncoder-free): {out}"
        );
        assert!(
            out.contains("polyplug.writeU32(argsPtr + 16, args.code);"),
            "scalar field after the 16-byte StringView must write at offset 16: {out}"
        );
    }

    // ─── Deno host caller — full-shape marshalling ──────────────────────────────

    /// Build an IR exercising every Deno host-caller marshalling shape: a
    /// multi-param function (u32 + u64 + enum + StringView in, StringView out),
    /// a struct parameter, a struct return, and an enum return.
    fn deno_shapes_ir() -> ValidatedIr {
        ValidatedIr {
            types: vec![ResolvedType {
                name: "Pair".to_owned(),
                fields: vec![
                    ResolvedField {
                        name: "a".to_owned(),
                        ty: ResolvedTypeRef::Primitive(PrimitiveType::U32),
                    },
                    ResolvedField {
                        name: "b".to_owned(),
                        ty: ResolvedTypeRef::Primitive(PrimitiveType::I64),
                    },
                ],
            }],
            enums: vec![EnumDef {
                name: "Color".to_owned(),
                repr: ReprType::U32,
                bitflag: false,
                variants: vec![EnumVariant {
                    name: "Red".to_owned(),
                    value: "1".to_owned(),
                }],
            }],
            contracts: vec![ResolvedContract {
                name: "test.shapes".to_owned(),
                contract_id: 0x0011_2233_4455_6677_u64,
                version: Version {
                    major: 1,
                    minor: 0,
                    patch: 0,
                },
                functions: vec![
                    ResolvedFunction {
                        name: "mix".to_owned(),
                        function_id: 0,
                        params: vec![
                            ResolvedParam {
                                name: "n".to_owned(),
                                ty: ResolvedTypeRef::Primitive(PrimitiveType::U32),
                            },
                            ResolvedParam {
                                name: "big".to_owned(),
                                ty: ResolvedTypeRef::Primitive(PrimitiveType::U64),
                            },
                            ResolvedParam {
                                name: "c".to_owned(),
                                ty: ResolvedTypeRef::UserDefined("Color".to_owned()),
                            },
                            ResolvedParam {
                                name: "s".to_owned(),
                                ty: ResolvedTypeRef::AbiType(AbiBuiltin::StringView),
                            },
                        ],
                        returns: Some(ResolvedTypeRef::AbiType(AbiBuiltin::StringView)),
                    },
                    ResolvedFunction {
                        name: "take_struct".to_owned(),
                        function_id: 1,
                        params: vec![ResolvedParam {
                            name: "p".to_owned(),
                            ty: ResolvedTypeRef::UserDefined("Pair".to_owned()),
                        }],
                        returns: Some(ResolvedTypeRef::Primitive(PrimitiveType::U32)),
                    },
                    ResolvedFunction {
                        name: "get_struct".to_owned(),
                        function_id: 2,
                        params: vec![],
                        returns: Some(ResolvedTypeRef::UserDefined("Pair".to_owned())),
                    },
                    ResolvedFunction {
                        name: "get_color".to_owned(),
                        function_id: 3,
                        params: vec![],
                        returns: Some(ResolvedTypeRef::UserDefined("Color".to_owned())),
                    },
                ],
            }],
            host_contracts: vec![],
            bundle: None,
        }
    }

    #[test]
    fn deno_host_caller_marshals_all_shapes() {
        let ir: ValidatedIr = deno_shapes_ir();
        let out: String = generate_callers_ts(&ir).expect("generate Deno callers");

        // The old "shape not supported" stub must be gone for every shape.
        assert!(
            !out.contains("shape not supported"),
            "no caller shape may emit the unsupported stub: {out}"
        );

        // Ergonomic, import-free TS signatures.
        assert!(
            out.contains("mix(n: number, big: bigint, c: number, s: string): string {"),
            "mix signature must map each param to its ergonomic Deno type: {out}"
        );
        assert!(
            out.contains("take_struct(p: { a: number; b: bigint }): number {"),
            "struct param must be an inline object type: {out}"
        );
        assert!(
            out.contains("get_struct(): { a: number; b: bigint } {"),
            "struct return must be an inline object type: {out}"
        );
        assert!(
            out.contains("get_color(): number {"),
            "enum return must be `number`: {out}"
        );

        // `mix` args struct: u32@0, u64@8 (8-aligned), Color u32@16, StringView@24
        // → 40-byte buffer.
        assert!(
            out.contains("const argsBuf = new Uint8Array(40);"),
            "mix args must pack to 40 bytes under C layout: {out}"
        );
        assert!(
            out.contains("argsDv.setBigUint64(8, BigInt(big), true);"),
            "u64 param must be written at offset 8 as bigint: {out}"
        );
        // Enum param written UNSIGNED at its repr width (the #34 item-3 fix).
        assert!(
            out.contains("argsDv.setUint32(16, Number(c) >>> 0, true);"),
            "u32-repr enum param must be written UNSIGNED at offset 16: {out}"
        );
        // StringView param: host-allocated, length+ptr written, tracked for free.
        assert!(
            out.contains("rt.alloc(_sv0Alloc, 1)")
                && out.contains("_allocs.push([_sv0Ptr, _sv0Alloc]);"),
            "StringView param must be host-allocated and tracked for release: {out}"
        );
        assert!(
            out.contains(
                "argsDv.setBigUint64(24, BigInt(Deno.UnsafePointer.value(_sv0Ptr)), true);"
            ),
            "StringView param ptr must be written at C-layout offset 24: {out}"
        );
        // All argument payloads are freed after dispatch.
        assert!(
            out.contains("for (const [_p, _s] of _allocs) { rt.free(_p, _s, 1); }"),
            "argument payloads must be freed after dispatch: {out}"
        );
        // StringView return: decoded then freed.
        assert!(
            out.contains("_decoder.decode(") && out.contains("rt.free(_ptr, _l, 1);"),
            "returned StringView must be decoded and freed: {out}"
        );

        // Struct return read field-by-field, UNSIGNED/signed at exact widths.
        assert!(
            out.contains("const _r1 = outDv.getUint32(0, true);"),
            "struct return u32 field must read at offset 0: {out}"
        );
        assert!(
            out.contains("const _r2 = outDv.getBigInt64(8, true);"),
            "struct return i64 field must read as signed bigint at offset 8: {out}"
        );
        assert!(
            out.contains("const _r0 = { a: _r1, b: _r2 };"),
            "struct return must be assembled into an object literal: {out}"
        );

        // Enum return read UNSIGNED at repr width — never the {lo,hi} object.
        assert!(
            out.contains("const _r0 = outDv.getUint32(0, true);"),
            "u32-repr enum return must read the UNSIGNED repr integer: {out}"
        );
        assert!(
            !out.contains("getBigInt64") || !out.contains("Number(outDv.getBigInt64"),
            "enum returns must not sign-extend: {out}"
        );
    }

    #[test]
    fn deno_host_caller_marshals_nested_struct() {
        // A struct field that is itself a struct (depth > 1) must marshal through
        // the Deno host caller, not be rejected. Same layout as the QuickJS guest
        // nested test:
        //   Inner { a: u32, b: u32 }          -> a@0, b@4 ; size 8, align 4
        //   Boxed { tag: u64, inner: Inner }  -> tag@0, inner@8 ; size 16, align 8
        let ir: ValidatedIr = ValidatedIr {
            types: vec![
                ResolvedType {
                    name: "Inner".to_owned(),
                    fields: vec![
                        ResolvedField {
                            name: "a".to_owned(),
                            ty: ResolvedTypeRef::Primitive(PrimitiveType::U32),
                        },
                        ResolvedField {
                            name: "b".to_owned(),
                            ty: ResolvedTypeRef::Primitive(PrimitiveType::U32),
                        },
                    ],
                },
                ResolvedType {
                    name: "Boxed".to_owned(),
                    fields: vec![
                        ResolvedField {
                            name: "tag".to_owned(),
                            ty: ResolvedTypeRef::Primitive(PrimitiveType::U64),
                        },
                        ResolvedField {
                            name: "inner".to_owned(),
                            ty: ResolvedTypeRef::UserDefined("Inner".to_owned()),
                        },
                    ],
                },
            ],
            enums: vec![],
            contracts: vec![ResolvedContract {
                name: "test.nested".to_owned(),
                contract_id: 0x0011_2233_4455_6677_u64,
                version: Version {
                    major: 1,
                    minor: 0,
                    patch: 0,
                },
                functions: vec![
                    ResolvedFunction {
                        name: "take_box".to_owned(),
                        function_id: 0,
                        params: vec![ResolvedParam {
                            name: "o".to_owned(),
                            ty: ResolvedTypeRef::UserDefined("Boxed".to_owned()),
                        }],
                        returns: Some(ResolvedTypeRef::Primitive(PrimitiveType::U32)),
                    },
                    ResolvedFunction {
                        name: "get_box".to_owned(),
                        function_id: 1,
                        params: vec![],
                        returns: Some(ResolvedTypeRef::UserDefined("Boxed".to_owned())),
                    },
                ],
            }],
            host_contracts: vec![],
            bundle: None,
        };
        let out: String = generate_callers_ts(&ir).expect("generate Deno callers");

        // The depth guard is gone: nothing rejects the nested struct.
        assert!(
            !out.contains("shape not supported")
                && !out.contains("UnsupportedType")
                && !out.contains("nested struct"),
            "nested struct must not be rejected by the Deno caller: {out}"
        );
        // Nested inline TS types (deno_caller_ts_type recurses).
        assert!(
            out.contains("take_box(o: { tag: bigint; inner: { a: number; b: number } }): number {"),
            "nested struct param must be a nested inline object type: {out}"
        );
        assert!(
            out.contains("get_box(): { tag: bigint; inner: { a: number; b: number } } {"),
            "nested struct return must be a nested inline object type: {out}"
        );
        // WRITE (emit_deno_write_value) descends into the inner struct at the
        // 8-aligned C offsets (8/12), past the u64 tag at 0.
        assert!(
            out.contains("argsDv.setBigUint64(0, BigInt(o.tag), true);"),
            "u64 tag must be written at offset 0: {out}"
        );
        assert!(
            out.contains("argsDv.setUint32(8, Number(o.inner.a) >>> 0, true);"),
            "nested inner.a must be written at offset 8: {out}"
        );
        assert!(
            out.contains("argsDv.setUint32(12, Number(o.inner.b) >>> 0, true);"),
            "nested inner.b must be written at offset 12: {out}"
        );
        // READ (emit_deno_read_local) assembles the nested return object from the
        // inner fields read at offsets 8/12.
        assert!(
            out.contains("const _r2 = { a: _r3, b: _r4 };")
                && out.contains("const _r0 = { tag: _r1, inner: _r2 };"),
            "nested struct return must assemble a nested object literal: {out}"
        );
        assert!(
            out.contains("outDv.getUint32(8, true)") && out.contains("outDv.getUint32(12, true)"),
            "nested inner fields must be read at offsets 8/12: {out}"
        );
    }

    // ─── Deno host PROVIDER — full-shape marshalling ────────────────────────────

    /// Build an IR with a HOST contract exercising every provider marshalling
    /// shape: a multi-param method (u32 + u64 + enum + StringView in, StringView
    /// out), a struct parameter with a scalar return, a Buffer parameter, and a
    /// struct return. Proves the provider has no type-support limitation.
    fn deno_host_provider_shapes_ir() -> ValidatedIr {
        let base: ValidatedIr = deno_shapes_ir();
        ValidatedIr {
            types: base.types,
            enums: base.enums,
            contracts: vec![],
            host_contracts: vec![ResolvedHostContract {
                name: "host.shapes".to_owned(),
                contract_id: 0x0011_2233_4455_6677_u64,
                version: Version {
                    major: 1,
                    minor: 0,
                    patch: 0,
                },
                singleton: false,
                functions: vec![
                    ResolvedFunction {
                        name: "mix".to_owned(),
                        function_id: 0,
                        params: vec![
                            ResolvedParam {
                                name: "n".to_owned(),
                                ty: ResolvedTypeRef::Primitive(PrimitiveType::U32),
                            },
                            ResolvedParam {
                                name: "big".to_owned(),
                                ty: ResolvedTypeRef::Primitive(PrimitiveType::U64),
                            },
                            ResolvedParam {
                                name: "c".to_owned(),
                                ty: ResolvedTypeRef::UserDefined("Color".to_owned()),
                            },
                            ResolvedParam {
                                name: "s".to_owned(),
                                ty: ResolvedTypeRef::AbiType(AbiBuiltin::StringView),
                            },
                        ],
                        returns: Some(ResolvedTypeRef::AbiType(AbiBuiltin::StringView)),
                    },
                    ResolvedFunction {
                        name: "take_struct".to_owned(),
                        function_id: 1,
                        params: vec![ResolvedParam {
                            name: "p".to_owned(),
                            ty: ResolvedTypeRef::UserDefined("Pair".to_owned()),
                        }],
                        returns: Some(ResolvedTypeRef::Primitive(PrimitiveType::U32)),
                    },
                    ResolvedFunction {
                        name: "take_buffer".to_owned(),
                        function_id: 2,
                        params: vec![ResolvedParam {
                            name: "b".to_owned(),
                            ty: ResolvedTypeRef::AbiType(AbiBuiltin::Buffer),
                        }],
                        returns: None,
                    },
                    ResolvedFunction {
                        name: "get_struct".to_owned(),
                        function_id: 3,
                        params: vec![],
                        returns: Some(ResolvedTypeRef::UserDefined("Pair".to_owned())),
                    },
                ],
            }],
            bundle: None,
        }
    }

    #[test]
    fn deno_host_provider_marshals_all_shapes() {
        let ir: ValidatedIr = deno_host_provider_shapes_ir();
        let out: String =
            generate_js_host_interface_factories_ts(&ir).expect("generate Deno host provider");

        // Every shape generates — no type is rejected as unsupported.
        assert!(
            !out.contains("UnsupportedType") && !out.contains("unsupported"),
            "no provider shape may be unsupported: {out}"
        );

        // Multi-param method: u32@0, u64@8, Color(u32)@16, StringView@24 (ptr+len)
        // → 40-byte args buffer; all four reach the impl.
        assert!(
            out.contains("const _argsDv = new DataView(new Deno.UnsafePointerView(argsPtr!).getArrayBuffer(40));"),
            "mix args must read from a 40-byte C-layout buffer: {out}"
        );
        assert!(
            out.contains("_argsDv.getBigUint64(8, true)"),
            "u64 arg must be read at offset 8: {out}"
        );
        assert!(
            out.contains("_argsDv.getUint32(16, true)"),
            "u32-repr enum arg must be read UNSIGNED at offset 16: {out}"
        );
        assert!(
            out.contains("impl.Mix(_r0, _r1, _r2, _r3);"),
            "all four args must reach the impl: {out}"
        );

        // StringView RETURN: host-allocated via rt.alloc (the guest frees it later).
        assert!(
            out.contains("rt.alloc(") && out.contains("_outDv.setBigUint64(0,"),
            "StringView return must host-allocate its payload and write the slot: {out}"
        );

        // Struct param read into an object; struct return written field-by-field.
        assert!(
            out.contains("const _r0 = { a:") || out.contains("a: _r"),
            "struct param must be read into an object literal: {out}"
        );
        assert!(
            out.contains("impl.GetStruct()") && out.contains("_outDv.setUint32(0,"),
            "struct return must write its scalar field: {out}"
        );

        // Buffer param read into a Uint8Array copy.
        assert!(
            out.contains("new Uint8Array(0)") && out.contains("impl.TakeBuffer(_r0);"),
            "Buffer param must be read into a Uint8Array and passed to the impl: {out}"
        );

        // CRITICAL ownership: the provider BORROWS its args — it must never free a
        // caller-owned arg payload (that would be a use-after-free). The only host
        // allocation is for the StringView RETURN; there is no rt.free anywhere.
        assert!(
            !out.contains("rt.free"),
            "provider must NOT free caller-owned arg payloads: {out}"
        );
    }
}
