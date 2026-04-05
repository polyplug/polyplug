//! C++ code generator for polyplugc.
//!
//! Generates:
//! - Host-side: header-only C++ callers (RAII wrapper + vtable dispatch)
//! - Guest-side: extern "C" ABI wrappers + abstract base classes + vtable statics

use super::is_native_runtime;
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
use crate::ir::ResolvedFunction;
use crate::ir::ResolvedHostContract;
use crate::ir::ResolvedPlugin;
use crate::ir::ResolvedType;
use crate::ir::ResolvedTypeRef;
use crate::ir::ValidatedIr;
use polyplug_codegen::PolyplugcError;

/// The C++ code generator.
pub(crate) struct CppGenerator;

impl CodeGenerator for CppGenerator {
    fn language_name(&self) -> &'static str {
        "cpp"
    }

    fn generate_host(
        &self,
        ir: &ValidatedIr,
        files: &mut GeneratedFiles,
    ) -> Result<(), PolyplugcError> {
        // ── File 1: types.hpp ────────────────────────────────────────────────
        let types_hpp: String = generate_types_hpp(ir);
        files.files.push(GeneratedFile {
            path: std::path::PathBuf::from("host/types.hpp"),
            content: types_hpp,
            force_regenerate: false,
        });

        // ── File 2: host_callers.hpp ─────────────────────────────────────────
        let host_callers_hpp: String = generate_host_callers_hpp(ir)?;
        files.files.push(GeneratedFile {
            path: std::path::PathBuf::from("host/host_callers.hpp"),
            content: host_callers_hpp,
            force_regenerate: false,
        });

        // ── File 3: manifest.toml ────────────────────────────────────────────
        let manifest_toml: String = generate_manifest_toml();
        files.files.push(GeneratedFile {
            path: std::path::PathBuf::from("manifest.toml"),
            content: manifest_toml,
            force_regenerate: true,
        });

        // ── File 4: host_contracts.hpp ───────────────────────────────────────
        if !ir.host_contracts.is_empty() {
            let host_contracts_hpp: String = generate_cpp_host_contracts_file(ir);
            files.files.push(GeneratedFile {
                path: std::path::PathBuf::from("host/host_contracts.hpp"),
                content: host_contracts_hpp,
                force_regenerate: false,
            });
        }

        // ── File 5: interface_factories.hpp ─────────────────────────────────────
        if !ir.host_contracts.is_empty() {
            let interface_factories_hpp: String = generate_cpp_host_interface_factories_file(ir);
            files.files.push(GeneratedFile {
                path: std::path::PathBuf::from("host/interface_factories.hpp"),
                content: interface_factories_hpp,
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
        // ── File 1: types.hpp ────────────────────────────────────────────────
        let types_hpp: String = generate_types_hpp(ir);
        files.files.push(GeneratedFile {
            path: std::path::PathBuf::from("guest/types.hpp"),
            content: types_hpp,
            force_regenerate: false,
        });

        // ── File 2: contracts.hpp ────────────────────────────────────────────
        let contracts_hpp: String = generate_contracts_hpp(ir);
        files.files.push(GeneratedFile {
            path: std::path::PathBuf::from("guest/contracts.hpp"),
            content: contracts_hpp,
            force_regenerate: false,
        });

        // ── File 3: interfaces.hpp ──────────────────────────────────────────────
        let interfaces_hpp: String = generate_interfaces_hpp(ir)?;
        files.files.push(GeneratedFile {
            path: std::path::PathBuf::from("guest/interfaces.hpp"),
            content: interfaces_hpp,
            force_regenerate: false,
        });

        // ── File 4: init.hpp ─────────────────────────────────────────────────
        let init_hpp: String = generate_init_hpp(ir)?;
        files.files.push(GeneratedFile {
            path: std::path::PathBuf::from("guest/init.hpp"),
            content: init_hpp,
            force_regenerate: false,
        });

        // --api: manifest emitted by generate_host(); --bundle: emit full discovery manifest
        if ir.bundle.is_some() {
            let manifest_toml: String = generate_bundle_manifest_cpp(ir);
            files.files.push(GeneratedFile {
                path: std::path::PathBuf::from("manifest.toml"),
                content: manifest_toml,
                force_regenerate: true,
            });
        }
        // When ir.bundle.is_none() (--api mode): NO manifest emitted here.
        // The root manifest.toml was already emitted by generate_host().

        // ── File 5: host_contracts.hpp (guest-side callers) ─────────────────────
        if !ir.host_contracts.is_empty() {
            let host_contracts_hpp: String = generate_cpp_guest_host_contracts_file(ir);
            files.files.push(GeneratedFile {
                path: std::path::PathBuf::from("guest/host_contracts.hpp"),
                content: host_contracts_hpp,
                force_regenerate: false,
            });
        }

        Ok(())
    }
}

// ─── types.hpp generator ─────────────────────────────────────────────────────

fn generate_types_hpp(ir: &ValidatedIr) -> String {
    let mut out: String = String::new();
    out.push_str("// THIS FILE IS AUTO-GENERATED BY polyplugc. DO NOT EDIT.\n");
    out.push_str("// Re-generate with: polyplugc generate --api api.toml --lang cpp --out <dir>\n");
    out.push_str("#pragma once\n");
    out.push_str("#include <cstdint>\n");
    out.push_str("#include \"polyplug/abi.hpp\"\n\n");
    out.push_str("namespace polyplug_generated {\n\n");

    // Emit contract ID constants
    for contract in &ir.contracts {
        let contract_upper: String = contract.name.to_uppercase().replace(['.', '-'], "_");
        out.push_str(&format!(
            "constexpr uint64_t {}_CONTRACT_ID = 0x{:016X};\n",
            contract_upper, contract.contract_id
        ));
    }
    out.push('\n');

    // Emit enums before struct types
    for e in &ir.enums {
        generate_cpp_enum(&mut out, e);
    }

    for ty in &ir.types {
        generate_cpp_type(&mut out, ty);
    }

    out.push_str("}  // namespace polyplug_generated\n");
    out
}

// ─── contracts.hpp generator ─────────────────────────────────────────────────

fn generate_contracts_hpp(ir: &ValidatedIr) -> String {
    let mut out: String = String::new();
    out.push_str("// THIS FILE IS AUTO-GENERATED BY polyplugc. DO NOT EDIT.\n");
    out.push_str("// Re-generate with: polyplugc generate --api api.toml --lang cpp --out <dir>\n");
    out.push_str("#pragma once\n");
    out.push_str("#include \"types.hpp\"\n");
    out.push_str("#include <cstdint>\n\n");
    out.push_str("namespace polyplug_plugin {\n\nusing namespace polyplug_generated;\n\n");
    out.push_str("struct RuntimeError { uint32_t code; };\n\n");

    for contract in &ir.contracts {
        generate_cpp_guest_contract_class(&mut out, contract);
    }

    out.push_str("}  // namespace polyplug_plugin\n");
    out
}

fn generate_cpp_guest_contract_class(out: &mut String, contract: &ResolvedContract) {
    let class_name: String = contract_name_to_plugin_class(&contract.name);
    out.push_str(&format!(
        "/// Abstract plugin base for contract `{}` (id=0x{:016X})\n",
        contract.name, contract.contract_id
    ));
    out.push_str(&format!("class {} {{\npublic:\n", class_name));
    out.push_str(&format!("    virtual ~{}() = default;\n", class_name));

    for func in &contract.functions {
        generate_cpp_guest_abstract_method(out, func);
    }

    out.push_str("};\n\n");
}

fn generate_cpp_guest_abstract_method(out: &mut String, func: &ResolvedFunction) {
    let return_type: String = func
        .returns
        .as_ref()
        .map(cpp_type_name)
        .unwrap_or_else(|| "void".to_owned());

    let params: Vec<String> = func
        .params
        .iter()
        .map(|p| {
            let cpp_ty: String = cpp_type_name(&p.ty);
            match &p.ty {
                ResolvedTypeRef::UserDefined(_) => format!("const {}& {}", cpp_ty, p.name),
                ResolvedTypeRef::Primitive(_) | ResolvedTypeRef::AbiType(_) => {
                    format!("{} {}", cpp_ty, p.name)
                }
            }
        })
        .collect();
    let params_str: String = params.join(", ");

    out.push_str(&format!(
        "    virtual {} {}({}) = 0;\n",
        return_type, func.name, params_str
    ));
}

// ─── interfaces.hpp generator ───────────────────────────────────────────────────

fn generate_interfaces_hpp(ir: &ValidatedIr) -> Result<String, PolyplugcError> {
    let mut out: String = String::new();
    out.push_str("// THIS FILE IS AUTO-GENERATED BY polyplugc. DO NOT EDIT.\n");
    out.push_str("// Re-generate with: polyplugc generate --api api.toml --lang cpp --out <dir>\n");
    out.push_str("#pragma once\n");
    out.push_str("#include \"contracts.hpp\"\n");
    out.push_str("#include \"polyplug/abi.hpp\"\n");
    out.push_str("#include <cstdint>\n");
    out.push_str("#include <cstring>\n");
    out.push_str("#include <exception>\n\n");
    out.push_str("namespace polyplug_plugin {\n\nusing namespace polyplug_generated;\n\n");

    if let Some(bundle) = &ir.bundle {
        for plugin in &bundle.plugins {
            for contract_impl in &plugin.implements {
                if let Some(contract) = ir.contracts.iter().find(|c| {
                    let contract_full =
                        format!("{}@{}.{}", c.name, c.version.major, c.version.minor);
                    &contract_full == contract_impl
                }) {
                    generate_cpp_guest_plugin_interface(
                        &mut out,
                        &plugin.name,
                        contract,
                        is_native_runtime(&bundle.runtime),
                    )?;
                }
            }
        }
    } else {
        // When no bundle info, default to native dispatch
        for contract in &ir.contracts {
            generate_cpp_guest_contract_interface(&mut out, contract, true)?;
        }
    }

    out.push_str("}  // namespace polyplug_plugin\n");
    Ok(out)
}

fn generate_cpp_guest_plugin_interface(
    out: &mut String,
    plugin_name: &str,
    contract: &ResolvedContract,
    is_native: bool,
) -> Result<(), PolyplugcError> {
    let plugin_upper: String = plugin_name.to_uppercase().replace('.', "_");
    let plugin_lower: String = plugin_name.to_lowercase().replace('.', "_");
    let class_name: String = contract_name_to_plugin_class(&contract.name);
    let fn_count: usize = contract.functions.len();

    out.push_str(&format!("// Plugin: {}\n", plugin_name));
    out.push_str(&format!(
        "extern {}* g_{}_impl;\n\n",
        class_name, plugin_lower
    ));

    out.push_str(&format!(
        "constexpr uint64_t {}_CONTRACT_ID = 0x{:016X}ULL;\n\n",
        plugin_upper, contract.contract_id
    ));

    out.push_str(&format!(
        "inline void set_{}_impl({}* impl) {{ g_{}_impl = impl; }}\n\n",
        plugin_lower, class_name, plugin_lower
    ));

    // Forward declaration - user must implement this function
    out.push_str("// Forward declaration - user must implement this\n");
    out.push_str(&format!(
        "{}* create_{}_impl();\n\n",
        class_name, plugin_lower
    ));

    for func in &contract.functions {
        generate_cpp_guest_abi_wrapper(out, &plugin_lower, func)?;
    }

    out.push_str(&format!("static void* const {}_FNS[] = {{\n", plugin_upper));
    for func in &contract.functions {
        out.push_str(&format!(
            "    reinterpret_cast<void*>({0}_{1}_abi),\n",
            plugin_lower, func.name
        ));
    }
    out.push_str("};\n\n");

    // Instance lifecycle stubs
    out.push_str(&format!(
        "// Default create_instance stub for {} - returns null instance.\n",
        plugin_name
    ));
    out.push_str(&format!(
        "static GuestContractInstance {0}_create_instance_stub(void* rt_ctx, const void* args) noexcept {{\n",
        plugin_upper
    ));
    out.push_str("    (void)rt_ctx; (void)args;  // Unused in default stub.\n");
    out.push_str("    return GuestContractInstance{nullptr};  // Null instance for stateless plugins.\n");
    out.push_str("}\n\n");
    out.push_str(&format!(
        "// Default destroy_instance stub for {} - no-op.\n",
        plugin_name
    ));
    out.push_str(&format!(
        "static void {0}_destroy_instance_stub(void* rt_ctx, GuestContractInstance instance) noexcept {{\n",
        plugin_upper
    ));
    out.push_str("    (void)rt_ctx; (void)instance;  // Unused in default stub.\n");
    out.push_str("    // No-op - stateless plugins don't need cleanup.\n");
    out.push_str("}\n\n");

    out.push_str(&format!(
        "static GuestContractInterface {}_VTABLE = {{\n",
        plugin_upper
    ));
    out.push_str(&format!("    {}_CONTRACT_ID,\n", plugin_upper));
    out.push_str(&format!(
        "    Version{{ {}U, {}U, {}U }},  // contract_version\n",
        contract.version.major, contract.version.minor, contract.version.patch
    ));
    let dispatch_type_str: &str = if is_native {
        "DispatchType::Native"
    } else {
        "DispatchType::VirtualMachine"
    };
    out.push_str(&format!("    {},\n", dispatch_type_str));
    out.push_str(&format!("    {}_create_instance_stub,\n", plugin_upper));
    out.push_str(&format!("    {}_destroy_instance_stub,\n", plugin_upper));
    out.push_str(&format!(
        "    PluginDispatch{{ .native = NativeDispatch{{ {fn_count}U, {}_FNS }} }}\n",
        plugin_upper
    ));
    out.push_str("};\n\n");

    Ok(())
}

fn generate_cpp_guest_contract_interface(
    out: &mut String,
    contract: &ResolvedContract,
    is_native: bool,
) -> Result<(), PolyplugcError> {
    let lower: String = contract_name_to_lower_snake(&contract.name);
    let upper: String = contract_name_to_upper_snake(&contract.name);
    let class_name: String = contract_name_to_plugin_class(&contract.name);
    let fn_count: usize = contract.functions.len();

    // Forward declaration of impl pointer
    out.push_str("// Forward declaration -- set by polyplug_init\n");
    out.push_str(&format!("extern {}* g_{}_impl;\n\n", class_name, lower));

    // Contract ID constant
    out.push_str(&format!(
        "constexpr uint64_t {}_CONTRACT_ID = 0x{:016X}ULL;\n\n",
        upper, contract.contract_id
    ));

    // ABI wrapper functions — one per function
    for func in &contract.functions {
        generate_cpp_guest_abi_wrapper(out, &lower, func)?;
    }

    // Function pointer array
    out.push_str(&format!("static void* const {}_FNS[] = {{\n", upper));
    for func in &contract.functions {
        out.push_str(&format!(
            "    reinterpret_cast<void*>({0}_{1}_abi),\n",
            lower, func.name
        ));
    }
    out.push_str("};\n\n");

    // Instance lifecycle stubs
    out.push_str(&format!(
        "// Default create_instance stub for {} - returns null instance.\n",
        contract.name
    ));
    out.push_str(&format!(
        "static GuestContractInstance {0}_create_instance_stub(void* rt_ctx, const void* args) noexcept {{\n",
        upper
    ));
    out.push_str("    (void)rt_ctx; (void)args;  // Unused in default stub.\n");
    out.push_str("    return GuestContractInstance{nullptr};  // Null instance for stateless plugins.\n");
    out.push_str("}\n\n");
    out.push_str(&format!(
        "// Default destroy_instance stub for {} - no-op.\n",
        contract.name
    ));
    out.push_str(&format!(
        "static void {0}_destroy_instance_stub(void* rt_ctx, GuestContractInstance instance) noexcept {{\n",
        upper
    ));
    out.push_str("    (void)rt_ctx; (void)instance;  // Unused in default stub.\n");
    out.push_str("    // No-op - stateless plugins don't need cleanup.\n");
    out.push_str("}\n\n");

    // VTable static
    out.push_str(&format!("static GuestContractInterface {}_VTABLE = {{\n", upper));
    out.push_str(&format!("    {}_CONTRACT_ID,\n", upper));
    out.push_str(&format!(
        "    Version{{ {}U, {}U, {}U }},  // contract_version\n",
        contract.version.major, contract.version.minor, contract.version.patch
    ));
    let dispatch_type_str: &str = if is_native {
        "DispatchType::Native"
    } else {
        "DispatchType::VirtualMachine"
    };
    out.push_str(&format!("    {},\n", dispatch_type_str));
    out.push_str(&format!("    {}_create_instance_stub,\n", upper));
    out.push_str(&format!("    {}_destroy_instance_stub,\n", upper));
    out.push_str(&format!(
        "    PluginDispatch{{ .native = NativeDispatch{{ {}U, {}_FNS }} }}\n",
        fn_count, upper
    ));
    out.push_str("};\n\n");

    Ok(())
}

fn generate_cpp_guest_abi_wrapper(
    out: &mut String,
    contract_lower: &str,
    func: &ResolvedFunction,
) -> Result<(), PolyplugcError> {
    let fn_id: u32 = func.function_id;
    let is_void_return: bool = matches!(
        func.returns.as_ref(),
        None | Some(ResolvedTypeRef::AbiType(AbiBuiltin::Void))
    );
    let has_params: bool = !func.params.is_empty();

    out.push_str(&format!(
        "// ABI wrapper for {} (function_id = {})\n",
        func.name, fn_id
    ));
    out.push_str(&format!(
        "inline AbiError {0}_{1}_abi(GuestContractInstance instance, const void* args, void* out) noexcept {{\n",
        contract_lower, func.name
    ));
    out.push_str("    // Instance is ignored for stateless plugins (instance.data is nullptr).\n");
    out.push_str("    // For stateful plugins, users override create_instance and use instance.data.\n");
    out.push_str("    (void)instance;  // Suppress unused warning for stateless plugins.\n");
    out.push_str("    try {\n");

    if has_params {
        out.push_str("        if (args == nullptr) {\n");
        out.push_str("            return AbiError{8U, StringView{nullptr, 0}};  // ABI_ERROR_INVALID_POINTER\n");
        out.push_str("        }\n");
    }
    if !is_void_return {
        out.push_str("        if (out == nullptr) {\n");
        out.push_str("            return AbiError{8U, StringView{nullptr, 0}};  // ABI_ERROR_INVALID_POINTER\n");
        out.push_str("        }\n");
    }

    // Build the call expression
    let call_expr: String = build_guest_call_expr(contract_lower, func);
    out.push_str(&call_expr);

    if is_void_return {
        // For void, emit success return (call_expr already emits the call + newline)
        out.push_str("        // SAFETY: out pointer is not dereferenced for void return per ABI contract.\n");
        out.push_str("        (void)out;\n");
        out.push_str("        return AbiError{static_cast<uint32_t>(AbiErrorCode::Ok), StringView{nullptr, 0}};\n");
    } else {
        let ret_type: String = func
            .returns
            .as_ref()
            .map(cpp_type_name)
            .unwrap_or_else(|| "void".to_owned());
        out.push_str(&format!(
            "        // SAFETY: out is a valid void* pointing to a {ret_type} per ABI contract.\n"
        ));
        out.push_str("        // The host guarantees proper alignment and size before calling this wrapper.\n");
        out.push_str(&format!(
            "        *static_cast<{}*>(out) = result;\n",
            ret_type
        ));
        out.push_str("        return AbiError{static_cast<uint32_t>(AbiErrorCode::Ok), StringView{nullptr, 0}};\n");
    }

    out.push_str("    } catch (const std::exception& e) {\n");
    out.push_str("        // SAFETY: e.what() returns a valid null-terminated C string; reinterpret_cast preserves pointer validity.\n");
    out.push_str("        return AbiError{static_cast<uint32_t>(AbiErrorCode::Generic), StringView{reinterpret_cast<const uint8_t*>(e.what()), std::strlen(e.what())}};\n");
    out.push_str("    } catch (...) {\n");
    out.push_str(
        "        // SAFETY: panic_msg is a static constexpr string literal with known length 15.\n",
    );
    out.push_str("        static constexpr const char* panic_msg = \"plugin panicked\";\n");
    out.push_str("        return AbiError{static_cast<uint32_t>(AbiErrorCode::Panic), StringView{reinterpret_cast<const uint8_t*>(panic_msg), 15}};\n");
    out.push_str("    }\n");
    out.push_str("}\n\n");

    Ok(())
}

/// Build the call-to-impl lines inside the try block.
/// For non-void, assigns `auto result = ...`. For void, just calls.
fn build_guest_call_expr(contract_lower: &str, func: &ResolvedFunction) -> String {
    let is_void_return: bool = matches!(
        func.returns.as_ref(),
        None | Some(ResolvedTypeRef::AbiType(AbiBuiltin::Void))
    );

    let result_prefix: &str = if is_void_return { "" } else { "auto result = " };

    if func.params.is_empty() {
        // No params — ignore args entirely
        return format!(
            "        // SAFETY: args is null for this function per ABI contract; no dereference needed.\n\
             (void)args;\n        {}g_{}_impl->{}();\n",
            result_prefix, contract_lower, func.name
        );
    }

    if func.params.len() == 1 {
        let param: &crate::ir::ResolvedParam = &func.params[0];
        match &param.ty {
            ResolvedTypeRef::UserDefined(type_name) => {
                // Single user-defined struct param — dereference args directly
                return format!(
                    "        // SAFETY: args is a valid const void* pointing to a {type_name} per ABI contract.\n\
             // The host guarantees proper alignment and size before calling this wrapper.\n\
             {}g_{}_impl->{}(*static_cast<const {}*>(args));\n",
                    result_prefix, contract_lower, func.name, type_name
                );
            }
            ResolvedTypeRef::Primitive(_) | ResolvedTypeRef::AbiType(_) => {
                // Single primitive param — dereference args as the primitive type
                let cpp_ty: String = cpp_type_name(&param.ty);
                return format!(
                    "        // SAFETY: args is a valid const void* pointing to a {cpp_ty} per ABI contract.\n\
             // The host guarantees proper alignment and size before calling this wrapper.\n\
             {}g_{}_impl->{}(*static_cast<const {}*>(args));\n",
                    result_prefix, contract_lower, func.name, cpp_ty
                );
            }
        }
    }

    // Multiple params — use a packed struct
    let func_name_cap: String = capitalise_first(&func.name);
    let struct_name: String = format!("{}Args", func_name_cap);

    let mut code: String = String::new();
    // SAFETY comments for generated code are required per AGENTS.md for all unsafe operations
    code.push_str("        // SAFETY: args is a valid const void* pointing to a packed struct layout per ABI contract.\n");
    code.push_str("        // The host guarantees proper alignment and size matching the struct definition below.\n");
    // Inline struct definition
    code.push_str(&format!("        struct {} {{", struct_name));
    for param in &func.params {
        let cpp_ty: String = cpp_type_name(&param.ty);
        code.push_str(&format!(" {} {};", cpp_ty, param.name));
    }
    code.push_str(" };\n");

    // Cast args to packed struct pointer
    code.push_str(&format!(
        "        const {name}* packed = static_cast<const {name}*>(args);\n",
        name = struct_name
    ));

    // Build call argument list
    let call_args: Vec<String> = func
        .params
        .iter()
        .map(|p| format!("packed->{}", p.name))
        .collect();
    let call_args_str: String = call_args.join(", ");

    code.push_str(&format!(
        "        {}g_{}_impl->{}({});\n",
        result_prefix, contract_lower, func.name, call_args_str
    ));

    code
}

// ─── init.hpp generator ──────────────────────────────────────────────────────

fn generate_init_hpp(ir: &ValidatedIr) -> Result<String, PolyplugcError> {
    let mut out: String = String::new();
    out.push_str("// THIS FILE IS AUTO-GENERATED BY polyplugc. DO NOT EDIT.\n");
    out.push_str("// Re-generate with: polyplugc generate --api api.toml --lang cpp --out <dir>\n");
    out.push_str("#pragma once\n");
    out.push_str("#include \"interfaces.hpp\"\n");
    out.push_str("#include \"polyplug/abi.hpp\"\n");
    out.push_str("#include \"polyplug/guest.hpp\"\n\n");

    out.push_str("namespace polyplug_plugin {\n\nusing namespace polyplug_generated;\n\n");
    if let Some(bundle) = &ir.bundle {
        for plugin in &bundle.plugins {
            let plugin_lower: String = plugin.name.to_lowercase().replace('.', "_");
            let class_name = if let Some(contract_impl) = plugin.implements.first() {
                if let Some(contract) = ir.contracts.iter().find(|c| {
                    let contract_full =
                        format!("{}@{}.{}", c.name, c.version.major, c.version.minor);
                    &contract_full == contract_impl
                }) {
                    contract_name_to_plugin_class(&contract.name)
                } else {
                    "IPlugin".to_string()
                }
            } else {
                "IPlugin".to_string()
            };
            out.push_str(&format!(
                "{}* g_{}_impl = nullptr;\n",
                class_name, plugin_lower
            ));
        }
    } else {
        for contract in &ir.contracts {
            let lower: String = contract_name_to_lower_snake(&contract.name);
            let class_name: String = contract_name_to_plugin_class(&contract.name);
            out.push_str(&format!("{}* g_{}_impl = nullptr;\n", class_name, lower));
        }
    }
    out.push_str("\n}  // namespace polyplug_plugin\n\n");

    // polyplug_abi_version
    out.push_str("extern \"C\" uint32_t polyplug_abi_version() { return 1U; }\n\n");

    // polyplug_init
    out.push_str("extern \"C\" AbiError polyplug_init(void* rt_ctx, const RuntimeAbi* host, const PluginContext* ctx) {\n");
    out.push_str("    if (!rt_ctx || !host || !ctx) {\n");
    out.push_str(
        "        static constexpr const char* err_msg = \"null parameter in polyplug_init\";\n",
    );
    out.push_str(
        "        return AbiError{1U, StringView{reinterpret_cast<const uint8_t*>(err_msg), 32}};\n",
    );
    out.push_str("    }\n\n");
    out.push_str("    // Store host vtable for later access via polyplug::get_host_interface()\n");
    out.push_str("    polyplug::store_host_vtable(host);\n\n");

    if let Some(bundle) = &ir.bundle {
        for plugin in &bundle.plugins {
            let plugin_upper: String = plugin.name.to_uppercase().replace('.', "_");
            let plugin_lower: String = plugin.name.to_lowercase().replace('.', "_");
            let contract_impl = plugin.implements.first().map(|s| s.as_str()).unwrap_or("");
            let (contract_name, version_str) = contract_impl
                .split_once('@')
                .unwrap_or((contract_impl, "1.0.0"));
            let (version_major, version_minor_patch) =
                version_str.split_once('.').unwrap_or((version_str, "0"));
            let version_minor = version_minor_patch.split('.').next().unwrap_or("0");
            let contract_name_full = format!("{}@{}", contract_name, version_major);

            out.push_str(&format!("    // Register plugin: {}\n", plugin.name));
            out.push_str(&format!(
                "    polyplug_plugin::set_{}_impl(polyplug_plugin::create_{}_impl());\n",
                plugin_lower, plugin_lower
            ));

            out.push_str(&format!(
                "    PluginDescriptor desc_{} = {{\n",
                plugin_upper
            ));
            out.push_str(&format!(
                "        {{ (const uint8_t*)\"{name}\", {len}U }},  // name (StringView)\n",
                name = plugin.name,
                len = plugin.name.len()
            ));
            out.push_str(&format!(
                "        {{ (const uint8_t*)\"{name}\", {len}U }},  // contract_name (StringView)\n",
                name = contract_name_full,
                len = contract_name_full.len()
            ));
            out.push_str(&format!(
                "        {{ {}U, {}U, 0U }}  // version (Version)\n",
                version_major, version_minor
            ));
            out.push_str("    };\n");

            out.push_str(&format!(
                "    AbiError err_{upper} = host->register_contract(rt_ctx, &desc_{upper}, &polyplug_plugin::{upper}_VTABLE);\n",
                upper = plugin_upper
            ));
            out.push_str(&format!(
                "    if (err_{}.code != 0U) return err_{};\n\n",
                plugin_upper, plugin_upper
            ));
        }
    } else {
        for contract in &ir.contracts {
            generate_init_hpp_register_contract(&mut out, contract)?;
        }
    }

    out.push_str("    return AbiError{static_cast<uint32_t>(AbiErrorCode::Ok), StringView{nullptr, 0}};\n");
    out.push_str("}\n");

    Ok(out)
}

fn generate_init_hpp_register_contract(
    out: &mut String,
    contract: &ResolvedContract,
) -> Result<(), PolyplugcError> {
    let lower: String = contract_name_to_lower_snake(&contract.name);
    let upper: String = contract_name_to_upper_snake(&contract.name);
    let name_bytes: usize = contract.name.len();

    out.push_str(&format!("    // Register contract: {}\n", contract.name));
    out.push_str(&format!(
        "    polyplug_plugin::g_{0}_impl = polyplug_plugin::create_{0}_impl();\n",
        lower
    ));

    out.push_str("    PluginDescriptor desc_");
    out.push_str(&upper);
    out.push_str(" = {\n");
    let contract_name_full: String = format!(
        "{}@{}.{}",
        contract.name, contract.version.major, contract.version.minor
    );
    let name_bytes_full: usize = contract_name_full.len();
    out.push_str(&format!(
        "        {{ (const uint8_t*)\"{name}\", {len}U }},  // name (StringView)\n",
        name = contract.name,
        len = name_bytes
    ));
    out.push_str(&format!(
        "        {{ (const uint8_t*)\"{name}\", {len}U }},  // contract_name (StringView)\n",
        name = contract_name_full,
        len = name_bytes_full
    ));
    out.push_str(&format!(
        "        {{ {}U, {}U, {}U }}  // version (Version)\n",
        contract.version.major, contract.version.minor, contract.version.patch
    ));
    out.push_str("    };\n");

    out.push_str(&format!(
        "    AbiError err_{upper} = host->register_contract(rt_ctx, &desc_{upper}, &polyplug_plugin::{upper}_VTABLE);\n",
        upper = upper
    ));
    out.push_str(&format!(
        "    if (err_{}.code != 0U) return err_{};\n\n",
        upper, upper
    ));

    Ok(())
}

// ─── host_callers.hpp generator ──────────────────────────────────────────────

fn generate_host_callers_hpp(ir: &ValidatedIr) -> Result<String, PolyplugcError> {
    let mut out: String = String::new();
    out.push_str("// THIS FILE IS AUTO-GENERATED BY polyplugc. DO NOT EDIT.\n");
    out.push_str("// Re-generate with: polyplugc generate --api api.toml --lang cpp --out <dir>\n");
    out.push_str("#pragma once\n");
    out.push_str("#include \"types.hpp\"\n");
    out.push_str("#include \"polyplug/error.hpp\"\n");
    out.push_str("#include \"polyplug/abi.hpp\"\n");
    out.push_str("#include \"polyplug/runtime.hpp\"\n");
    out.push_str("#include <optional>\n\n");
    out.push_str("namespace polyplug_generated {\n\n");
    if let Some(ref bundle) = ir.bundle {
        out.push_str(&format!(
            "static constexpr uint64_t MY_BUNDLE_ID = {}ULL;\n\n",
            bundle.bundle_id
        ));
    }

    for contract in &ir.contracts {
        generate_cpp_host_contract(&mut out, contract)?;
    }

    out.push_str("}  // namespace polyplug_generated\n");
    Ok(out)
}

// ─── manifest.toml generator ─────────────────────────────────────────────────

fn generate_manifest_toml() -> String {
    let mut out: String = String::new();
    out.push_str("# THIS FILE IS AUTO-GENERATED BY polyplugc. DO NOT EDIT.\n");
    out.push_str("[manifest]\n");
    out.push_str("schema_version = 1\n");
    out.push_str("lang = \"cpp\"\n");
    out.push_str("generated_by = \"polyplugc\"\n");
    out
}

/// Generate a full discovery `manifest.toml` for `--bundle` invocations.
/// Called only when `ir.bundle.is_some()`.
fn generate_bundle_manifest_cpp(ir: &ValidatedIr) -> String {
    let bundle: &ResolvedBundle = match ir.bundle.as_ref() {
        Some(b) => b,
        None => return String::from("# ERROR: bundle manifest called without bundle IR\n"),
    };

    let name: &str = &bundle.name;
    let version: String = format!(
        "{}.{}.{}",
        bundle.version.major, bundle.version.minor, bundle.version.patch
    );
    // C++ native runtime uses platform-specific shared libraries from bundle.toml
    let file_field: String = super::format_manifest_file_field(&bundle.file);

    // Collect provides: all implements from all plugins, deduplicated
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

    // Build TOML string array for provides
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

    // Build function_count inline table: only for contracts this bundle PROVIDES
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

    // Build [[dependency]] tables
    let mut dep_tables: String = String::new();
    for dep in &bundle.dependencies {
        match dep {
            ResolvedDependency::ByContract {
                contract,
                contract_id,
                min_version,
            } => {
                dep_tables.push_str(&format!(
                    "[[dependency]]\ncontract = \"{}\"\ncontract_id = 0x{:016X}\nmin_version = {}\n\n",
                    contract, contract_id, min_version
                ));
            }
            ResolvedDependency::ByBundle {
                bundle: dep_bundle,
                bundle_id,
                contract,
                contract_id,
                min_version,
            } => {
                dep_tables.push_str(&format!(
                    "[[dependency]]\nbundle = \"{}\"\nbundle_id = 0x{:016X}\ncontract = \"{}\"\ncontract_id = 0x{:016X}\nmin_version = {}\n\n",
                    dep_bundle, bundle_id, contract, contract_id, min_version
                ));
            }
        }
    }

    let reinit: bool = bundle.needs_reinit_on_dep_reload;
    let runtime: &str = "native";

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
{dep_tables}",
        bundle_id = bundle.bundle_id
    )
}

// ─── Per-enum emitter ────────────────────────────────────────────────────────

fn substitute_variant_refs_cpp(
    declared_variants: &[EnumVariant],
    expr: &str,
    enum_name: &str,
    repr_cpp: &str,
) -> String {
    let declared_names: Vec<&str> = declared_variants.iter().map(|v| v.name.as_str()).collect();
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
            if declared_names.contains(&ident.as_str()) {
                result.push_str(&format!(
                    "static_cast<{}>({}::{})",
                    repr_cpp, enum_name, ident
                ));
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

fn generate_cpp_enum(out: &mut String, e: &EnumDef) {
    let repr_cpp: &str = e.repr.cpp_name();
    out.push_str(&format!("/// Enum `{}` (repr: {})\n", e.name, repr_cpp));
    out.push_str(&format!("enum class {} : {} {{\n", e.name, repr_cpp));
    for variant in &e.variants {
        let subst_value: String =
            substitute_variant_refs_cpp(&e.variants, &variant.value, &e.name, repr_cpp);
        out.push_str(&format!("    {} = {},\n", variant.name, subst_value));
    }
    out.push_str("};\n");
    if e.bitflag {
        out.push_str(&format!(
            "inline {} operator|({}  a, {} b) {{ return static_cast<{}>(static_cast<{}>(a) | static_cast<{}>(b)); }}\n",
            e.name, e.name, e.name, e.name, repr_cpp, repr_cpp
        ));
        out.push_str(&format!(
            "inline {} operator&({} a, {} b) {{ return static_cast<{}>(static_cast<{}>(a) & static_cast<{}>(b)); }}\n",
            e.name, e.name, e.name, e.name, repr_cpp, repr_cpp
        ));
        out.push_str(&format!(
            "inline {} operator~({} a) {{ return static_cast<{}>(~static_cast<{}>(a)); }}\n",
            e.name, e.name, e.name, repr_cpp
        ));
    }
    out.push('\n');
}

// ─── Per-type struct emitter ──────────────────────────────────────────────────

fn generate_cpp_type(out: &mut String, ty: &ResolvedType) {
    out.push_str(&format!("/// User-defined type `{}`\n", ty.name));
    out.push_str("struct ");
    out.push_str(&ty.name);
    out.push_str(" {\n");
    for field in &ty.fields {
        out.push_str(&format!(
            "    {} {};\n",
            cpp_type_name(&field.ty),
            field.name
        ));
    }
    out.push_str("};\n\n");
}

// ─── Per-contract class emitter ───────────────────────────────────────────────

fn generate_cpp_host_contract(
    out: &mut String,
    contract: &ResolvedContract,
) -> Result<(), PolyplugcError> {
    let class_name: String = contract_name_to_class(&contract.name);
    let contract_upper: String = contract.name.to_uppercase().replace(['.', '-'], "_");

    out.push_str(&format!(
        "/// Host caller for contract `{}` (id=0x{:016X})\n",
        contract.name, contract.contract_id
    ));
    out.push_str(&format!("class {} {{\npublic:\n", class_name));

    out.push_str("    /// Factory method - creates instance or nullopt if not found.\n");
    out.push_str(&format!(
        "    static std::optional<{}> create(polyplug::Runtime& rt, uint32_t min_version = 0) noexcept {{\n",
        class_name
    ));
    out.push_str(&format!(
        "        uint64_t handle = rt.find({}_CONTRACT_ID, min_version);\n",
        contract_upper
    ));
    out.push_str("        if (handle == UINT64_MAX) {\n");
    out.push_str("            return std::nullopt;\n");
    out.push_str("        }\n");
    out.push_str("        polyplug::PluginGuard guard = rt.resolve_plugin(handle);\n");
    out.push_str("        if (!guard) {\n");
    out.push_str("            return std::nullopt;\n");
    out.push_str("        }\n");
    out.push_str(&format!(
        "        return {}(std::move(guard));\n",
        class_name
    ));
    out.push_str("    }\n\n");

    out.push_str("    // Move-only (guard is not copyable)\n");
    out.push_str(&format!(
        "    {}({}&&) noexcept = default;\n",
        class_name, class_name
    ));
    out.push_str(&format!(
        "    {}& operator=({}&&) noexcept = default;\n",
        class_name, class_name
    ));
    out.push_str(&format!(
        "    {}(const {}&) = delete;\n",
        class_name, class_name
    ));
    out.push_str(&format!(
        "    {}& operator=(const {}&) = delete;\n\n",
        class_name, class_name
    ));

    out.push_str("    /// Check if instance is valid.\n");
    out.push_str(
        "    explicit operator bool() const noexcept { return static_cast<bool>(guard_); }\n\n",
    );
    out.push_str("    /// Check if instance is valid.\n");
    out.push_str("    bool is_valid() const noexcept { return static_cast<bool>(guard_); }\n\n");
    out.push_str("    /// Explicitly destroy instance (optional - destructor does this too).\n");
    out.push_str("    void reset() noexcept { guard_ = polyplug::PluginGuard{}; }\n\n");

    for func in &contract.functions {
        generate_cpp_host_function(out, &class_name, func)?;
    }

    out.push_str("private:\n");
    out.push_str(&format!(
        "    explicit {}(polyplug::PluginGuard guard) noexcept\n",
        class_name
    ));
    out.push_str("        : guard_(std::move(guard)) {}\n\n");
    out.push_str("    polyplug::PluginGuard guard_;\n");
    out.push_str("};\n\n");
    Ok(())
}

// ─── Per-function method emitter ──────────────────────────────────────────────

fn generate_cpp_host_function(
    out: &mut String,
    class_name: &str,
    func: &ResolvedFunction,
) -> Result<(), PolyplugcError> {
    let return_type: String = func
        .returns
        .as_ref()
        .map(cpp_type_name)
        .unwrap_or_else(|| "void".to_owned());

    // Build parameter list string.
    let params: Vec<String> = func
        .params
        .iter()
        .map(|p| format!("{} {}", cpp_type_name(&p.ty), p.name))
        .collect();
    let params_str: String = params.join(", ");

    out.push_str(&format!(
        "    {} {}({}) {{\n",
        return_type, func.name, params_str
    ));

    // Determine args_ptr expression.
    let args_ptr_code: String = build_args_ptr_code(class_name, func);
    out.push_str(&args_ptr_code);

    let fn_id: u32 = func.function_id;

    let is_void_return: bool = matches!(
        func.returns.as_ref(),
        None | Some(ResolvedTypeRef::AbiType(AbiBuiltin::Void))
    );

    out.push_str("        const PluginInterface* vtable = guard_.interface();\n");
    out.push_str("        if (!vtable) {\n");
    out.push_str("            static constexpr const char* err_msg = \"vtable is null\";\n");
    out.push_str("            polyplug::check_abi_error(AbiError{4, StringView{reinterpret_cast<const uint8_t*>(err_msg), 14}});\n");
    out.push_str("        }\n");

    if is_void_return {
        out.push_str(&format!(
            "        if ({}_u32 >= vtable->function_count) {{\n",
            fn_id
        ));
        out.push_str("            static constexpr const char* err_msg = \"function not available in vtable\";\n");
        out.push_str("            polyplug::check_abi_error(AbiError{4, StringView{reinterpret_cast<const uint8_t*>(err_msg), 32}});\n");
        out.push_str("        }\n");
        out.push_str(&format!(
            "        auto fn_ = reinterpret_cast<AbiError(*)(const void*, void*)>(vtable->dispatch.native.functions[{}U]);\n",
            fn_id
        ));
        out.push_str("        AbiError err = fn_(args_ptr, nullptr);\n");
        out.push_str("        polyplug::check_abi_error(err);\n");
    } else {
        out.push_str(&format!("        {} out{{}};\n", return_type));
        out.push_str("        void* out_ptr = &out;\n");
        out.push_str(&format!(
            "        if ({}_u32 >= vtable->function_count) {{\n",
            fn_id
        ));
        out.push_str("            static constexpr const char* err_msg = \"function not available in vtable\";\n");
        out.push_str("            polyplug::check_abi_error(AbiError{4, StringView{reinterpret_cast<const uint8_t*>(err_msg), 32}});\n");
        out.push_str("        }\n");
        out.push_str(&format!(
            "        auto fn_ = reinterpret_cast<AbiError(*)(const void*, void*)>(vtable->dispatch.native.functions[{}U]);\n",
            fn_id
        ));
        out.push_str("        AbiError err = fn_(args_ptr, out_ptr);\n");
        out.push_str("        polyplug::check_abi_error(err);\n");
        out.push_str("        return out;\n");
    }

    out.push_str("    }\n\n");
    Ok(())
}

/// Build the `args_ptr` preamble lines for a function body.
///
/// Returns the lines (indented 8 spaces) that set up `args_ptr`.
fn build_args_ptr_code(class_name: &str, func: &ResolvedFunction) -> String {
    if func.params.is_empty() {
        // No args — pass nullptr.
        return "        const void* args_ptr = nullptr;\n".to_owned();
    }

    if func.params.len() == 1 {
        let param = &func.params[0];
        match &param.ty {
            ResolvedTypeRef::UserDefined(_) => {
                // User-defined struct: pass pointer directly.
                return format!("        const void* args_ptr = &{};\n", param.name);
            }
            ResolvedTypeRef::Primitive(_) | ResolvedTypeRef::AbiType(_) => {
                // Single primitive: store in local and pass pointer.
                let cpp_ty: String = cpp_type_name(&param.ty);
                return format!(
                    "        const {cpp_ty} local_{name} = {name};\n        const void* args_ptr = &local_{name};\n",
                    cpp_ty = cpp_ty,
                    name = param.name
                );
            }
        }
    }

    // Multiple params: pack into a generated args struct.
    // Capitalise the function name for the struct name.
    let func_name_cap: String = capitalise_first(&func.name);
    let struct_name: String = format!("{}{}{}", class_name, func_name_cap, "Args");

    let mut code: String = String::new();
    // Inline struct definition.
    code.push_str(&format!("        struct {} {{", struct_name));
    for param in &func.params {
        let cpp_ty: String = cpp_type_name(&param.ty);
        code.push_str(&format!(" {} {};", cpp_ty, param.name));
    }
    code.push_str(" };\n");

    // Initialise the struct.
    let field_inits: Vec<String> = func.params.iter().map(|p| p.name.clone()).collect();
    code.push_str(&format!(
        "        {} args_val{{ {} }};\n",
        struct_name,
        field_inits.join(", ")
    ));
    code.push_str("        const void* args_ptr = &args_val;\n");
    code
}

// ─── Utility helpers ──────────────────────────────────────────────────────────

fn cpp_type_name(ty: &ResolvedTypeRef) -> String {
    match ty {
        ResolvedTypeRef::Primitive(p) => p.cpp_name().to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => "StringView".to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => "Buffer".to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::Ptr) => "void*".to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::Void) => "void".to_owned(),
        ResolvedTypeRef::UserDefined(name) => name.clone(),
    }
}

fn contract_name_to_class(name: &str) -> String {
    // Convert "image.decode" -> "ImageDecodeContract"
    name.split('.')
        .map(|p| {
            let mut chars: core::str::Chars<'_> = p.chars();
            match chars.next() {
                Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join("")
        + "Contract"
}

/// Convert "test.add" → "TestAddPlugin"
fn contract_name_to_plugin_class(name: &str) -> String {
    name.split('.')
        .map(|p| {
            let mut chars: core::str::Chars<'_> = p.chars();
            match chars.next() {
                Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join("")
        + "Plugin"
}

/// Convert "test.add" → "test_add"
fn contract_name_to_lower_snake(name: &str) -> String {
    name.replace('.', "_")
}

/// Convert "test.add" → "TEST_ADD"
fn contract_name_to_upper_snake(name: &str) -> String {
    name.replace('.', "_").to_uppercase()
}

fn capitalise_first(s: &str) -> String {
    let mut chars: core::str::Chars<'_> = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

// ─── Host Contract Trait Generation ───────────────────────────────────────────

/// Convert host contract name to C++ abstract class name.
fn host_contract_name_to_cpp_trait(name: &str) -> String {
    let name_without_prefix: &str = name.strip_prefix("host.").unwrap_or(name);

    let pascal: String = name_without_prefix
        .split('.')
        .map(|p| {
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

/// Generate C++ host-side type name for trait method parameters.
fn cpp_host_param_type_name(ty: &ResolvedTypeRef) -> String {
    match ty {
        ResolvedTypeRef::Primitive(p) => p.cpp_name().to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => "StringView".to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => "Buffer".to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::Ptr) => "void*".to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::Void) => "void".to_owned(),
        ResolvedTypeRef::UserDefined(name) => name.clone(),
    }
}

/// Generate C++ host-side return type name for trait methods.
fn cpp_host_return_type_name(ty: &ResolvedTypeRef) -> String {
    match ty {
        ResolvedTypeRef::Primitive(p) => p.cpp_name().to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => "StringView".to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => "Buffer".to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::Ptr) => "void*".to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::Void) => "void".to_owned(),
        ResolvedTypeRef::UserDefined(name) => name.clone(),
    }
}

/// Generate the abstract class definition for one host contract.
fn generate_cpp_host_contract_trait(out: &mut String, contract: &ResolvedHostContract) {
    let class_name: String = host_contract_name_to_cpp_trait(&contract.name);
    out.push_str(&format!(
        "/// Host abstract class for contract `{}` (id=0x{:016X})\n",
        contract.name, contract.contract_id
    ));
    out.push_str("/// Hosts implement this class to provide functionality to plugins.\n");
    out.push_str(&format!("class {} {{\npublic:\n", class_name));
    out.push_str(&format!("    virtual ~{}() = default;\n", class_name));

    for func in &contract.functions {
        generate_cpp_host_trait_method(out, func);
    }

    out.push_str("};\n\n");
}

// ─── Guest Host Contract Caller Generation ─────────────────────────────────────

/// Convert host contract name to C++ guest caller class name.
/// e.g. "host.logger" -> "HostLoggerContract", "host.fs.reader" -> "HostFsReaderContract"
fn host_contract_name_to_cpp_caller(name: &str) -> String {
    let name_without_prefix: &str = name.strip_prefix("host.").unwrap_or(name);

    let pascal: String = name_without_prefix
        .split('.')
        .map(|p| {
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

/// Generate C++ guest-side type name for caller method parameters.
/// For guest callers, we use ergonomic C++ types:
/// - StringView -> std::string_view (borrowed view)
/// - Buffer -> Buffer (ABI type, passed by value)
/// - UserDefined -> const TypeName& (passed by const reference)
/// - Primitives -> T (passed by value)
fn cpp_guest_caller_param_type_name(ty: &ResolvedTypeRef) -> String {
    match ty {
        ResolvedTypeRef::Primitive(p) => p.cpp_name().to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => "std::string_view".to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => "Buffer".to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::Ptr) => "void*".to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::Void) => "void".to_owned(),
        ResolvedTypeRef::UserDefined(name) => format!("const {name}&"),
    }
}

/// Generate C++ guest-side return type name for caller methods.
/// Return types are ABI types where appropriate:
/// - StringView -> StringView (ABI type, caller must copy if needed)
/// - Buffer -> Buffer (ABI type)
/// - UserDefined -> TypeName (by value)
/// - Primitives -> T (by value)
fn cpp_guest_caller_return_type_name(ty: &ResolvedTypeRef) -> String {
    match ty {
        ResolvedTypeRef::Primitive(p) => p.cpp_name().to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => "StringView".to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => "Buffer".to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::Ptr) => "void*".to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::Void) => "void".to_owned(),
        ResolvedTypeRef::UserDefined(name) => name.clone(),
    }
}

/// Generate one guest-side host contract caller class.
fn generate_cpp_guest_host_contract_caller(out: &mut String, contract: &ResolvedHostContract) {
    let class_name: String = host_contract_name_to_cpp_caller(&contract.name);

    out.push_str(&format!(
        "/// Guest caller for host contract `{}` (id=0x{:016X})\n",
        contract.name, contract.contract_id
    ));
    out.push_str("/// Plugins use this class to call host-provided functionality.\n");
    out.push_str(&format!("class {} {{\npublic:\n", class_name));

    // Factory method - from_host
    out.push_str(
        "    /// Factory method - creates caller from RuntimeAbi or nullopt if not found.\n",
    );
    out.push_str(&format!(
        "    static std::optional<{}> from_host(const RuntimeAbi* host, uint32_t min_version = 0) noexcept {{\n",
        class_name
    ));
    out.push_str("        if (host == nullptr) {\n");
    out.push_str("            return std::nullopt;\n");
    out.push_str("        }\n");
    out.push_str(&format!(
        "        const HostContractVTable* vtable = host->get_host_contract(nullptr, 0x{:016X}ULL, min_version);\n",
        contract.contract_id
    ));
    out.push_str("        if (vtable == nullptr) {\n");
    out.push_str("            return std::nullopt;\n");
    out.push_str("        }\n");
    out.push_str(&format!("        return {}(vtable);\n", class_name));
    out.push_str("    }\n\n");

    // is_valid method
    out.push_str("    /// Check if caller is valid (vtable is non-null).\n");
    out.push_str("    bool is_valid() const noexcept { return vtable_ != nullptr; }\n\n");

    // Explicit bool conversion
    out.push_str("    /// Explicit bool conversion for validity check.\n");
    out.push_str("    explicit operator bool() const noexcept { return vtable_ != nullptr; }\n\n");

    // Methods for each function
    for func in &contract.functions {
        generate_cpp_guest_host_contract_method(out, func, &class_name);
    }

    // Private section
    out.push_str("private:\n");
    out.push_str(&format!(
        "    explicit {}(const HostContractVTable* vtable) noexcept\n",
        class_name
    ));
    out.push_str("        : vtable_(vtable) {}\n\n");
    out.push_str("    const HostContractVTable* vtable_;\n");
    out.push_str("};\n\n");
}

/// Generate one method for a guest-side host contract caller.
fn generate_cpp_guest_host_contract_method(
    out: &mut String,
    func: &ResolvedFunction,
    class_name: &str,
) {
    let fn_id: u32 = func.function_id;

    let return_type: String = func
        .returns
        .as_ref()
        .map(cpp_guest_caller_return_type_name)
        .unwrap_or_else(|| "void".to_owned());

    // Build parameter list
    let params: Vec<String> = func
        .params
        .iter()
        .map(|p| format!("{} {}", cpp_guest_caller_param_type_name(&p.ty), p.name))
        .collect();
    let params_str: String = params.join(", ");

    out.push_str(&format!(
        "    /// Call host contract function `{}` (function_id={})\n",
        func.name, fn_id
    ));
    out.push_str(&format!(
        "    {} {}({}) noexcept {{\n",
        return_type, func.name, params_str
    ));

    // Null vtable check
    out.push_str("        if (vtable_ == nullptr) {\n");
    if func.returns.is_some() {
        out.push_str(&format!("            return {}{{}};\n", return_type));
    } else {
        out.push_str("            return;\n");
    }
    out.push_str("        }\n\n");

    // Get header and check function count
    out.push_str("        const HostContractVTableHeader* header = &vtable_->header;\n");
    out.push_str(&format!(
        "        if ({fn_id}_u32 >= header->function_count) {{\n"
    ));
    if func.returns.is_some() {
        out.push_str(&format!("            return {}{{}};\n", return_type));
    } else {
        out.push_str("            return;\n");
    }
    out.push_str("        }\n\n");

    // Build args_ptr setup
    emit_cpp_guest_host_contract_args_setup(out, func, class_name);

    // Build out_ptr setup
    emit_cpp_guest_host_contract_out_setup(out, &func.returns);

    // Dispatch call
    out.push_str("        AbiError err;\n");
    out.push_str("        switch (header->dispatch_type) {\n");
    out.push_str("            case DispatchType::Native: {\n");
    out.push_str(&format!(
        "                auto fn_ = reinterpret_cast<AbiError(*)(const void*, const void*, void*)>(vtable_->dispatch.native.functions[{fn_id}_u32]);\n"
    ));
    out.push_str(
        "                err = fn_(vtable_->dispatch.native.impl_ptr, args_ptr, out_ptr);\n",
    );
    out.push_str("                break;\n");
    out.push_str("            }\n");
    out.push_str("            case DispatchType::VirtualMachine: {\n");
    out.push_str(&format!(
        "                err = (vtable_->dispatch.vm.call)(vtable_->dispatch.vm.bridge_data, {fn_id}_u32, args_ptr, out_ptr);\n"
    ));
    out.push_str("                break;\n");
    out.push_str("            }\n");
    out.push_str("        }\n\n");

    // Error handling - for now, just return default on error
    out.push_str("        if (err.code != static_cast<uint32_t>(AbiErrorCode::Ok)) {\n");
    if func.returns.is_some() {
        out.push_str(&format!("            return {}{{}};\n", return_type));
    } else {
        out.push_str("            return;\n");
    }
    out.push_str("        }\n\n");

    // Return result
    if func.returns.is_some() {
        out.push_str("        return out;\n");
    }

    out.push_str("    }\n\n");
}

/// Emit the args_ptr setup for a C++ guest host contract method.
fn emit_cpp_guest_host_contract_args_setup(
    out: &mut String,
    func: &ResolvedFunction,
    class_name: &str,
) {
    if func.params.is_empty() {
        out.push_str("        const void* args_ptr = nullptr;\n");
        return;
    }

    if func.params.len() == 1 {
        let param: &crate::ir::ResolvedParam = &func.params[0];
        match &param.ty {
            ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => {
                // std::string_view -> StringView conversion
                out.push_str(&format!(
                    "        StringView {}_view{{ reinterpret_cast<const uint8_t*>({}.data()), {}.size() }};\n",
                    param.name, param.name, param.name
                ));
                out.push_str(&format!(
                    "        const void* args_ptr = &{}_view;\n",
                    param.name
                ));
            }
            ResolvedTypeRef::UserDefined(_) => {
                // User-defined struct - pass pointer directly
                out.push_str(&format!(
                    "        const void* args_ptr = &{};\n",
                    param.name
                ));
            }
            ResolvedTypeRef::Primitive(_) | ResolvedTypeRef::AbiType(_) => {
                // Primitive or other ABI type - store in local and pass pointer
                let cpp_ty: String = cpp_type_name(&param.ty);
                out.push_str(&format!(
                    "        const {cpp_ty} local_{name} = {name};\n",
                    cpp_ty = cpp_ty,
                    name = param.name
                ));
                out.push_str(&format!(
                    "        const void* args_ptr = &local_{name};\n",
                    name = param.name
                ));
            }
        }
        return;
    }

    // Multiple params: pack into inline struct
    let func_name_cap: String = capitalise_first(&func.name);
    let struct_name: String = format!("{}{}Args", class_name, func_name_cap);

    out.push_str(&format!("        struct {} {{", struct_name));
    for param in &func.params {
        let cpp_ty: String = cpp_guest_caller_abi_type_name(&param.ty);
        out.push_str(&format!(" {} {};", cpp_ty, param.name));
    }
    out.push_str(" };\n");

    // Initialize struct fields
    let field_inits: Vec<String> = func
        .params
        .iter()
        .map(|p| match &p.ty {
            ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => {
                format!(
                    "StringView{{ reinterpret_cast<const uint8_t*>({}.data()), {}.size() }}",
                    p.name, p.name
                )
            }
            _ => p.name.clone(),
        })
        .collect();

    out.push_str(&format!(
        "        {} args_val{{ {} }};\n",
        struct_name,
        field_inits.join(", ")
    ));
    out.push_str("        const void* args_ptr = &args_val;\n");
}

/// Get the ABI type name for packing into arg structs.
/// For StringView parameters, we need the ABI type, not std::string_view.
fn cpp_guest_caller_abi_type_name(ty: &ResolvedTypeRef) -> String {
    match ty {
        ResolvedTypeRef::Primitive(p) => p.cpp_name().to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => "StringView".to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => "Buffer".to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::Ptr) => "void*".to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::Void) => "void".to_owned(),
        ResolvedTypeRef::UserDefined(name) => name.clone(),
    }
}

/// Emit the out_ptr setup for a C++ guest host contract method.
fn emit_cpp_guest_host_contract_out_setup(out: &mut String, returns: &Option<ResolvedTypeRef>) {
    if let Some(ret_ty) = returns {
        let cpp_ty: String = cpp_guest_caller_return_type_name(ret_ty);
        out.push_str(&format!("        {} out{{}};\n", cpp_ty));
        out.push_str("        void* out_ptr = &out;\n");
    } else {
        out.push_str("        void* out_ptr = nullptr;\n");
    }
}

/// Generate all guest-side host contract callers into a single file.
fn generate_cpp_guest_host_contracts_file(ir: &ValidatedIr) -> String {
    let mut out: String = String::new();
    out.push_str("// THIS FILE IS AUTO-GENERATED BY polyplugc. DO NOT EDIT.\n");
    out.push_str(
        "// Re-generate with: polyplugc generate --bundle bundle.toml --lang cpp --out <dir>\n",
    );
    out.push_str("#pragma once\n");
    out.push_str("#include \"types.hpp\"\n");
    out.push_str("#include \"polyplug/abi.hpp\"\n");
    out.push_str("#include <cstdint>\n");
    out.push_str("#include <optional>\n");
    out.push_str("#include <string_view>\n\n");
    out.push_str("namespace polyplug_plugin {\n\nusing namespace polyplug_generated;\n\n");

    for contract in &ir.host_contracts {
        generate_cpp_guest_host_contract_caller(&mut out, contract);
    }

    for contract in &ir.host_contracts {
        let class_name: String = host_contract_name_to_cpp_caller(&contract.name);
        let const_name: String = class_name.to_uppercase() + "_ID";
        out.push_str(&format!(
            "/// Contract ID constant for `{}` (FNV-1a of \"host_contract:{}@{}\")\n",
            contract.name, contract.name, contract.version.major
        ));
        out.push_str(&format!(
            "constexpr uint64_t {} = 0x{:016X}ULL;\n\n",
            const_name, contract.contract_id
        ));
    }

    out.push_str("}  // namespace polyplug_plugin\n");
    out
}

/// Generate one pure virtual method for a host contract function.
fn generate_cpp_host_trait_method(out: &mut String, func: &ResolvedFunction) {
    let return_type: String = func
        .returns
        .as_ref()
        .map(cpp_host_return_type_name)
        .unwrap_or_else(|| "void".to_owned());

    let params: Vec<String> = func
        .params
        .iter()
        .map(|p| {
            let cpp_ty: String = cpp_host_param_type_name(&p.ty);
            match &p.ty {
                ResolvedTypeRef::UserDefined(_) => format!("const {}& {}", cpp_ty, p.name),
                ResolvedTypeRef::AbiType(_) => format!("{} {}", cpp_ty, p.name),
                ResolvedTypeRef::Primitive(_) => format!("{} {}", cpp_ty, p.name),
            }
        })
        .collect();
    let params_str: String = params.join(", ");

    out.push_str(&format!(
        "    virtual {} {}({}) = 0;\n",
        return_type, func.name, params_str
    ));
}

/// Generate all host contract traits into a single file.
fn generate_cpp_host_contracts_file(ir: &ValidatedIr) -> String {
    let mut out: String = String::new();
    out.push_str("// THIS FILE IS AUTO-GENERATED BY polyplugc. DO NOT EDIT.\n");
    out.push_str("// Re-generate with: polyplugc generate --api api.toml --lang cpp --out <dir>\n");
    out.push_str("#pragma once\n");
    out.push_str("#include \"types.hpp\"\n");
    out.push_str("#include \"polyplug/abi.hpp\"\n");
    out.push_str("#include <cstdint>\n\n");
    out.push_str("namespace polyplug_host {\n\nusing namespace polyplug_generated;\n\n");

    for contract in &ir.host_contracts {
        generate_cpp_host_contract_trait(&mut out, contract);
    }

    for contract in &ir.host_contracts {
        let trait_name: String = host_contract_name_to_cpp_trait(&contract.name);
        let const_name: String = trait_name.to_uppercase() + "_CONTRACT_ID";
        out.push_str(&format!(
            "/// Contract ID constant for `{}` (FNV-1a of \"host_contract:{}@{}\")\n",
            contract.name, contract.name, contract.version.major
        ));
        out.push_str(&format!(
            "constexpr uint64_t {} = 0x{:016X}ULL;\n\n",
            const_name, contract.contract_id
        ));
    }

    out.push_str("}  // namespace polyplug_host\n");
    out
}

// ─── Host VTable Factories Generation ─────────────────────────────────────────

/// Generate all host-side interface factories into a single file.
fn generate_cpp_host_interface_factories_file(ir: &ValidatedIr) -> String {
    let mut out: String = String::new();
    out.push_str("// THIS FILE IS AUTO-GENERATED BY polyplugc. DO NOT EDIT.\n");
    out.push_str("// Re-generate with: polyplugc generate --api api.toml --lang cpp --out <dir>\n");
    out.push_str("#pragma once\n");
    out.push_str("#include \"host_contracts.hpp\"\n");
    out.push_str("#include \"polyplug/abi.hpp\"\n");
    out.push_str("#include <cstdint>\n");
    out.push_str("#include <memory>\n\n");
    out.push_str("namespace polyplug_host {\n\nusing namespace polyplug_generated;\n\n");

    for contract in &ir.host_contracts {
        generate_cpp_host_interface_factory(&mut out, contract);
    }

    out.push_str("}  // namespace polyplug_host\n");
    out
}

/// Generate interface factories for one host contract.
fn generate_cpp_host_interface_factory(out: &mut String, contract: &ResolvedHostContract) {
    let trait_name: String = host_contract_name_to_cpp_trait(&contract.name);
    let factory_name: String = format!(
        "create_{}_interface",
        contract.name.replace('.', "_").to_lowercase()
    );
    let factory_vm_name: String = format!(
        "create_{}_interface_vm",
        contract.name.replace('.', "_").to_lowercase()
    );
    let fn_count: usize = contract.functions.len();
    let contract_id: u64 = contract.contract_id;
    let major: u32 = contract.version.major;
    let minor: u32 = contract.version.minor;
    let singleton: bool = contract.singleton;

    // NATIVE dispatch factory
    out.push_str(&format!(
        "/// Create a host contract vtable for `{}` with NATIVE dispatch.\n",
        contract.name
    ));
    out.push_str("///\n");
    out.push_str("/// Takes ownership of the implementation and creates a 'static vtable.\n");
    out.push_str("/// The implementation must inherit from the abstract class.\n");
    out.push_str("///\n");
    out.push_str("/// # Memory\n");
    out.push_str("/// The returned vtable pointer is valid for the lifetime of the program.\n");
    out.push_str("/// The implementation unique_ptr is released and managed internally.\n");
    out.push_str("template<typename T>\n");
    out.push_str(&format!(
        "const HostContractVTable* {}(std::unique_ptr<T> impl) noexcept {{\n",
        factory_name
    ));
    out.push_str("    static T* s_impl = nullptr;\n");
    out.push_str("    s_impl = impl.release();\n\n");

    // Generate thunks for each function
    for func in &contract.functions {
        generate_cpp_host_thunk(out, func, &contract.name, &trait_name);
    }

    // Static function pointer array
    out.push_str(&format!(
        "    static constexpr HostContractFn FUNCTIONS[{}] = {{\n",
        fn_count
    ));
    for func in &contract.functions {
        let thunk_name: String = format!(
            "{}_{}_thunk",
            contract.name.replace('.', "_").to_lowercase(),
            func.name
        );
        out.push_str(&format!("        {},\n", thunk_name));
    }
    out.push_str("    };\n\n");

    // Static vtable
    out.push_str("    static HostContractVTable s_vtable = {\n");
    out.push_str("        HostContractVTableHeader{\n");
    out.push_str("            1,  // vtable_version\n");
    out.push_str(&format!(
        "            0x{contract_id:016X}ULL,  // contract_id\n"
    ));
    out.push_str(&format!("            {major}U,  // contract_major\n"));
    out.push_str(&format!("            {minor}U,  // contract_minor\n"));
    out.push_str(&format!("            {fn_count}U,  // function_count\n"));
    out.push_str(&format!("            {},  // singleton\n", singleton));
    out.push_str("            DispatchType::Native,\n");
    out.push_str("        },\n");
    out.push_str("        HostContractDispatch{\n");
    out.push_str("            NativeHostContractDispatch{\n");
    out.push_str("                static_cast<const void*>(static_cast<T*>(nullptr)),  // impl_ptr (unused, we use s_impl)\n");
    out.push_str("                FUNCTIONS,\n");
    out.push_str("            },\n");
    out.push_str("        },\n");
    out.push_str("    };\n\n");
    out.push_str("    (void)s_impl;  // Suppress unused warning - used by thunks\n");
    out.push_str("    return &s_vtable;\n");
    out.push_str("}\n\n");

    // VM dispatch factory
    out.push_str(&format!(
        "/// Create a host contract vtable for `{}` with VM dispatch.\n",
        contract.name
    ));
    out.push_str("///\n");
    out.push_str("/// Used when the host implementation is in a VM language (Python, Lua, JS).\n");
    out.push_str("///\n");
    out.push_str("/// # Arguments\n");
    out.push_str("/// * `bridge_data` - Opaque pointer to VM-specific data\n");
    out.push_str("/// * `dispatch_fn` - Function to call for each contract function\n");
    out.push_str("///\n");
    out.push_str("/// # Memory\n");
    out.push_str("/// The returned vtable pointer is valid for the lifetime of the program.\n");
    out.push_str(&format!("const HostContractVTable* {}(\n", factory_vm_name));
    out.push_str("    void* bridge_data,\n");
    out.push_str("    VmHostContractDispatchFn dispatch_fn\n");
    out.push_str(") noexcept {\n");
    out.push_str("    static HostContractVTable s_vtable = {\n");
    out.push_str("        HostContractVTableHeader{\n");
    out.push_str("            1,  // vtable_version\n");
    out.push_str(&format!(
        "            0x{contract_id:016X}ULL,  // contract_id\n"
    ));
    out.push_str(&format!("            {major}U,  // contract_major\n"));
    out.push_str(&format!("            {minor}U,  // contract_minor\n"));
    out.push_str(&format!("            {fn_count}U,  // function_count\n"));
    out.push_str(&format!("            {},  // singleton\n", singleton));
    out.push_str("            DispatchType::VirtualMachine,\n");
    out.push_str("        },\n");
    out.push_str("        HostContractDispatch{\n");
    out.push_str("            VmHostContractDispatch{\n");
    out.push_str("                dispatch_fn,\n");
    out.push_str("                bridge_data,\n");
    out.push_str("            },\n");
    out.push_str("        },\n");
    out.push_str("    };\n");
    out.push_str("    s_vtable.dispatch.vm.bridge_data = bridge_data;\n");
    out.push_str("    s_vtable.dispatch.vm.call = dispatch_fn;\n");
    out.push_str("    return &s_vtable;\n");
    out.push_str("}\n\n");
}

/// Generate a thunk function for a host contract function.
fn generate_cpp_host_thunk(
    out: &mut String,
    func: &ResolvedFunction,
    contract_name: &str,
    _trait_name: &str,
) {
    let thunk_name: String = format!(
        "{}_{}_thunk",
        contract_name.replace('.', "_").to_lowercase(),
        func.name
    );
    let has_return: bool = func.returns.is_some();

    out.push_str(&format!(
        "    static AbiError {}(const void* impl_ptr, const void* args, void* out) noexcept {{\n",
        thunk_name
    ));
    out.push_str("        (void)impl_ptr;  // We use s_impl directly\n");
    out.push_str("        if (s_impl == nullptr) {\n");
    out.push_str("            return AbiError{static_cast<uint32_t>(AbiErrorCode::Panic), StringView{nullptr, 0}};\n");
    out.push_str("        }\n");
    out.push_str("        try {\n");

    // Generate argument extraction
    if !func.params.is_empty() {
        generate_cpp_host_thunk_args(out, func);
    } else {
        out.push_str("            (void)args;\n");
    }

    // Generate the trait method call
    generate_cpp_host_thunk_call(out, func, has_return);

    // Handle return value
    if has_return {
        let ret_ty: String = func
            .returns
            .as_ref()
            .map(cpp_host_return_type_name)
            .unwrap_or_else(|| "void".to_owned());
        out.push_str(&format!(
            "            *static_cast<{}*>(out) = result;\n",
            ret_ty
        ));
    } else {
        out.push_str("            (void)out;\n");
    }

    out.push_str("            return AbiError{static_cast<uint32_t>(AbiErrorCode::Ok), StringView{nullptr, 0}};\n");
    out.push_str("        } catch (...) {\n");
    out.push_str("            return AbiError{static_cast<uint32_t>(AbiErrorCode::Panic), StringView{nullptr, 0}};\n");
    out.push_str("        }\n");
    out.push_str("    }\n\n");
}

/// Generate argument extraction for a host thunk.
fn generate_cpp_host_thunk_args(out: &mut String, func: &ResolvedFunction) {
    if func.params.len() == 1 {
        let param: &crate::ir::ResolvedParam = &func.params[0];
        let ty_name: String = cpp_host_abi_type_name(&param.ty);
        match &param.ty {
            ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => {
                out.push_str(&format!(
                    "            StringView {}_sv = *static_cast<const StringView*>(args);\n",
                    param.name
                ));
                out.push_str(&format!(
                    "            std::string_view {}(reinterpret_cast<const char*>({}_sv.ptr), {}_sv.len);\n",
                    param.name, param.name, param.name
                ));
            }
            ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => {
                out.push_str(&format!(
                    "            Buffer {}_buf = *static_cast<const Buffer*>(args);\n",
                    param.name
                ));
                out.push_str(&format!(
                    "            (void){}_buf;  // Buffer handling depends on use case\n",
                    param.name
                ));
            }
            ResolvedTypeRef::UserDefined(_) => {
                out.push_str(&format!(
                    "            const {}& {} = *static_cast<const {}*>(args);\n",
                    ty_name, param.name, ty_name
                ));
            }
            _ => {
                out.push_str(&format!(
                    "            {} {} = *static_cast<const {}*>(args);\n",
                    ty_name, param.name, ty_name
                ));
            }
        }
    } else {
        // Multiple params - use arg-pack struct
        let pack_struct: String = format!("{}Args", func.name.to_uppercase());
        out.push_str(&format!("            struct {} {{\n", pack_struct));
        for param in &func.params {
            let cpp_ty: String = cpp_host_abi_type_name(&param.ty);
            out.push_str(&format!("                {} {};\n", cpp_ty, param.name));
        }
        out.push_str("            };\n");
        out.push_str(&format!(
            "            const {}* packed = static_cast<const {}*>(args);\n",
            pack_struct, pack_struct
        ));
        // Extract each param from the packed struct
        for param in &func.params {
            match &param.ty {
                ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => {
                    out.push_str(&format!(
                        "            std::string_view {}(reinterpret_cast<const char*>(packed->{}.ptr), packed->{}.len);\n",
                        param.name, param.name, param.name
                    ));
                }
                ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => {
                    out.push_str(&format!(
                        "            Buffer {} = packed->{}; (void){};\n",
                        param.name, param.name, param.name
                    ));
                }
                _ => {
                    let cpp_ty: String = cpp_host_abi_type_name(&param.ty);
                    out.push_str(&format!(
                        "            {} {} = packed->{};\n",
                        cpp_ty, param.name, param.name
                    ));
                }
            }
        }
    }
}

/// Generate the trait method call inside a host thunk.
fn generate_cpp_host_thunk_call(out: &mut String, func: &ResolvedFunction, has_return: bool) {
    let call_args: String = if func.params.is_empty() {
        String::new()
    } else if func.params.len() == 1 {
        let param: &crate::ir::ResolvedParam = &func.params[0];
        match &param.ty {
            ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => param.name.clone(),
            ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => param.name.clone(),
            ResolvedTypeRef::UserDefined(_) => param.name.clone(),
            _ => param.name.clone(),
        }
    } else {
        func.params
            .iter()
            .map(|p| p.name.clone())
            .collect::<Vec<_>>()
            .join(", ")
    };

    if has_return {
        let ret_ty: String = func
            .returns
            .as_ref()
            .map(cpp_host_return_type_name)
            .unwrap_or_else(|| "void".to_owned());
        out.push_str(&format!(
            "            {} result = s_impl->{}({});\n",
            ret_ty, func.name, call_args
        ));
    } else {
        out.push_str(&format!(
            "            s_impl->{}({});\n",
            func.name, call_args
        ));
    }
}

/// Generate ABI type name for host thunk arguments.
fn cpp_host_abi_type_name(ty: &ResolvedTypeRef) -> String {
    match ty {
        ResolvedTypeRef::Primitive(p) => p.cpp_name().to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => "StringView".to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => "Buffer".to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::Ptr) => "void*".to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::Void) => "void".to_owned(),
        ResolvedTypeRef::UserDefined(name) => name.clone(),
    }
}

// Suppress unused import warning
const _: fn() = || {
    let _ = cpp_type_name(&ResolvedTypeRef::Primitive(PrimitiveType::U8));
};

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::ir::ReprType;
    use crate::ir::ResolvedParam;
    use crate::ir::Version;

    #[test]
    fn class_name_conversion() {
        assert_eq!(
            contract_name_to_class("image.decode"),
            "ImageDecodeContract"
        );
    }

    #[test]
    fn plugin_class_name_conversion() {
        assert_eq!(contract_name_to_plugin_class("test.add"), "TestAddPlugin");
    }

    #[test]
    fn lower_snake_conversion() {
        assert_eq!(contract_name_to_lower_snake("test.add"), "test_add");
    }

    #[test]
    fn upper_snake_conversion() {
        assert_eq!(contract_name_to_upper_snake("test.add"), "TEST_ADD");
    }

    #[test]
    fn generate_host_empty_ir() {
        let generator: CppGenerator = CppGenerator;
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
        // Now produces 3 files: types.hpp, host_callers.hpp, manifest.toml
        assert!(!files.files.is_empty());
        // At least one file contains the AUTO-GENERATED header
        assert!(files
            .files
            .iter()
            .any(|f| f.content.contains("AUTO-GENERATED")));
    }

    #[test]
    fn generate_guest_empty_ir() {
        let generator: CppGenerator = CppGenerator;
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
        // Produces 4 files (bundle=None, so no manifest): types.hpp, contracts.hpp, interfaces.hpp, init.hpp
        assert_eq!(files.files.len(), 4);
        let names: Vec<String> = files
            .files
            .iter()
            .map(|f| f.path.to_string_lossy().to_string())
            .collect();
        assert!(names.contains(&"guest/types.hpp".to_owned()));
        assert!(names.contains(&"guest/contracts.hpp".to_owned()));
        assert!(names.contains(&"guest/interfaces.hpp".to_owned()));
        assert!(names.contains(&"guest/init.hpp".to_owned()));
    }

    #[test]
    fn generate_cpp_enum_non_bitflag() {
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
        generate_cpp_enum(&mut out, &e);
        assert!(
            out.contains("enum class PixelFormat : uint32_t"),
            "missing enum class: {out}"
        );
        assert!(
            out.contains("Unknown = 0"),
            "missing Unknown variant: {out}"
        );
        assert!(
            !out.contains("operator|"),
            "non-bitflag should not have operator|: {out}"
        );
    }

    #[test]
    fn generate_cpp_enum_bitflag() {
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
        generate_cpp_enum(&mut out, &e);
        assert!(
            out.contains("enum class ImageFlags : uint32_t"),
            "missing enum class: {out}"
        );
        assert!(out.contains("operator|"), "missing operator|: {out}");
        assert!(out.contains("operator&"), "missing operator&: {out}");
        assert!(out.contains("operator~"), "missing operator~: {out}");
        assert!(
            out.contains("static_cast<uint32_t>"),
            "missing static_cast: {out}"
        );
    }

    #[test]
    fn generate_cpp_host_contract_has_factory_method() {
        use crate::ir::ResolvedContract;
        use crate::ir::ResolvedFunction;
        use crate::ir::ResolvedParam;
        use crate::ir::ResolvedTypeRef;

        let contract = ResolvedContract {
            name: "test.add".to_owned(),
            contract_id: 0x123456789ABCDEF0,
            version: Version {
                major: 1,
                minor: 0,
                patch: 0,
            },
            functions: vec![ResolvedFunction {
                name: "add".to_owned(),
                function_id: 0,
                params: vec![
                    ResolvedParam {
                        name: "a".to_owned(),
                        ty: ResolvedTypeRef::Primitive(PrimitiveType::I32),
                    },
                    ResolvedParam {
                        name: "b".to_owned(),
                        ty: ResolvedTypeRef::Primitive(PrimitiveType::I32),
                    },
                ],
                returns: Some(ResolvedTypeRef::Primitive(PrimitiveType::I32)),
            }],
        };

        let mut out: String = String::new();
        generate_cpp_host_contract(&mut out, &contract).unwrap();

        assert!(
            out.contains("static std::optional<TestAddContract> create(polyplug::Runtime& rt, uint32_t min_version = 0)"),
            "missing factory method: {out}"
        );

        assert!(
            out.contains("polyplug::PluginGuard guard_"),
            "missing guard member: {out}"
        );

        assert!(
            out.contains("bool is_valid() const noexcept"),
            "missing is_valid method: {out}"
        );
        assert!(
            out.contains("explicit operator bool() const noexcept"),
            "missing operator bool: {out}"
        );
        assert!(
            out.contains("void reset() noexcept"),
            "missing reset method: {out}"
        );

        assert!(
            out.contains("TestAddContract(TestAddContract&&) noexcept = default"),
            "missing move constructor: {out}"
        );
        assert!(
            out.contains("TestAddContract(const TestAddContract&) = delete"),
            "missing deleted copy constructor: {out}"
        );

        assert!(
            out.contains("explicit TestAddContract(polyplug::PluginGuard guard) noexcept"),
            "missing private constructor: {out}"
        );

        assert!(
            out.contains("guard_.interface()"),
            "missing guard_.interface() call: {out}"
        );
    }

    #[test]
    fn host_contract_name_to_cpp_trait_conversion() {
        assert_eq!(host_contract_name_to_cpp_trait("host.logger"), "HostLogger");
        assert_eq!(
            host_contract_name_to_cpp_trait("host.fs.reader"),
            "HostFsReader"
        );
        assert_eq!(
            host_contract_name_to_cpp_trait("host.HostLogger"),
            "HostLogger"
        );
        assert_eq!(host_contract_name_to_cpp_trait("logger"), "HostLogger");
    }

    #[test]
    fn cpp_host_param_type_name_mappings() {
        assert_eq!(
            cpp_host_param_type_name(&ResolvedTypeRef::Primitive(PrimitiveType::U32)),
            "uint32_t"
        );
        assert_eq!(
            cpp_host_param_type_name(&ResolvedTypeRef::AbiType(AbiBuiltin::StringView)),
            "StringView"
        );
        assert_eq!(
            cpp_host_param_type_name(&ResolvedTypeRef::AbiType(AbiBuiltin::Buffer)),
            "Buffer"
        );
        assert_eq!(
            cpp_host_param_type_name(&ResolvedTypeRef::UserDefined("MyStruct".to_owned())),
            "MyStruct"
        );
    }

    #[test]
    fn cpp_host_return_type_name_mappings() {
        assert_eq!(
            cpp_host_return_type_name(&ResolvedTypeRef::Primitive(PrimitiveType::U32)),
            "uint32_t"
        );
        assert_eq!(
            cpp_host_return_type_name(&ResolvedTypeRef::AbiType(AbiBuiltin::StringView)),
            "StringView"
        );
        assert_eq!(
            cpp_host_return_type_name(&ResolvedTypeRef::AbiType(AbiBuiltin::Buffer)),
            "Buffer"
        );
        assert_eq!(
            cpp_host_return_type_name(&ResolvedTypeRef::UserDefined("MyStruct".to_owned())),
            "MyStruct"
        );
    }

    #[test]
    fn generate_cpp_host_contract_trait_produces_class() {
        let contract = ResolvedHostContract {
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
        generate_cpp_host_contract_trait(&mut out, &contract);
        assert!(out.contains("class HostLogger"), "missing class: {out}");
        assert!(
            out.contains("virtual ~HostLogger() = default"),
            "missing virtual destructor: {out}"
        );
        assert!(
            out.contains("virtual void log(StringView message) = 0"),
            "missing log method: {out}"
        );
        assert!(
            out.contains("virtual void logf(uint32_t level, StringView format) = 0"),
            "missing logf method: {out}"
        );
    }

    #[test]
    fn generate_cpp_host_contracts_file_produces_file() {
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
        let out: String = generate_cpp_host_contracts_file(&ir);
        assert!(out.contains("AUTO-GENERATED"), "missing header: {out}");
        assert!(out.contains("class HostLogger"), "missing class: {out}");
        assert!(
            out.contains("HOSTLOGGER_CONTRACT_ID"),
            "missing constant: {out}"
        );
        assert!(
            out.contains("namespace polyplug_host"),
            "missing namespace: {out}"
        );
    }

    #[test]
    fn generate_host_with_host_contracts_produces_host_contracts_file() {
        let generator: CppGenerator = CppGenerator;
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
            names.contains(&"host/host_contracts.hpp".to_owned()),
            "missing host_contracts.hpp: {names:?}"
        );
    }

    #[test]
    fn generate_host_without_host_contracts_no_host_contracts_file() {
        let generator: CppGenerator = CppGenerator;
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
            !names.contains(&"host/host_contracts.hpp".to_owned()),
            "unexpected host_contracts.hpp: {names:?}"
        );
    }

    #[test]
    fn host_contract_name_to_cpp_caller_conversion() {
        assert_eq!(
            host_contract_name_to_cpp_caller("host.logger"),
            "HostLoggerContract"
        );
        assert_eq!(
            host_contract_name_to_cpp_caller("host.fs.reader"),
            "HostFsReaderContract"
        );
        assert_eq!(
            host_contract_name_to_cpp_caller("host.HostLogger"),
            "HostLoggerContract"
        );
        assert_eq!(
            host_contract_name_to_cpp_caller("logger"),
            "HostLoggerContract"
        );
    }

    #[test]
    fn cpp_guest_caller_param_type_name_mappings() {
        assert_eq!(
            cpp_guest_caller_param_type_name(&ResolvedTypeRef::Primitive(PrimitiveType::U32)),
            "uint32_t"
        );
        assert_eq!(
            cpp_guest_caller_param_type_name(&ResolvedTypeRef::AbiType(AbiBuiltin::StringView)),
            "std::string_view"
        );
        assert_eq!(
            cpp_guest_caller_param_type_name(&ResolvedTypeRef::AbiType(AbiBuiltin::Buffer)),
            "Buffer"
        );
        assert_eq!(
            cpp_guest_caller_param_type_name(&ResolvedTypeRef::UserDefined("MyStruct".to_owned())),
            "const MyStruct&"
        );
    }

    #[test]
    fn cpp_guest_caller_return_type_name_mappings() {
        assert_eq!(
            cpp_guest_caller_return_type_name(&ResolvedTypeRef::Primitive(PrimitiveType::U32)),
            "uint32_t"
        );
        assert_eq!(
            cpp_guest_caller_return_type_name(&ResolvedTypeRef::AbiType(AbiBuiltin::StringView)),
            "StringView"
        );
        assert_eq!(
            cpp_guest_caller_return_type_name(&ResolvedTypeRef::AbiType(AbiBuiltin::Buffer)),
            "Buffer"
        );
        assert_eq!(
            cpp_guest_caller_return_type_name(&ResolvedTypeRef::UserDefined("MyStruct".to_owned())),
            "MyStruct"
        );
    }

    #[test]
    fn generate_cpp_guest_host_contract_caller_produces_class() {
        let contract = ResolvedHostContract {
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
        };
        let mut out: String = String::new();
        generate_cpp_guest_host_contract_caller(&mut out, &contract);
        assert!(
            out.contains("class HostLoggerContract"),
            "missing class: {out}"
        );
        assert!(
            out.contains("const HostContractVTable* vtable_"),
            "missing vtable member: {out}"
        );
        assert!(
            out.contains("static std::optional<HostLoggerContract> from_host"),
            "missing from_host method: {out}"
        );
        assert!(
            out.contains("void log(std::string_view message) noexcept"),
            "missing log method: {out}"
        );
        assert!(
            out.contains("bool is_valid() const noexcept"),
            "missing is_valid method: {out}"
        );
    }

    #[test]
    fn generate_cpp_guest_host_contracts_file_produces_file() {
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
        let out: String = generate_cpp_guest_host_contracts_file(&ir);
        assert!(out.contains("AUTO-GENERATED"), "missing header: {out}");
        assert!(
            out.contains("class HostLoggerContract"),
            "missing class: {out}"
        );
        assert!(
            out.contains("HOSTLOGGERCONTRACT_ID"),
            "missing constant: {out}"
        );
        assert!(
            out.contains("namespace polyplug_plugin"),
            "missing namespace: {out}"
        );
    }

    #[test]
    fn generate_guest_with_host_contracts_produces_guest_host_contracts_file() {
        let generator: CppGenerator = CppGenerator;
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
            names.contains(&"guest/host_contracts.hpp".to_owned()),
            "missing guest/host_contracts.hpp: {names:?}"
        );
    }

    #[test]
    fn generate_guest_without_host_contracts_no_guest_host_contracts_file() {
        let generator: CppGenerator = CppGenerator;
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
            !names.contains(&"guest/host_contracts.hpp".to_owned()),
            "unexpected guest/host_contracts.hpp: {names:?}"
        );
    }

    #[test]
    fn generate_host_with_host_contracts_produces_interface_factories_file() {
        let generator: CppGenerator = CppGenerator;
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
            names.contains(&"host/interface_factories.hpp".to_owned()),
            "missing interface_factories.hpp: {names:?}"
        );
    }

    #[test]
    fn generate_host_without_host_contracts_no_interface_factories_file() {
        let generator: CppGenerator = CppGenerator;
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
            !names.contains(&"host/interface_factories.hpp".to_owned()),
            "unexpected interface_factories.hpp: {names:?}"
        );
    }

    #[test]
    fn generate_cpp_host_interface_factories_file_produces_file() {
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
        let out: String = generate_cpp_host_interface_factories_file(&ir);
        assert!(out.contains("AUTO-GENERATED"), "missing header: {out}");
        assert!(
            out.contains("template<typename T>"),
            "missing template: {out}"
        );
        assert!(
            out.contains("create_host_logger_interface"),
            "missing NATIVE factory: {out}"
        );
        assert!(
            out.contains("create_host_logger_interface_vm"),
            "missing VM factory: {out}"
        );
        assert!(
            out.contains("std::unique_ptr<T> impl"),
            "missing unique_ptr param: {out}"
        );
        assert!(
            out.contains("VmHostContractDispatchFn dispatch_fn"),
            "missing dispatch_fn param: {out}"
        );
        assert!(
            out.contains("namespace polyplug_host"),
            "missing namespace: {out}"
        );
    }

    #[test]
    fn generate_cpp_host_interface_factory_produces_native_and_vm_factories() {
        let contract = ResolvedHostContract {
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
        };
        let mut out: String = String::new();
        generate_cpp_host_interface_factory(&mut out, &contract);
        assert!(
            out.contains("template<typename T>"),
            "missing template: {out}"
        );
        assert!(
            out.contains("const HostContractVTable* create_host_logger_interface"),
            "missing NATIVE factory: {out}"
        );
        assert!(
            out.contains("std::unique_ptr<T> impl"),
            "missing unique_ptr: {out}"
        );
        assert!(
            out.contains("static T* s_impl = nullptr"),
            "missing static impl: {out}"
        );
        assert!(
            out.contains("HostContractFn FUNCTIONS"),
            "missing function array: {out}"
        );
        assert!(
            out.contains("host_logger_log_thunk"),
            "missing thunk: {out}"
        );
        assert!(
            out.contains("create_host_logger_interface_vm"),
            "missing VM factory: {out}"
        );
        assert!(
            out.contains("VmHostContractDispatchFn dispatch_fn"),
            "missing dispatch_fn: {out}"
        );
    }
}
