//! C++ code generator for polyplugc.
//!
//! Generates:
//! - Host-side: header-only C++ callers (RAII wrapper + interface dispatch)
//! - Guest-side: extern "C" ABI wrappers + abstract base classes + interface statics

use super::CodeGenerator;
use super::GeneratedFile;
use super::GeneratedFiles;
use super::is_native_runtime;
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

        // ── File 6: peer_callers.hpp (guest→guest peer callers) ─────────────────
        let peer_contracts: Vec<&ResolvedContract> = collect_peer_contracts(ir);
        if !peer_contracts.is_empty() {
            let peer_callers_hpp: String = generate_cpp_peer_callers_file(ir, &peer_contracts);
            files.files.push(GeneratedFile {
                path: std::path::PathBuf::from("guest/peer_callers.hpp"),
                content: peer_callers_hpp,
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
    let class_name: String = contract_name_to_guest_contract_class(&contract.name);
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
    let class_name: String = contract_name_to_guest_contract_class(&contract.name);
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
        "static GuestContractInstance {0}_create_instance_stub(const HostApi* host, const void* args) noexcept {{\n",
        plugin_upper
    ));
    out.push_str("    (void)host; (void)args;  // Unused in default stub.\n");
    out.push_str(
        "    return GuestContractInstance{nullptr, 0U};  // Null instance for stateless plugins.\n",
    );
    out.push_str("}\n\n");
    out.push_str(&format!(
        "// Default destroy_instance stub for {} - no-op.\n",
        plugin_name
    ));
    out.push_str(&format!(
        "static void {0}_destroy_instance_stub(const HostApi* host, GuestContractInstance instance) noexcept {{\n",
        plugin_upper
    ));
    out.push_str("    (void)host; (void)instance;  // Unused in default stub.\n");
    out.push_str("    // No-op - stateless plugins don't need cleanup.\n");
    out.push_str("}\n\n");

    out.push_str(&format!(
        "static GuestContractInterface {}_INTERFACE = {{\n",
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
        "    DispatchMechanisms{{ .native = NativeDispatch{{ {fn_count}U, {}_FNS }} }}\n",
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
    let class_name: String = contract_name_to_guest_contract_class(&contract.name);
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
        "static GuestContractInstance {0}_create_instance_stub(const HostApi* host, const void* args) noexcept {{\n",
        upper
    ));
    out.push_str("    (void)host; (void)args;  // Unused in default stub.\n");
    out.push_str(
        "    return GuestContractInstance{nullptr, 0U};  // Null instance for stateless plugins.\n",
    );
    out.push_str("}\n\n");
    out.push_str(&format!(
        "// Default destroy_instance stub for {} - no-op.\n",
        contract.name
    ));
    out.push_str(&format!(
        "static void {0}_destroy_instance_stub(const HostApi* host, GuestContractInstance instance) noexcept {{\n",
        upper
    ));
    out.push_str("    (void)host; (void)instance;  // Unused in default stub.\n");
    out.push_str("    // No-op - stateless plugins don't need cleanup.\n");
    out.push_str("}\n\n");

    // Interface static
    out.push_str(&format!(
        "static GuestContractInterface {}_INTERFACE = {{\n",
        upper
    ));
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
        "    DispatchMechanisms{{ .native = NativeDispatch{{ {}U, {}_FNS }} }}\n",
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
    out.push_str(
        "    // For stateful plugins, users override create_instance and use instance.data.\n",
    );
    out.push_str("    (void)instance;  // Suppress unused warning for stateless plugins.\n");
    out.push_str("    try {\n");

    if has_params {
        out.push_str("        if (args == nullptr) {\n");
        out.push_str(
            "            return AbiError{static_cast<uint32_t>(AbiErrorCode::InvalidPointer), StringView{nullptr, 0}};\n",
        );
        out.push_str("        }\n");
    }
    if !is_void_return {
        out.push_str("        if (out == nullptr) {\n");
        out.push_str(
            "            return AbiError{static_cast<uint32_t>(AbiErrorCode::InvalidPointer), StringView{nullptr, 0}};\n",
        );
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

    out.push_str("    } catch (const std::exception&) {\n");
    out.push_str(
        "        // The AbiError message must outlive this stack frame; the host never frees it.\n",
    );
    out.push_str(
        "        // e.what() points into the (about-to-be-destroyed) exception object, so we\n",
    );
    out.push_str("        // return a static literal instead of a dangling pointer.\n");
    out.push_str(
        "        // SAFETY: err_msg is a static constexpr string literal with known length 26.\n",
    );
    out.push_str(
        "        static constexpr const char* err_msg = \"guest threw std::exception\";\n",
    );
    out.push_str("        return AbiError{static_cast<uint32_t>(AbiErrorCode::Generic), StringView{reinterpret_cast<const uint8_t*>(err_msg), 26}};\n");
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
    // SAFETY comments for generated code are required per CLAUDE.md rule 6 for all unsafe operations
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
                    contract_name_to_guest_contract_class(&contract.name)
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
            let class_name: String = contract_name_to_guest_contract_class(&contract.name);
            out.push_str(&format!("{}* g_{}_impl = nullptr;\n", class_name, lower));
        }
    }
    out.push_str("\n}  // namespace polyplug_plugin\n\n");

    // polyplug_abi_version
    out.push_str("extern \"C\" uint32_t polyplug_abi_version() { return 1U; }\n\n");

    // polyplug_init
    out.push_str("extern \"C\" AbiError polyplug_init(const HostApi* host, const BundleInitContext* ctx) {\n");
    out.push_str("    if (!host || !ctx) {\n");
    out.push_str(
        "        static constexpr const char* err_msg = \"null parameter in polyplug_init\";\n",
    );
    out.push_str(
        "        return AbiError{static_cast<uint32_t>(AbiErrorCode::Generic), StringView{reinterpret_cast<const uint8_t*>(err_msg), 32}};\n",
    );
    out.push_str("    }\n\n");
    out.push_str(
        "    // Store host interface for later access via polyplug::get_host_interface()\n",
    );
    out.push_str("    polyplug::store_host_interface(host);\n\n");

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
                "    AbiError err_{upper} = host->register_guest_contract(host, &desc_{upper}, &polyplug_plugin::{upper}_INTERFACE);\n",
                upper = plugin_upper
            ));
            out.push_str(&format!(
                "    if (err_{}.code != static_cast<uint32_t>(AbiErrorCode::Ok)) return err_{};\n\n",
                plugin_upper, plugin_upper
            ));
        }
    } else {
        for contract in &ir.contracts {
            generate_init_hpp_register_guest_contract(&mut out, contract)?;
        }
    }

    out.push_str(
        "    return AbiError{static_cast<uint32_t>(AbiErrorCode::Ok), StringView{nullptr, 0}};\n",
    );
    out.push_str("}\n\n");

    Ok(out)
}

fn generate_init_hpp_register_guest_contract(
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
        "    AbiError err_{upper} = host->register_guest_contract(host, &desc_{upper}, &polyplug_plugin::{upper}_INTERFACE);\n",
        upper = upper
    ));
    out.push_str(&format!(
        "    if (err_{}.code != static_cast<uint32_t>(AbiErrorCode::Ok)) return err_{};\n\n",
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
    out.push_str("#include <array>\n");
    out.push_str("#include <cstddef>\n");
    out.push_str("#include <memory>\n");
    out.push_str("#include <optional>\n\n");
    out.push_str("namespace polyplug_generated {\n\n");
    if let Some(ref bundle) = ir.bundle {
        out.push_str(&format!(
            "static constexpr uint64_t MY_BUNDLE_ID = {}ULL;\n\n",
            bundle.bundle_id
        ));
    }

    // Emit the per-caller call-arena helpers only when some contract needs one.
    if ir.contracts.iter().any(contract_needs_arena) {
        emit_cpp_call_arena_helpers(&mut out);
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
    let dep_tables: String = super::emit_manifest_dependencies(&bundle.dependencies);

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

// ─── Call-arena support ────────────────────────────────────────────────────────

/// Whether a function returns a variable-size value the guest writes into the
/// call arena (a `StringView`, a `Buffer`, or any user-defined struct that may
/// embed one). Such functions receive a per-caller `CallArena`; all others pass
/// a null arena and the VM bridge falls back to per-value `host->alloc`.
///
/// Passing an arena where none is needed is harmless; passing null where one is
/// needed only loses the optimisation. The conservative `UserDefined` case keeps
/// the rule sound without resolving struct fields here. Mirrors `fn_needs_arena`
/// in `rust.rs` so every generator agrees on which functions are arena-backed.
fn fn_needs_arena(func: &ResolvedFunction) -> bool {
    matches!(
        &func.returns,
        Some(ResolvedTypeRef::AbiType(AbiBuiltin::StringView))
            | Some(ResolvedTypeRef::AbiType(AbiBuiltin::Buffer))
            | Some(ResolvedTypeRef::UserDefined(_))
    )
}

/// Whether any function on the contract needs a call arena.
fn contract_needs_arena(contract: &ResolvedContract) -> bool {
    contract.functions.iter().any(fn_needs_arena)
}

/// Emit the inline call-arena helpers used by per-caller arenas.
///
/// `CallArena` in `polyplug/abi.hpp` is a layout-only POD (no methods), so the
/// bump/overflow allocation and reset logic is emitted here. It is a direct port
/// of `polyplug_abi::CallArena::{alloc, reset}`; keeping the two in lockstep is
/// required by Rule 10 (identical ABI mechanisms across generators).
fn emit_cpp_call_arena_helpers(out: &mut String) {
    out.push_str("/// Size of each caller's inline call-arena buffer.\n");
    out.push_str("///\n");
    out.push_str("/// Variable-size VM return values (strings, buffers) are bump-allocated from\n");
    out.push_str("/// this buffer; outputs larger than it spill into host-allocated overflow\n");
    out.push_str("/// blocks that are retained across resets and freed only at teardown.\n");
    out.push_str("static constexpr size_t CALL_ARENA_BUF_LEN = 512;\n\n");

    out.push_str("/// Minimum size of a host-allocated overflow block, including its header.\n");
    out.push_str("static constexpr size_t POLYPLUG_OVERFLOW_BLOCK_MIN = 4096;\n");
    out.push_str("/// Alignment used for host-allocated overflow blocks.\n");
    out.push_str(
        "static constexpr size_t POLYPLUG_OVERFLOW_BLOCK_ALIGN = alignof(ArenaOverflowBlock);\n\n",
    );

    out.push_str("/// Bump-allocate `size` bytes aligned to `align` within `[from, end)`.\n");
    out.push_str("/// Returns nullptr if the request does not fit.\n");
    out.push_str(
        "inline uint8_t* polyplug_arena_bump(uint8_t* from, uint8_t* end, size_t size, size_t align) noexcept {\n",
    );
    out.push_str("    auto addr = reinterpret_cast<size_t>(from);\n");
    out.push_str("    size_t aligned = (addr + (align - 1)) & ~(align - 1);\n");
    out.push_str("    if (aligned < addr) { return nullptr; }  // overflow on alignment\n");
    out.push_str("    size_t new_cur = aligned + size;\n");
    out.push_str("    if (new_cur < aligned) { return nullptr; }  // overflow on size\n");
    out.push_str("    if (new_cur <= reinterpret_cast<size_t>(end)) {\n");
    out.push_str("        return reinterpret_cast<uint8_t*>(aligned);\n");
    out.push_str("    }\n");
    out.push_str("    return nullptr;\n");
    out.push_str("}\n\n");

    out.push_str("/// Try to bump-allocate `size`@`align` from `block`'s free region.\n");
    out.push_str("///\n");
    out.push_str("/// Advances `block->used` on success and returns the allocation pointer.\n");
    out.push_str(
        "/// Returns nullptr if the request does not fit in the block's remaining room.\n",
    );
    out.push_str(
        "inline uint8_t* polyplug_arena_serve_from_block(ArenaOverflowBlock* block, size_t size, size_t align) noexcept {\n",
    );
    out.push_str(
        "    // SAFETY: block is a valid overflow block previously allocated by polyplug_arena_alloc;\n",
    );
    out.push_str(
        "    // reading used/capacity and deriving pointers from the block base stays within\n",
    );
    out.push_str("    // the capacity-byte allocation.\n");
    out.push_str("    auto block_bytes = reinterpret_cast<uint8_t*>(block);\n");
    out.push_str("    uint8_t* from = block_bytes + block->used;\n");
    out.push_str("    uint8_t* end  = block_bytes + block->capacity;\n");
    out.push_str("    uint8_t* p = polyplug_arena_bump(from, end, size, align);\n");
    out.push_str("    if (p == nullptr) { return nullptr; }\n");
    out.push_str(
        "    // SAFETY: block is a valid chain node; writing used (a plain size_t field)\n",
    );
    out.push_str(
        "    // is in-bounds because the block was allocated with at least sizeof(ArenaOverflowBlock) bytes.\n",
    );
    out.push_str("    block->used = static_cast<size_t>(p - block_bytes) + size;\n");
    out.push_str("    return p;\n");
    out.push_str("}\n\n");

    out.push_str("/// Allocate `size` bytes aligned to `align` from `arena`.\n");
    out.push_str("///\n");
    out.push_str("/// Serves from the primary region by bumping `cur`; on exhaustion, walks the\n");
    out.push_str(
        "/// retained overflow chain for a block with spare room; if none fits, requests a\n",
    );
    out.push_str("/// fresh overflow block from the host and serves from it. Returns nullptr if\n");
    out.push_str(
        "/// `size == 0`, if `align` is not a power of two, or if a host allocation fails.\n",
    );
    out.push_str("/// The returned pointer is valid until the next polyplug_arena_reset().\n");
    out.push_str(
        "inline uint8_t* polyplug_arena_alloc(CallArena* arena, size_t size, size_t align) noexcept {\n",
    );
    out.push_str("    if (size == 0 || align == 0 || (align & (align - 1)) != 0) {\n");
    out.push_str("        return nullptr;\n");
    out.push_str("    }\n");
    out.push_str(
        "    if (uint8_t* p = polyplug_arena_bump(arena->cur, arena->end, size, align)) {\n",
    );
    out.push_str("        arena->cur = p + size;\n");
    out.push_str("        return p;\n");
    out.push_str("    }\n");
    out.push_str("    if (arena->host == nullptr) { return nullptr; }\n");
    out.push_str(
        "    // REUSE PASS: walk the retained chain; serve from the first block with room.\n",
    );
    out.push_str(
        "    for (ArenaOverflowBlock* b = arena->first_overflow; b != nullptr; b = b->next) {\n",
    );
    out.push_str(
        "        if (uint8_t* p = polyplug_arena_serve_from_block(b, size, align)) { return p; }\n",
    );
    out.push_str("    }\n");
    out.push_str("    // ALLOCATE NEW: no retained block had enough room.\n");
    out.push_str("    size_t header = sizeof(ArenaOverflowBlock);\n");
    out.push_str("    size_t needed = header + align + size;\n");
    out.push_str("    size_t capacity = needed > POLYPLUG_OVERFLOW_BLOCK_MIN ? needed : POLYPLUG_OVERFLOW_BLOCK_MIN;\n");
    out.push_str(
        "    // SAFETY: arena->host is non-null (checked above) and valid for the arena's\n",
    );
    out.push_str(
        "    // lifetime. The allocator returns a block of `capacity` bytes or nullptr.\n",
    );
    out.push_str(
        "    auto block_ptr = static_cast<uint8_t*>(arena->host->alloc(arena->host, capacity, POLYPLUG_OVERFLOW_BLOCK_ALIGN));\n",
    );
    out.push_str("    if (block_ptr == nullptr) { return nullptr; }\n");
    out.push_str("    // SAFETY: block_ptr is aligned for ArenaOverflowBlock and owns at least\n");
    out.push_str("    // `capacity >= header` bytes, so writing the header is sound.\n");
    out.push_str("    auto block = reinterpret_cast<ArenaOverflowBlock*>(block_ptr);\n");
    out.push_str("    block->next = arena->first_overflow;\n");
    out.push_str("    block->capacity = capacity;\n");
    out.push_str("    block->used = header;\n");
    out.push_str("    arena->first_overflow = block;\n");
    out.push_str("    return polyplug_arena_serve_from_block(block, size, align);\n");
    out.push_str("}\n\n");

    out.push_str(
        "/// Rewind `arena` for reuse: the primary region and every retained overflow block\n",
    );
    out.push_str(
        "/// become available again. Overflow blocks are NOT freed — they are retained for\n",
    );
    out.push_str(
        "/// reuse across calls; call polyplug_arena_free_all() at teardown to free them.\n",
    );
    out.push_str(
        "/// After reset, all pointers previously returned by polyplug_arena_alloc are invalid.\n",
    );
    out.push_str("inline void polyplug_arena_reset(CallArena* arena) noexcept {\n");
    out.push_str("    arena->cur = arena->base;\n");
    out.push_str("    ArenaOverflowBlock* block = arena->first_overflow;\n");
    out.push_str("    while (block != nullptr) {\n");
    out.push_str(
        "        // SAFETY: every block in the chain was allocated by polyplug_arena_alloc\n",
    );
    out.push_str("        // with a valid header; reading next and writing used are in-bounds.\n");
    out.push_str("        block->used = sizeof(ArenaOverflowBlock);\n");
    out.push_str("        block = block->next;\n");
    out.push_str("    }\n");
    out.push_str("}\n\n");

    out.push_str("/// Free all retained overflow blocks and reset the overflow chain to empty.\n");
    out.push_str("/// Call this at teardown (destructor) to release all host-allocated memory.\n");
    out.push_str("inline void polyplug_arena_free_all(CallArena* arena) noexcept {\n");
    out.push_str("    ArenaOverflowBlock* block = arena->first_overflow;\n");
    out.push_str("    while (block != nullptr) {\n");
    out.push_str(
        "        // SAFETY: every block was allocated by polyplug_arena_alloc with a valid\n",
    );
    out.push_str("        // header; reading next/capacity before freeing is sound.\n");
    out.push_str("        ArenaOverflowBlock* next = block->next;\n");
    out.push_str("        size_t capacity = block->capacity;\n");
    out.push_str("        if (arena->host != nullptr) {\n");
    out.push_str(
        "            // SAFETY: block was allocated by host->alloc with these exact args.\n",
    );
    out.push_str(
        "            arena->host->free(arena->host, reinterpret_cast<uint8_t*>(block), capacity, POLYPLUG_OVERFLOW_BLOCK_ALIGN);\n",
    );
    out.push_str("        }\n");
    out.push_str("        block = next;\n");
    out.push_str("    }\n");
    out.push_str("    arena->first_overflow = nullptr;\n");
    out.push_str("}\n\n");

    out.push_str(
        "/// Construct a CallArena over `buf` (primary region) with `host` for overflow.\n",
    );
    out.push_str("inline CallArena polyplug_arena_new(uint8_t* buf, size_t len, const HostApi* host) noexcept {\n");
    out.push_str("    CallArena arena{};\n");
    out.push_str("    arena.cur = buf;\n");
    out.push_str("    arena.end = buf + len;\n");
    out.push_str("    arena.base = buf;\n");
    out.push_str("    arena.host = host;\n");
    out.push_str("    arena.first_overflow = nullptr;\n");
    out.push_str("    return arena;\n");
    out.push_str("}\n\n");
}

// ─── Per-contract class emitter (instance wrapper) ──────────────────────────────

fn generate_cpp_host_contract(
    out: &mut String,
    contract: &ResolvedContract,
) -> Result<(), PolyplugcError> {
    let class_name: String = contract_name_to_class(&contract.name);
    let _contract_upper: String = contract.name.to_uppercase().replace(['.', '-'], "_");
    let needs_arena: bool = contract_needs_arena(contract);

    out.push_str(&format!(
        "/// Host caller for contract `{}` (id=0x{:016X})\n",
        contract.name, contract.contract_id
    ));
    out.push_str("///\n");
    out.push_str("/// RAII wrapper that manages instance lifecycle:\n");
    out.push_str("/// - `create()`: resolves handle and calls `create_instance`\n");
    out.push_str("/// - destructor: calls `destroy_instance` to clean up\n");
    out.push_str("/// - dispatch: passes `instance_` to all method calls\n");
    if needs_arena {
        out.push_str("///\n");
        out.push_str("/// # Call-arena lifetime\n");
        out.push_str("///\n");
        out.push_str(
            "/// Methods returning variable-size values (`StringView`, `Buffer`, or structs\n",
        );
        out.push_str(
            "/// that may embed one) are non-const and reset this caller's arena at the start\n",
        );
        out.push_str(
            "/// of the call. Any view returned by such a method borrows arena memory and is\n",
        );
        out.push_str("/// valid only until the next arena-backed call on the same caller.\n");
    }
    out.push_str(&format!("class {} {{\npublic:\n", class_name));

    // Factory method: resolve handle + create_instance
    out.push_str("    /// Factory method - creates instance or nullopt if not found.\n");
    out.push_str("    /// Calls `create_instance` on the resolved interface.\n");
    out.push_str("    ///\n");
    out.push_str("    /// # Arguments\n");
    out.push_str("    /// - `handle`: Contract handle from `find_guest_contract`\n");
    out.push_str("    /// - `host`: Host interface pointer\n");
    out.push_str("    ///\n");
    out.push_str("    /// # Returns\n");
    out.push_str("    /// - `std::optional<Self>` if interface found and instance created\n");
    out.push_str("    /// - `std::nullopt` if interface not found or `create_instance` failed\n");
    out.push_str(&format!(
        "    static std::optional<{}> create(GuestContractHandle handle, const HostApi* host) noexcept {{\n",
        class_name
    ));
    out.push_str("        if (host == nullptr) {\n");
    out.push_str("            return std::nullopt;\n");
    out.push_str("        }\n");
    out.push_str("        // Resolve the interface from the handle via HostApi method.\n");
    out.push_str("        const GuestContractInterface* iface = host->resolve_guest_contract(host, handle);\n");
    out.push_str("        if (iface == nullptr) {\n");
    out.push_str("            return std::nullopt;\n");
    out.push_str("        }\n");
    out.push_str("        // Create instance via factory function.\n");
    out.push_str("        // A null `instance.data` is valid: stateless contracts return a null\n");
    out.push_str(
        "        // handle from `create_instance` and use it as an opaque dispatch token.\n",
    );
    out.push_str(
        "        GuestContractInstance instance = iface->create_instance(host, nullptr);\n",
    );
    out.push_str(&format!(
        "        return {}(iface, instance, host);\n",
        class_name
    ));
    out.push_str("    }\n\n");

    // Destructor: calls destroy_instance
    out.push_str("    /// Destructor - calls `destroy_instance` to clean up.\n");
    out.push_str(&format!("    ~{}() noexcept {{\n", class_name));
    if needs_arena {
        out.push_str(
            "        // Free any overflow blocks the arena still holds before destruction.\n",
        );
        out.push_str("        // arena_buf_ is null only on a moved-from caller.\n");
        out.push_str("        if (arena_buf_) {\n");
        out.push_str("            polyplug_arena_free_all(&arena_);\n");
        out.push_str("        }\n");
    }
    out.push_str("        // Destroy instance via factory\n");
    out.push_str("        // SAFETY: instance was created by create_instance and is valid.\n");
    out.push_str("        if (instance_.data != nullptr) {\n");
    out.push_str("            interface_->destroy_instance(host_, instance_);\n");
    out.push_str("            instance_.data = nullptr;  // Prevent reuse after cleanup.\n");
    out.push_str("        }\n");
    out.push_str("    }\n\n");

    // Move-only (instance handles are unique)
    out.push_str("    // Move-only (instance handles are unique)\n");
    out.push_str(&format!(
        "    {}({}&& other) noexcept\n",
        class_name, class_name
    ));
    out.push_str("        : interface_(other.interface_),\n");
    out.push_str("          instance_(other.instance_),\n");
    if needs_arena {
        // The arena's interior pointers refer into *arena_buf_, a heap block whose
        // address is preserved by moving the unique_ptr, so the arena stays valid.
        out.push_str("          host_(other.host_),\n");
        out.push_str("          arena_buf_(std::move(other.arena_buf_)),\n");
        out.push_str("          arena_(other.arena_) {\n");
    } else {
        out.push_str("          host_(other.host_) {\n");
    }
    out.push_str("        other.instance_.data = nullptr;  // Prevent double-destroy.\n");
    out.push_str("    }\n");
    out.push_str(&format!(
        "    {}& operator=({}&& other) noexcept {{\n",
        class_name, class_name
    ));
    out.push_str("        if (this != &other) {\n");
    if needs_arena {
        out.push_str("            // Release this caller's overflow blocks before overwriting.\n");
        out.push_str("            if (arena_buf_) {\n");
        out.push_str("                polyplug_arena_free_all(&arena_);\n");
        out.push_str("            }\n");
    }
    out.push_str("            // Destroy current instance first\n");
    out.push_str("            if (instance_.data != nullptr) {\n");
    out.push_str("                interface_->destroy_instance(host_, instance_);\n");
    out.push_str("            }\n");
    out.push_str("            interface_ = other.interface_; instance_ = other.instance_; host_ = other.host_; other.instance_.data = nullptr;\n");
    if needs_arena {
        out.push_str(
            "            arena_buf_ = std::move(other.arena_buf_); arena_ = other.arena_;\n",
        );
    }
    out.push_str("        }\n");
    out.push_str("        return *this;\n");
    out.push_str("    }\n");
    out.push_str(&format!(
        "    {}(const {}&) = delete;\n",
        class_name, class_name
    ));
    out.push_str(&format!(
        "    {}& operator=(const {}&) = delete;\n\n",
        class_name, class_name
    ));

    out.push_str("    /// Check if this caller holds a resolved contract interface.\n");
    out.push_str("    /// Keys off the interface pointer, not `instance_.data`: stateless\n");
    out.push_str(
        "    /// contracts legitimately use a null instance as an opaque dispatch token.\n",
    );
    out.push_str(
        "    explicit operator bool() const noexcept { return interface_ != nullptr; }\n\n",
    );
    out.push_str("    /// Check if this caller holds a resolved contract interface.\n");
    out.push_str("    bool is_valid() const noexcept { return interface_ != nullptr; }\n\n");

    // reset() method: destroy and recreate
    out.push_str("    /// Destroy current instance and create a new one.\n");
    out.push_str("    /// Useful for recovering from plugin errors.\n");
    out.push_str("    void reset() noexcept {\n");
    out.push_str("        if (instance_.data != nullptr) {\n");
    out.push_str("            interface_->destroy_instance(host_, instance_);\n");
    out.push_str("        }\n");
    out.push_str("        instance_ = interface_->create_instance(host_, nullptr);\n");
    out.push_str("    }\n\n");

    // Generate method callers
    for func in &contract.functions {
        generate_cpp_host_function(out, &class_name, func)?;
    }

    // Private members and constructor
    out.push_str("private:\n");
    out.push_str("    /// Resolved interface pointer from the registry.\n");
    out.push_str("    const GuestContractInterface* interface_;\n");
    out.push_str("    /// Instance handle created by `create_instance`.\n");
    out.push_str("    GuestContractInstance instance_;\n");
    out.push_str("    /// Host interface pointer (needed for create/destroy_instance).\n");
    out.push_str("    const HostApi* host_;\n");
    if needs_arena {
        out.push_str(
            "    /// Stable-address backing buffer for the per-call arena. Held by unique_ptr\n",
        );
        out.push_str("    /// so the arena's interior pointers survive moving the caller value.\n");
        out.push_str("    std::unique_ptr<std::array<uint8_t, CALL_ARENA_BUF_LEN>> arena_buf_;\n");
        out.push_str(
            "    /// Per-call bump arena over `arena_buf_`, reset at each arena-backed call.\n",
        );
        out.push_str("    CallArena arena_;\n");
    }
    out.push('\n');
    if needs_arena {
        out.push_str(&format!(
            "    explicit {}(const GuestContractInterface* iface, GuestContractInstance inst, const HostApi* host)\n",
            class_name
        ));
        out.push_str("        : interface_(iface), instance_(inst), host_(host),\n");
        out.push_str(
            "          arena_buf_(std::make_unique<std::array<uint8_t, CALL_ARENA_BUF_LEN>>()),\n",
        );
        out.push_str(
            "          arena_(polyplug_arena_new(arena_buf_->data(), CALL_ARENA_BUF_LEN, host)) {}\n",
        );
    } else {
        out.push_str(&format!(
            "    explicit {}(const GuestContractInterface* iface, GuestContractInstance inst, const HostApi* host) noexcept\n",
            class_name
        ));
        out.push_str("        : interface_(iface), instance_(inst), host_(host) {}\n");
    }
    out.push_str("};\n\n");
    Ok(())
}

// ─── Per-function method emitter (instance-based dispatch) ─────────────────────

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
        "    /// Call `{}` (function_id={})\n",
        func.name, func.function_id
    ));
    let needs_arena: bool = fn_needs_arena(func);
    if needs_arena {
        out.push_str(
            "    /// Returns a value borrowing this caller's arena; it stays valid until\n",
        );
        out.push_str("    /// the next arena-backed call on this caller.\n");
    }
    out.push_str(&format!(
        "    {} {}({}) {{\n",
        return_type, func.name, params_str
    ));

    if needs_arena {
        out.push_str(
            "        // Reset the arena at call start: frees the previous call's overflow\n",
        );
        out.push_str(
            "        // blocks and rewinds the primary region, invalidating prior views.\n",
        );
        out.push_str("        polyplug_arena_reset(&arena_);\n");
    }

    // Determine args_ptr expression.
    let args_ptr_code: String = build_args_ptr_code(class_name, func);
    out.push_str(&args_ptr_code);

    let fn_id: u32 = func.function_id;

    let is_void_return: bool = matches!(
        func.returns.as_ref(),
        None | Some(ResolvedTypeRef::AbiType(AbiBuiltin::Void))
    );

    // SAFETY: Check interface validity
    out.push_str("        // SAFETY: interface_ is valid for the lifetime of this wrapper.\n");
    out.push_str("        if (!interface_) {\n");
    out.push_str("            static constexpr const char* err_msg = \"interface is null\";\n");
    out.push_str("            polyplug::check_abi_error(AbiError{static_cast<uint32_t>(AbiErrorCode::InvalidPointer), StringView{reinterpret_cast<const uint8_t*>(err_msg), 16}});\n");
    out.push_str("        }\n");

    let out_ptr_expr: &str = if is_void_return {
        out.push_str("        void* out_ptr = nullptr;\n");
        "out_ptr"
    } else {
        out.push_str(&format!("        {} out{{}};\n", return_type));
        out.push_str("        void* out_ptr = &out;\n");
        "out_ptr"
    };

    // Function-id bounds check against the native dispatch table. The function
    // count lives in `dispatch.native.function_count` — there is no top-level
    // `function_count` field on GuestContractInterface.
    out.push_str(&format!(
        "        if ({}U >= interface_->dispatch.native.function_count) {{\n",
        fn_id
    ));
    out.push_str("            static constexpr const char* err_msg = \"function not available in interface\";\n");
    out.push_str("            polyplug::check_abi_error(AbiError{static_cast<uint32_t>(AbiErrorCode::FunctionNotAvailable), StringView{reinterpret_cast<const uint8_t*>(err_msg), 32}});\n");
    out.push_str("        }\n");

    // Dispatch via the resolved interface, branching on its dispatch type so
    // native and VM-backed guests are both supported (ABI parity with rust.rs).
    out.push_str("        AbiError err{};\n");
    out.push_str("        switch (interface_->dispatch_type) {\n");
    out.push_str("            case DispatchType::Native: {\n");
    out.push_str(&format!(
        "                auto fn_ = reinterpret_cast<AbiError(*)(GuestContractInstance, const void*, void*)>(interface_->dispatch.native.functions[{}U]);\n",
        fn_id
    ));
    out.push_str("                // SAFETY: instance_ is the token returned by create_instance and is valid.\n");
    out.push_str("                // args_ptr/out_ptr match the ABI contract for this function.\n");
    out.push_str(&format!(
        "                err = fn_(instance_, args_ptr, {});\n",
        out_ptr_expr
    ));
    out.push_str("                break;\n");
    out.push_str("            }\n");
    out.push_str("            case DispatchType::VirtualMachine: {\n");
    // Arena-backed functions hand the guest this caller's per-call arena so it can
    // write variable-size returns without a per-value host->alloc; other functions
    // pass nullptr and the VM bridge falls back to per-value host allocation.
    let arena_arg: &str = if needs_arena { "&arena_" } else { "nullptr" };
    out.push_str(&format!(
        "                err = (interface_->dispatch.vm.call)(interface_->dispatch.vm.loader_data, instance_, {}U, args_ptr, {}, {});\n",
        fn_id, out_ptr_expr, arena_arg
    ));
    out.push_str("                break;\n");
    out.push_str("            }\n");
    out.push_str("        }\n");
    out.push_str("        polyplug::check_abi_error(err);\n");
    if !is_void_return {
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
fn contract_name_to_guest_contract_class(name: &str) -> String {
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
        + "GuestContract"
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
/// Returns ergonomic borrowed-view types for host-owned memory:
/// - StringView -> std::string_view (borrowed view into host-owned memory)
/// - Buffer -> std::span<const std::uint8_t> (borrowed view into host-owned memory)
/// - UserDefined -> TypeName (by value)
/// - Primitives -> T (by value)
fn cpp_guest_caller_return_type_name(ty: &ResolvedTypeRef) -> String {
    match ty {
        ResolvedTypeRef::Primitive(p) => p.cpp_name().to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => "std::string_view".to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => "std::span<const std::uint8_t>".to_owned(),
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
    out.push_str("    /// Factory method - creates caller from HostApi or nullopt if not found.\n");
    out.push_str(&format!(
        "    static std::optional<{}> from_host(const HostApi* host, uint32_t min_version = 0) noexcept {{\n",
        class_name
    ));
    out.push_str("        if (host == nullptr) {\n");
    out.push_str("            return std::nullopt;\n");
    out.push_str("        }\n");
    // Get the instance first
    out.push_str(&format!(
        "        HostContractInstance instance = host->get_host_contract(host, 0x{:016X}ULL, min_version);\n",
        contract.contract_id
    ));
    out.push_str("        if (instance.data == nullptr) {\n");
    out.push_str("            return std::nullopt;\n");
    out.push_str("        }\n");
    // Resolve the interface for dispatch metadata
    out.push_str(&format!(
        "        const HostContractInterface* interface = host->resolve_host_contract_interface(host, 0x{:016X}ULL, min_version);\n",
        contract.contract_id
    ));
    out.push_str("        if (interface == nullptr) {\n");
    out.push_str("            return std::nullopt;\n");
    out.push_str("        }\n");
    out.push_str(&format!(
        "        return {}(interface, instance);\n",
        class_name
    ));
    out.push_str("    }\n\n");

    // is_valid method
    out.push_str("    /// Check if caller is valid (interface and instance are non-null).\n");
    out.push_str("    bool is_valid() const noexcept { return interface_ != nullptr && instance_.data != nullptr; }\n\n");

    // Explicit bool conversion
    out.push_str("    /// Explicit bool conversion for validity check.\n");
    out.push_str("    explicit operator bool() const noexcept { return interface_ != nullptr && instance_.data != nullptr; }\n\n");

    // Methods for each function
    for func in &contract.functions {
        generate_cpp_guest_host_contract_method(out, func, &class_name);
    }

    // Private section
    out.push_str("private:\n");
    out.push_str(&format!(
        "    explicit {}(const HostContractInterface* interface, HostContractInstance instance) noexcept\n",
        class_name
    ));
    out.push_str("        : interface_(interface), instance_(instance) {}\n\n");
    out.push_str("    const HostContractInterface* interface_;\n");
    out.push_str("    HostContractInstance instance_;\n");
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

    // Null interface check
    out.push_str("        if (interface_ == nullptr) {\n");
    if func.returns.is_some() {
        out.push_str(&format!("            return {}{{}};\n", return_type));
    } else {
        out.push_str("            return;\n");
    }
    out.push_str("        }\n\n");

    // Get function count directly from interface (no header wrapper)
    out.push_str(&format!(
        "        if ({fn_id}U >= interface_->dispatch.native.function_count) {{\n"
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
    out.push_str("        switch (interface_->dispatch_type) {\n");
    out.push_str("            case DispatchType::Native: {\n");
    out.push_str(&format!(
        "                auto fn_ = reinterpret_cast<AbiError(*)(HostContractInstance, const void*, void*)>(interface_->dispatch.native.functions[{fn_id}U]);\n"
    ));
    out.push_str("                err = fn_(instance_, args_ptr, out_ptr);\n");
    out.push_str("                break;\n");
    out.push_str("            }\n");
    out.push_str("            case DispatchType::VirtualMachine: {\n");
    // The vm.call expects a GuestContractInstance; a host contract has no guest
    // instance, so pass a null one (matches rust.rs). The host-contract instance
    // is conveyed to the native thunk via the Native branch, not the VM bridge.
    out.push_str(&format!(
        "                err = (interface_->dispatch.vm.call)(interface_->dispatch.vm.loader_data, GuestContractInstance{{}}, {fn_id}U, args_ptr, out_ptr, nullptr);\n"
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
    if let Some(ret_ty) = &func.returns {
        let expr: String = cpp_guest_caller_return_expr(ret_ty);
        out.push_str(&format!("        return {};\n", expr));
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

/// Get the raw ABI type name for the `out` local in a guest caller method.
/// This is always the ABI struct (StringView/Buffer), never the ergonomic view type,
/// because the host writes into this local via the void* out_ptr.
fn cpp_guest_caller_out_local_type_name(ty: &ResolvedTypeRef) -> String {
    match ty {
        ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => "StringView".to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => "Buffer".to_owned(),
        _ => cpp_guest_caller_return_type_name(ty),
    }
}

/// Build the return expression for a guest caller method given the filled `out` local.
/// For StringView/Buffer, constructs a borrowed view into host-owned memory.
/// For all other types, returns `out` directly.
fn cpp_guest_caller_return_expr(ty: &ResolvedTypeRef) -> String {
    match ty {
        ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => {
            // Borrowed view into host-owned memory, valid until the next call on this caller.
            // A null/empty StringView (ptr=null, len=0) is a legal ABI return; constructing a
            // std::string_view from a null pointer is UB before C++26, so route through the
            // SDK's null-safe polyplug::to_string_view helper.
            "polyplug::to_string_view(out)".to_owned()
        }
        ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => {
            // Borrowed view into host-owned memory, valid until the next call on this caller.
            // A null/empty Buffer (ptr=null, len=0) is a legal ABI return; guard the null
            // pointer explicitly rather than constructing a span from it.
            "out.ptr ? std::span<const std::uint8_t>(out.ptr, out.len) : std::span<const std::uint8_t>{}".to_owned()
        }
        _ => "out".to_owned(),
    }
}

/// Emit the out_ptr setup for a C++ guest host contract method.
/// The `out` local is always the raw ABI type so the host can write into it via void*.
fn emit_cpp_guest_host_contract_out_setup(out: &mut String, returns: &Option<ResolvedTypeRef>) {
    if let Some(ret_ty) = returns {
        let abi_ty: String = cpp_guest_caller_out_local_type_name(ret_ty);
        out.push_str(&format!("        {} out{{}};\n", abi_ty));
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
    out.push_str("#include <span>\n");
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

// ─── Host Interface Factories Generation ─────────────────────────────────────────

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
    let patch: u32 = contract.version.patch;
    let singleton: bool = contract.singleton;

    // NATIVE dispatch factory
    out.push_str(&format!(
        "/// Create a host contract interface for `{}` with NATIVE dispatch.\n",
        contract.name
    ));
    out.push_str("///\n");
    out.push_str("/// Takes ownership of the implementation and creates a 'static interface.\n");
    out.push_str("/// The implementation must inherit from the abstract class.\n");
    out.push_str("///\n");
    out.push_str("/// # Memory\n");
    out.push_str("/// The returned interface pointer is valid for the lifetime of the program.\n");
    out.push_str("/// The implementation unique_ptr is released and managed internally.\n");
    out.push_str("template<typename T>\n");
    out.push_str(&format!(
        "const HostContractInterface* {}(std::unique_ptr<T> impl) noexcept {{\n",
        factory_name
    ));
    out.push_str("    T* impl_ptr = impl.release();\n\n");

    // Generate thunks for each function
    for func in &contract.functions {
        generate_cpp_host_thunk(out, func, &contract.name, &trait_name);
    }

    // Static function pointer array. NativeDispatch::functions is `void* const*`,
    // so the thunk lambdas are stored as type-erased `void*` (matching the guest
    // interfaces.hpp pattern) and reinterpreted at the dispatch site.
    out.push_str(&format!(
        "    static void* const FUNCTIONS[{}] = {{\n",
        fn_count
    ));
    for func in &contract.functions {
        let thunk_name: String = format!(
            "{}_{}_thunk",
            contract.name.replace('.', "_").to_lowercase(),
            func.name
        );
        out.push_str(&format!(
            "        reinterpret_cast<void*>({}),\n",
            thunk_name
        ));
    }
    out.push_str("    };\n\n");

    // create_instance stub for host-side factory (captureless lambda → fn ptr).
    out.push_str("    // create_instance stub - host owns the singleton instance lifecycle\n");
    out.push_str(
        "    static constexpr HostContractInterface_create_instance_fn create_instance_stub =\n",
    );
    out.push_str(
        "        +[](const HostContractInterface* self, const void* /*args*/) noexcept -> HostContractInstance {\n",
    );
    out.push_str("        // Return the registrant-owned user_data as the instance; the thunks\n");
    out.push_str("        // recover the implementation from it (no static state).\n");
    out.push_str("        return HostContractInstance{self->user_data};\n");
    out.push_str("    };\n\n");

    // destroy_instance stub for host-side factory (captureless lambda → fn ptr).
    out.push_str("    // destroy_instance stub - host owns the singleton instance lifecycle\n");
    out.push_str(
        "    static constexpr HostContractInterface_destroy_instance_fn destroy_instance_stub =\n",
    );
    out.push_str("        +[](const HostContractInterface* /*this*/, HostContractInstance /*instance*/) noexcept -> void {\n");
    if singleton {
        out.push_str(
            "        // Singleton: no-op, the implementation lives for program lifetime\n",
        );
    } else {
        out.push_str(
            "        // Multi-instance: not supported in host-side factory, use custom factory\n",
        );
    }
    out.push_str("    };\n\n");

    // Static interface with inline fields (matches HostContractInterface ABI layout)
    out.push_str("    static HostContractInterface s_interface = {\n");
    out.push_str(&format!(
        "        0x{contract_id:016X}ULL,  // contract_id\n"
    ));
    out.push_str(&format!(
        "        Version{{{major}U, {minor}U, {patch}U}},  // contract_version\n"
    ));
    out.push_str(&format!("        {},  // singleton\n", singleton));
    out.push_str("        DispatchType::Native,  // dispatch_type\n");
    out.push_str("        nullptr,  // runtime (set by polyplug during registration)\n");
    out.push_str("        nullptr,  // user_data (set below to the registrant-owned impl)\n");
    out.push_str("        create_instance_stub,  // create_instance\n");
    out.push_str("        destroy_instance_stub,  // destroy_instance\n");
    out.push_str("        DispatchMechanisms{ .native = NativeDispatch{\n");
    out.push_str(&format!("            {fn_count}U,  // function_count\n"));
    out.push_str("            FUNCTIONS,  // functions\n");
    out.push_str("        } },  // dispatch.native\n");
    out.push_str("    };  // dispatch\n\n");
    out.push_str(
        "    // Route the implementation through user_data; create_instance reads it via `this`.\n",
    );
    out.push_str("    s_interface.user_data = static_cast<void*>(impl_ptr);\n");
    out.push_str("    return &s_interface;\n");
    out.push_str("}\n\n");

    // VM dispatch factory
    out.push_str(&format!(
        "/// Create a host contract interface for `{}` with VM dispatch.\n",
        contract.name
    ));
    out.push_str("///\n");
    out.push_str("/// Used when the host implementation is in a VM language (Python, Lua, JS).\n");
    out.push_str("///\n");
    out.push_str("/// # Arguments\n");
    out.push_str("/// * `loader_data` - Opaque pointer to VM-specific data\n");
    out.push_str("/// * `dispatch_fn` - Function to call for each contract function\n");
    out.push_str("///\n");
    out.push_str("/// # Memory\n");
    out.push_str("/// The returned interface pointer is valid for the lifetime of the program.\n");
    out.push_str(&format!(
        "const HostContractInterface* {}(\n",
        factory_vm_name
    ));
    out.push_str("    void* loader_data,\n");
    out.push_str("    VmDispatch_call_fn dispatch_fn\n");
    out.push_str(") noexcept {\n");

    // create_instance stub for VM factory (captureless lambda → fn ptr).
    out.push_str("    // create_instance stub - VM loader owns instance lifecycle\n");
    out.push_str(
        "    static constexpr HostContractInterface_create_instance_fn vm_create_instance_stub =\n",
    );
    out.push_str(
        "        +[](const HostContractInterface* /*this*/, const void* /*args*/) noexcept -> HostContractInstance {\n",
    );
    out.push_str("        // VM dispatch: instance managed by VM loader, return placeholder\n");
    out.push_str("        return HostContractInstance{nullptr};\n");
    out.push_str("    };\n\n");

    // destroy_instance stub for VM factory (captureless lambda → fn ptr).
    out.push_str("    // destroy_instance stub - VM loader owns instance lifecycle\n");
    out.push_str("    static constexpr HostContractInterface_destroy_instance_fn vm_destroy_instance_stub =\n");
    out.push_str("        +[](const HostContractInterface* /*this*/, HostContractInstance /*instance*/) noexcept -> void {\n");
    out.push_str("        // VM dispatch: instance managed by VM loader, no-op here\n");
    out.push_str("    };\n\n");

    // Static interface with inline fields (matches HostContractInterface ABI layout)
    out.push_str("    static HostContractInterface s_interface = {\n");
    out.push_str(&format!(
        "        0x{contract_id:016X}ULL,  // contract_id\n"
    ));
    out.push_str(&format!(
        "        Version{{{major}U, {minor}U, {patch}U}},  // contract_version\n"
    ));
    out.push_str(&format!("        {},  // singleton\n", singleton));
    out.push_str("        DispatchType::VirtualMachine,  // dispatch_type\n");
    out.push_str("        nullptr,  // runtime (set by polyplug during registration)\n");
    out.push_str("        loader_data,  // user_data (registrant-owned VM bridge data)\n");
    out.push_str("        vm_create_instance_stub,  // create_instance\n");
    out.push_str("        vm_destroy_instance_stub,  // destroy_instance\n");
    // `dispatch` is a union; the VM variant is not the first member, so it must
    // be set via a designated initializer. `loader_data` is a VmLoaderData.
    out.push_str("        DispatchMechanisms{ .vm = VmDispatch{\n");
    out.push_str("            dispatch_fn,  // call\n");
    out.push_str("            VmLoaderData{loader_data},  // loader_data\n");
    out.push_str("        } },  // dispatch.vm\n");
    out.push_str("    };  // dispatch\n");
    out.push_str("    return &s_interface;\n");
    out.push_str("}\n\n");
}

/// Generate a thunk function for a host contract function.
fn generate_cpp_host_thunk(
    out: &mut String,
    func: &ResolvedFunction,
    contract_name: &str,
    trait_name: &str,
) {
    let thunk_name: String = format!(
        "{}_{}_thunk",
        contract_name.replace('.', "_").to_lowercase(),
        func.name
    );
    let has_return: bool = func.returns.is_some();

    // Emit the thunk as a captureless lambda. C++ forbids defining a named
    // function inside another function body, but a captureless lambda decays to
    // a plain function pointer. The implementation is recovered from the instance
    // token, which create_instance sets from the interface's user_data (no static).
    out.push_str(&format!(
        "    static constexpr auto {} = +[](HostContractInstance instance, const void* args, void* out) noexcept -> AbiError {{\n",
        thunk_name
    ));
    out.push_str(&format!(
        "        auto* impl = static_cast<{}*>(instance.data);\n",
        trait_name
    ));
    out.push_str("        if (impl == nullptr) {\n");
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
    out.push_str("    };\n\n");
}

/// Generate argument extraction for a host thunk.
fn generate_cpp_host_thunk_args(out: &mut String, func: &ResolvedFunction) {
    if func.params.len() == 1 {
        let param: &crate::ir::ResolvedParam = &func.params[0];
        let ty_name: String = cpp_host_abi_type_name(&param.ty);
        match &param.ty {
            ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => {
                // Pass the raw StringView through to the impl: the generated
                // abstract method takes StringView (rule 9 — UTF-8 boundary).
                out.push_str(&format!(
                    "            StringView {} = *static_cast<const StringView*>(args);\n",
                    param.name
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
                    // Pass the raw StringView through to the impl (rule 9).
                    out.push_str(&format!(
                        "            StringView {} = packed->{};\n",
                        param.name, param.name
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
            "            {} result = impl->{}({});\n",
            ret_ty, func.name, call_args
        ));
    } else {
        out.push_str(&format!(
            "            impl->{}({});\n",
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

// ─── Peer caller collection helpers ──────────────────────────────────────────

/// Collect contracts from `ir.contracts` whose `contract_id` matches any entry
/// in the bundle's declared dependencies.  Returns an empty vec when there is
/// no bundle or no matching dependency.
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

/// Return the `min_version` (major) for the dependency matching `target_contract_id`.
/// Returns 0 if no matching dependency is found.
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

// ─── Peer callers file generator ─────────────────────────────────────────────

/// Generate the full `guest/peer_callers.hpp` file for all peer contracts.
fn generate_cpp_peer_callers_file(ir: &ValidatedIr, peers: &[&ResolvedContract]) -> String {
    let mut out: String = String::new();
    out.push_str("// THIS FILE IS AUTO-GENERATED BY polyplugc. DO NOT EDIT.\n");
    out.push_str(
        "// Re-generate with: polyplugc generate --bundle bundle.toml --lang cpp --out <dir>\n",
    );
    out.push_str("#pragma once\n");
    out.push_str("#include \"types.hpp\"\n");
    out.push_str("#include \"polyplug/guest.hpp\"\n");
    out.push_str("#include \"polyplug/abi.hpp\"\n");
    out.push_str("#include <array>\n");
    out.push_str("#include <cstdint>\n");
    out.push_str("#include <memory>\n");
    out.push_str("#include <optional>\n\n");
    out.push_str("namespace polyplug_plugin {\n\n");
    out.push_str("using namespace polyplug_generated;\n\n");

    // Emit arena helpers only when at least one peer contract needs the arena.
    let any_needs_arena: bool = peers
        .iter()
        .any(|c: &&ResolvedContract| contract_needs_arena(c));
    if any_needs_arena {
        emit_cpp_call_arena_helpers(&mut out);
    }

    for contract in peers {
        let min_ver: u32 = peer_min_version(ir, contract.contract_id);
        generate_cpp_peer_caller(&mut out, contract, min_ver);
    }

    out.push_str("}  // namespace polyplug_plugin\n");
    out
}

/// Generate one `…Peer` class for a single peer guest contract.
///
/// The class mirrors the host caller (`generate_cpp_host_contract`) with two
/// differences:
/// 1. The factory (`resolve()`) takes no arguments — it fetches the `HostApi*`
///    from the cpp guest SDK via `polyplug::get_host_interface()`, then calls
///    `find_guest_contract` / `resolve_guest_contract` / `create_instance`.
/// 2. Per-method dispatch uses `host->call_guest_method` (the host-mediated
///    path at HostApi offset 136) instead of indexing the native dispatch table
///    directly.
fn generate_cpp_peer_caller(out: &mut String, contract: &ResolvedContract, min_version: u32) {
    let class_name: String = format!("{}Peer", contract_name_to_class(&contract.name));
    let needs_arena: bool = contract_needs_arena(contract);

    out.push_str(&format!(
        "/// Peer caller for guest contract `{}` (id=0x{:016X})\n",
        contract.name, contract.contract_id
    ));
    out.push_str("///\n");
    out.push_str(
        "/// Dispatches to the peer through the host-mediated `call_guest_method` path.\n",
    );
    out.push_str("/// Use `resolve()` to obtain an instance; returns `std::nullopt` when the\n");
    out.push_str("/// contract is not registered or the host interface is unavailable.\n");
    if needs_arena {
        out.push_str("///\n");
        out.push_str("/// # Call-arena lifetime\n");
        out.push_str("///\n");
        out.push_str(
            "/// Methods returning variable-size values (`StringView`, `Buffer`, or structs\n",
        );
        out.push_str(
            "/// that may embed one) are non-const and reset this caller's arena at the start\n",
        );
        out.push_str(
            "/// of the call. Any view returned by such a method borrows arena memory and is\n",
        );
        out.push_str("/// valid only until the next arena-backed call on the same caller.\n");
    }
    out.push_str(&format!("class {} {{\npublic:\n", class_name));

    // ── resolve() factory ───────────────────────────────────────────────────
    out.push_str("    /// Discover and resolve the peer contract through the host.\n");
    out.push_str("    ///\n");
    out.push_str(
        "    /// Returns `std::nullopt` if the host interface is unavailable, the contract\n",
    );
    out.push_str("    /// is not registered, or `resolve_guest_contract` returns null.\n");
    out.push_str(&format!(
        "    static std::optional<{}> resolve() noexcept {{\n",
        class_name
    ));
    out.push_str("        const HostApi* host = polyplug::get_host_interface();\n");
    out.push_str("        if (host == nullptr) {\n");
    out.push_str("            return std::nullopt;\n");
    out.push_str("        }\n");
    out.push_str(&format!(
        "        GuestContractHandle handle = host->find_guest_contract(host, 0x{:016X}ULL, {}U);\n",
        contract.contract_id, min_version
    ));
    out.push_str("        // Pass the handle straight to resolve_guest_contract; it rejects\n");
    out.push_str("        // stale/invalid handles and returns nullptr — do not inspect fields.\n");
    out.push_str(
        "        const GuestContractInterface* iface = host->resolve_guest_contract(host, handle);\n",
    );
    out.push_str("        if (iface == nullptr) {\n");
    out.push_str("            return std::nullopt;\n");
    out.push_str("        }\n");
    out.push_str("        // A null `instance.data` is valid: stateless contracts return a null\n");
    out.push_str(
        "        // handle from `create_instance` and use it as an opaque dispatch token.\n",
    );
    out.push_str(
        "        GuestContractInstance instance = iface->create_instance(host, nullptr);\n",
    );
    out.push_str(
        "        // Stamp the peer contract id so `host->call_guest_method` routes by it\n",
    );
    out.push_str(
        "        // even when a stateless peer's create_instance returns a null (null-id)\n",
    );
    out.push_str("        // handle. The host-mediated path keys routing on contract_id.\n");
    out.push_str(&format!(
        "        instance.contract_id = 0x{:016X}ULL;\n",
        contract.contract_id
    ));
    out.push_str(&format!(
        "        return {}(iface, instance, host);\n",
        class_name
    ));
    out.push_str("    }\n\n");

    // ── Destructor ──────────────────────────────────────────────────────────
    out.push_str("    /// Destructor - calls `destroy_instance` to clean up.\n");
    out.push_str(&format!("    ~{}() noexcept {{\n", class_name));
    if needs_arena {
        out.push_str(
            "        // Free any overflow blocks the arena still holds before destruction.\n",
        );
        out.push_str("        // arena_buf_ is null only on a moved-from caller.\n");
        out.push_str("        if (arena_buf_) {\n");
        out.push_str("            polyplug_arena_reset(&arena_);\n");
        out.push_str("        }\n");
    }
    out.push_str(
        "        // SAFETY: instance was created by create_instance on the resolved interface.\n",
    );
    out.push_str("        // We guard on instance.data to skip the call for stateless (null-data) contracts.\n");
    out.push_str("        if (instance_.data != nullptr) {\n");
    out.push_str("            iface_->destroy_instance(host_, instance_);\n");
    out.push_str("            instance_.data = nullptr;  // Prevent reuse after cleanup.\n");
    out.push_str("        }\n");
    out.push_str("    }\n\n");

    // ── Move-only ───────────────────────────────────────────────────────────
    out.push_str("    // Move-only (instance handles are unique)\n");
    out.push_str(&format!(
        "    {}({}&& other) noexcept\n",
        class_name, class_name
    ));
    out.push_str("        : iface_(other.iface_),\n");
    out.push_str("          instance_(other.instance_),\n");
    if needs_arena {
        out.push_str("          host_(other.host_),\n");
        out.push_str("          arena_buf_(std::move(other.arena_buf_)),\n");
        out.push_str("          arena_(other.arena_) {\n");
    } else {
        out.push_str("          host_(other.host_) {\n");
    }
    out.push_str("        other.instance_.data = nullptr;  // Prevent double-destroy.\n");
    out.push_str("    }\n");
    out.push_str(&format!(
        "    {}& operator=({}&& other) noexcept {{\n",
        class_name, class_name
    ));
    out.push_str("        if (this != &other) {\n");
    if needs_arena {
        out.push_str("            if (arena_buf_) {\n");
        out.push_str("                polyplug_arena_reset(&arena_);\n");
        out.push_str("            }\n");
    }
    out.push_str("            if (instance_.data != nullptr) {\n");
    out.push_str("                iface_->destroy_instance(host_, instance_);\n");
    out.push_str("            }\n");
    out.push_str(
        "            iface_ = other.iface_; instance_ = other.instance_; host_ = other.host_; other.instance_.data = nullptr;\n",
    );
    if needs_arena {
        out.push_str(
            "            arena_buf_ = std::move(other.arena_buf_); arena_ = other.arena_;\n",
        );
    }
    out.push_str("        }\n");
    out.push_str("        return *this;\n");
    out.push_str("    }\n");
    out.push_str(&format!(
        "    {}(const {}&) = delete;\n",
        class_name, class_name
    ));
    out.push_str(&format!(
        "    {}& operator=(const {}&) = delete;\n\n",
        class_name, class_name
    ));

    // ── Validity checks ─────────────────────────────────────────────────────
    out.push_str("    /// Check if this peer holds a resolved (non-null) interface.\n");
    out.push_str("    /// Keys off the interface pointer, not `instance_.data`: stateless\n");
    out.push_str(
        "    /// contracts legitimately use a null instance as an opaque dispatch token.\n",
    );
    out.push_str("    explicit operator bool() const noexcept { return iface_ != nullptr; }\n\n");
    out.push_str("    /// Check if this peer holds a resolved (non-null) interface.\n");
    out.push_str("    bool is_valid() const noexcept { return iface_ != nullptr; }\n\n");

    // ── Per-method peer callers ─────────────────────────────────────────────
    for func in &contract.functions {
        generate_cpp_peer_fn_caller(out, &class_name, func);
    }

    // ── Private members and constructor ─────────────────────────────────────
    out.push_str("private:\n");
    out.push_str("    /// Resolved interface pointer for the peer contract.\n");
    out.push_str("    const GuestContractInterface* iface_;\n");
    out.push_str("    /// Instance handle created from the peer interface.\n");
    out.push_str("    GuestContractInstance instance_;\n");
    out.push_str(
        "    /// Host interface pointer used for `call_guest_method` and instance lifecycle.\n",
    );
    out.push_str("    const HostApi* host_;\n");
    if needs_arena {
        out.push_str(
            "    /// Stable-address backing buffer for the per-call arena. Held by unique_ptr\n",
        );
        out.push_str("    /// so the arena's interior pointers survive moving the caller value.\n");
        out.push_str("    std::unique_ptr<std::array<uint8_t, CALL_ARENA_BUF_LEN>> arena_buf_;\n");
        out.push_str(
            "    /// Per-call bump arena over `arena_buf_`, reset at each arena-backed call.\n",
        );
        out.push_str("    CallArena arena_;\n");
    }
    out.push('\n');
    if needs_arena {
        out.push_str(&format!(
            "    explicit {}(const GuestContractInterface* iface, GuestContractInstance inst, const HostApi* host)\n",
            class_name
        ));
        out.push_str("        : iface_(iface), instance_(inst), host_(host),\n");
        out.push_str(
            "          arena_buf_(std::make_unique<std::array<uint8_t, CALL_ARENA_BUF_LEN>>()),\n",
        );
        out.push_str(
            "          arena_(polyplug_arena_new(arena_buf_->data(), CALL_ARENA_BUF_LEN, host)) {}\n",
        );
    } else {
        out.push_str(&format!(
            "    explicit {}(const GuestContractInterface* iface, GuestContractInstance inst, const HostApi* host) noexcept\n",
            class_name
        ));
        out.push_str("        : iface_(iface), instance_(inst), host_(host) {}\n");
    }
    out.push_str("};\n\n");
}

/// Generate one per-method peer caller body.
///
/// Uses `host->call_guest_method` (host-mediated path at HostApi offset 136).
/// Marshalling is identical to the host caller (`generate_cpp_host_function`):
/// raw ABI types (StringView/Buffer) are returned; variable-size returns reset
/// and pass the caller's arena; the dispatch switch is omitted because
/// `call_guest_method` already handles both Native and VirtualMachine dispatch.
fn generate_cpp_peer_fn_caller(out: &mut String, class_name: &str, func: &ResolvedFunction) {
    let fn_id: u32 = func.function_id;
    let needs_arena: bool = fn_needs_arena(func);

    let return_type: String = func
        .returns
        .as_ref()
        .map(cpp_type_name)
        .unwrap_or_else(|| "void".to_owned());

    let params: Vec<String> = func
        .params
        .iter()
        .map(|p| format!("{} {}", cpp_type_name(&p.ty), p.name))
        .collect();
    let params_str: String = params.join(", ");

    let self_qual: &str = if needs_arena { "" } else { " const" };

    out.push_str(&format!(
        "    /// Call peer function `{}` (function_id={}) via host-mediated dispatch.\n",
        func.name, fn_id
    ));
    if needs_arena {
        out.push_str(
            "    /// Returns a value borrowing this caller's arena; it stays valid until\n",
        );
        out.push_str("    /// the next arena-backed call on this caller.\n");
    }
    out.push_str(&format!(
        "    {} {}({}){} {{\n",
        return_type, func.name, params_str, self_qual
    ));

    // Interface null-guard.
    out.push_str("        if (iface_ == nullptr) {\n");
    if func.returns.is_some() {
        out.push_str(&format!("            return {}{{}};\n", return_type));
    } else {
        out.push_str("            return;\n");
    }
    out.push_str("        }\n");

    if needs_arena {
        out.push_str(
            "        // Reset the arena at call start: frees the previous call's overflow\n",
        );
        out.push_str(
            "        // blocks and rewinds the primary region, invalidating prior views.\n",
        );
        out.push_str("        polyplug_arena_reset(&arena_);\n");
    }

    // args_ptr — reuse the same helper as the host caller.
    let args_ptr_code: String = build_args_ptr_code(class_name, func);
    out.push_str(&args_ptr_code);

    // out_ptr
    let is_void_return: bool = matches!(
        func.returns.as_ref(),
        None | Some(ResolvedTypeRef::AbiType(AbiBuiltin::Void))
    );
    if is_void_return {
        out.push_str("        void* out_ptr = nullptr;\n");
    } else {
        out.push_str(&format!("        {} out{{}};\n", return_type));
        out.push_str("        void* out_ptr = &out;\n");
    }

    // Dispatch via call_guest_method (host-mediated, handles both Native and VM).
    let arena_arg: &str = if needs_arena { "&arena_" } else { "nullptr" };
    out.push_str("        // SAFETY: host_ is non-null (set in resolve()); iface_ and instance_\n");
    out.push_str("        // are valid for the lifetime of this wrapper. args_ptr/out_ptr match\n");
    out.push_str("        // the ABI contract for this function.\n");
    out.push_str(&format!(
        "        AbiError err = host_->call_guest_method(host_, instance_, {}U, args_ptr, out_ptr, {});\n",
        fn_id, arena_arg
    ));

    // Error handling — return default on ABI error (mirrors host caller's check_abi_error path
    // but for guest-side peer callers we cannot throw, so we return a zero-initialised value).
    out.push_str("        if (err.code != static_cast<uint32_t>(AbiErrorCode::Ok)) {\n");
    if func.returns.is_some() {
        out.push_str(&format!("            return {}{{}};\n", return_type));
    } else {
        out.push_str("            return;\n");
    }
    out.push_str("        }\n");

    if !is_void_return {
        out.push_str("        return out;\n");
    }

    out.push_str("    }\n\n");
}

// ─── Tests ───────────────────────────────────────────────────────────────────

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
        assert_eq!(
            contract_name_to_guest_contract_class("test.add"),
            "TestAddGuestContract"
        );
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
        assert!(
            files
                .files
                .iter()
                .any(|f| f.content.contains("AUTO-GENERATED"))
        );
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

        // Check for instance-based factory method. The handle is the typed
        // GuestContractHandle (a u32 index struct), never a raw u64.
        assert!(
            out.contains("static std::optional<TestAddContract> create(GuestContractHandle handle, const HostApi* host) noexcept"),
            "missing factory method: {out}"
        );

        // Validity must key off the resolved interface pointer, not instance_.data:
        // stateless contracts legitimately return a null instance handle.
        assert!(
            out.contains("return interface_ != nullptr;"),
            "validity check must key off interface_, not instance_.data: {out}"
        );

        // Function-id bounds check must read the native dispatch table's count
        // (there is no top-level function_count field on GuestContractInterface).
        assert!(
            out.contains("interface_->dispatch.native.function_count"),
            "bounds check must use dispatch.native.function_count: {out}"
        );

        // Dispatch must branch on the interface's dispatch type for ABI parity.
        assert!(
            out.contains("switch (interface_->dispatch_type)"),
            "dispatch must branch on dispatch_type: {out}"
        );

        // Check for instance member (not PluginGuard)
        assert!(
            out.contains("GuestContractInstance instance_"),
            "missing instance member: {out}"
        );

        // Check for interface and host members
        assert!(
            out.contains("const GuestContractInterface* interface_"),
            "missing interface member: {out}"
        );
        assert!(
            out.contains("const HostApi* host_"),
            "missing host member: {out}"
        );

        // Check lifecycle methods
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

        // Check destructor calls destroy_instance
        assert!(
            out.contains("~TestAddContract() noexcept"),
            "missing destructor: {out}"
        );
        assert!(
            out.contains("interface_->destroy_instance(host_, instance_)"),
            "missing destroy_instance call in destructor: {out}"
        );

        // Check factory calls create_instance
        assert!(
            out.contains("iface->create_instance(host, nullptr)"),
            "missing create_instance call in factory: {out}"
        );

        // Check move constructor (not default, explicit transfer)
        assert!(
            out.contains("TestAddContract(TestAddContract&& other) noexcept"),
            "missing move constructor: {out}"
        );
        assert!(
            out.contains("other.instance_.data = nullptr"),
            "missing nulling of moved-from instance: {out}"
        );
        assert!(
            out.contains("TestAddContract(const TestAddContract&) = delete"),
            "missing deleted copy constructor: {out}"
        );

        // Check private constructor
        assert!(
            out.contains("explicit TestAddContract(const GuestContractInterface* iface, GuestContractInstance inst, const HostApi* host) noexcept"),
            "missing private constructor: {out}"
        );

        // Check dispatch uses instance_
        assert!(
            out.contains("fn_(instance_, args_ptr,"),
            "missing instance_ in dispatch call: {out}"
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
            "std::string_view"
        );
        assert_eq!(
            cpp_guest_caller_return_type_name(&ResolvedTypeRef::AbiType(AbiBuiltin::Buffer)),
            "std::span<const std::uint8_t>"
        );
        assert_eq!(
            cpp_guest_caller_return_type_name(&ResolvedTypeRef::UserDefined("MyStruct".to_owned())),
            "MyStruct"
        );
    }

    #[test]
    fn cpp_guest_caller_return_expr_is_null_safe() {
        // A null/empty StringView/Buffer return is legal at the ABI boundary; constructing a
        // std::string_view from a null pointer is UB before C++26, so the emitted return must
        // route through the SDK's null-safe to_string_view, and the span must guard the pointer.
        let sv_expr: String =
            cpp_guest_caller_return_expr(&ResolvedTypeRef::AbiType(AbiBuiltin::StringView));
        assert_eq!(sv_expr, "polyplug::to_string_view(out)");
        assert!(
            !sv_expr.contains("std::string_view(reinterpret_cast"),
            "StringView return must not build string_view from raw ptr (UB on null): {sv_expr}"
        );

        let buf_expr: String =
            cpp_guest_caller_return_expr(&ResolvedTypeRef::AbiType(AbiBuiltin::Buffer));
        assert!(
            buf_expr.contains("out.ptr ?"),
            "Buffer return must guard the null pointer before building a span: {buf_expr}"
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
            out.contains("const HostContractInterface* interface_"),
            "missing interface member: {out}"
        );
        assert!(
            out.contains("HostContractInstance instance_"),
            "missing instance member: {out}"
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
            out.contains("VmDispatch_call_fn dispatch_fn"),
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
            out.contains("const HostContractInterface* create_host_logger_interface"),
            "missing NATIVE factory: {out}"
        );
        assert!(
            out.contains("std::unique_ptr<T> impl"),
            "missing unique_ptr: {out}"
        );
        assert!(
            out.contains("T* impl_ptr = impl.release();"),
            "implementation must be released into a local, not a static: {out}"
        );
        assert!(
            out.contains("s_interface.user_data = static_cast<void*>(impl_ptr);"),
            "implementation must be routed through user_data, not a static: {out}"
        );
        assert!(
            !out.contains("s_impl"),
            "host factory must not hold the implementation in a static: {out}"
        );
        assert!(
            out.contains("static void* const FUNCTIONS"),
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
            out.contains("VmDispatch_call_fn dispatch_fn"),
            "missing dispatch_fn: {out}"
        );
        assert!(
            out.contains("create_instance_stub"),
            "missing create_instance stub: {out}"
        );
        assert!(
            out.contains("destroy_instance_stub"),
            "missing destroy_instance stub: {out}"
        );
    }

    /// A declared [[dependency]] whose contract_id matches a contract in ir.contracts
    /// must produce a PeerCallers class in guest/peer_callers.hpp.
    #[test]
    fn peer_caller_emitted_for_declared_dependency() {
        use crate::ir::ResolvedBundle;
        use crate::ir::ResolvedPlugin;

        let contract_id: u64 = 0xDEAD_BEEF_1234_5678_u64;
        let ir: ValidatedIr = ValidatedIr {
            types: vec![],
            enums: vec![],
            contracts: vec![ResolvedContract {
                name: "pipeline.Validator".to_owned(),
                contract_id,
                version: Version {
                    major: 1,
                    minor: 0,
                    patch: 0,
                },
                functions: vec![ResolvedFunction {
                    name: "validate".to_owned(),
                    function_id: 0,
                    params: vec![ResolvedParam {
                        name: "input".to_owned(),
                        ty: ResolvedTypeRef::AbiType(AbiBuiltin::StringView),
                    }],
                    returns: Some(ResolvedTypeRef::AbiType(AbiBuiltin::StringView)),
                }],
            }],
            host_contracts: vec![],
            bundle: Some(ResolvedBundle {
                name: "cpp_transformer".to_owned(),
                bundle_id: 0xAAAA_BBBB_CCCC_DDDD_u64,
                version: Version {
                    major: 1,
                    minor: 0,
                    patch: 0,
                },
                runtime: "native".to_owned(),
                file: polyplug_codegen::ResolvedBundleFile::Single("test.so".to_owned()),
                plugins: vec![ResolvedPlugin {
                    name: "transformer".to_owned(),
                    version: Version {
                        major: 1,
                        minor: 0,
                        patch: 0,
                    },
                    implements: vec!["data.Transformer@1".to_owned()],
                    optional: vec![],
                }],
                dependencies: vec![ResolvedDependency::ByContract {
                    contract: "pipeline.Validator".to_owned(),
                    contract_id,
                    min_version: 1,
                }],
                needs_reinit_on_dep_reload: false,
            }),
        };

        let generator: CppGenerator = CppGenerator;
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
            names.contains(&"guest/peer_callers.hpp".to_owned()),
            "expected guest/peer_callers.hpp, got: {names:?}"
        );

        let peer_file: &GeneratedFile = files
            .files
            .iter()
            .find(|f: &&GeneratedFile| f.path.to_string_lossy() == "guest/peer_callers.hpp")
            .expect("peer_callers.hpp must be present");

        assert!(
            peer_file.content.contains("PipelineValidatorContractPeer"),
            "expected peer class name in output:\n{}",
            peer_file.content
        );
        assert!(
            peer_file.content.contains("call_guest_method"),
            "peer caller must use call_guest_method:\n{}",
            peer_file.content
        );
        assert!(
            peer_file.content.contains("find_guest_contract"),
            "resolve() must call find_guest_contract:\n{}",
            peer_file.content
        );
        assert!(
            peer_file.content.contains("resolve_guest_contract"),
            "resolve() must call resolve_guest_contract:\n{}",
            peer_file.content
        );
        assert!(
            peer_file.content.contains("polyplug::get_host_interface()"),
            "resolve() must use polyplug::get_host_interface():\n{}",
            peer_file.content
        );
        assert!(
            peer_file.content.contains("AUTO-GENERATED"),
            "missing AUTO-GENERATED header:\n{}",
            peer_file.content
        );
    }

    /// Without a bundle (or without a matching dependency), no peer_callers.hpp
    /// must be emitted.
    #[test]
    fn no_peer_callers_without_dependencies() {
        let generator: CppGenerator = CppGenerator;
        let ir: ValidatedIr = ValidatedIr {
            types: vec![],
            enums: vec![],
            contracts: vec![ResolvedContract {
                name: "pipeline.Validator".to_owned(),
                contract_id: 0xDEAD_BEEF_1234_5678_u64,
                version: Version {
                    major: 1,
                    minor: 0,
                    patch: 0,
                },
                functions: vec![],
            }],
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
            !names.contains(&"guest/peer_callers.hpp".to_owned()),
            "must NOT emit peer_callers.hpp without a bundle: {names:?}"
        );
    }
}
