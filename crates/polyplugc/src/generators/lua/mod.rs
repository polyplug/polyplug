use std::path::PathBuf;

use crate::error::CodegenError;
use crate::generators::CodeGenerator;
use crate::generators::GeneratedFile;
use crate::generators::GeneratedFiles;
use crate::ir::AbiBuiltin;
use crate::ir::PrimitiveType;
use crate::ir::ResolvedContract;
use crate::ir::ResolvedFunction;
use crate::ir::ResolvedParam;
use crate::ir::ResolvedType;
use crate::ir::ResolvedTypeRef;
use crate::ir::ValidatedIr;

pub(crate) struct LuaGenerator;

impl CodeGenerator for LuaGenerator {
    fn language_name(&self) -> &'static str {
        "lua"
    }

    fn generate_host(
        &self,
        ir: &ValidatedIr,
        files: &mut GeneratedFiles,
    ) -> Result<(), CodegenError> {
        let types_lua: String = generate_lua_types_file(ir);
        let callers_lua: String = generate_host_callers_file(ir);

        files.files.push(GeneratedFile {
            path: PathBuf::from("host/types.lua"),
            content: types_lua,
        });
        files.files.push(GeneratedFile {
            path: PathBuf::from("host/callers.lua"),
            content: callers_lua,
        });

        Ok(())
    }

    fn generate_guest(
        &self,
        ir: &ValidatedIr,
        files: &mut GeneratedFiles,
    ) -> Result<(), CodegenError> {
        let types_lua: String = generate_lua_types_file(ir);
        let contracts_lua: String = generate_guest_contracts_file(ir);

        files.files.push(GeneratedFile {
            path: PathBuf::from("guest/types.lua"),
            content: types_lua,
        });
        files.files.push(GeneratedFile {
            path: PathBuf::from("guest/contracts.lua"),
            content: contracts_lua,
        });

        Ok(())
    }
}

fn generate_lua_types_file(ir: &ValidatedIr) -> String {
    let mut out: String = String::new();
    out.push_str(file_header());
    out.push_str("local ffi = require(\"ffi\")\n\n");
    out.push_str(cdef_guarded_block());
    out.push_str("cdef_guarded([[\n");
    for ty in &ir.types {
        generate_lua_user_type(&mut out, ty);
        out.push('\n');
    }
    for contract in &ir.contracts {
        let contract_struct: String = contract_name_to_struct(&contract.name);
        for func in &contract.functions {
            if needs_arg_pack(&func.params) {
                emit_lua_arg_pack_struct(&mut out, &contract_struct, func);
                out.push('\n');
            }
        }
    }
    out.push_str("]])\n");
    out
}

fn generate_host_callers_file(ir: &ValidatedIr) -> String {
    let mut out: String = String::new();
    out.push_str(file_header());
    out.push_str("local ffi = require(\"ffi\")\n");
    out.push_str("local polyplug_guest = require(\"polyplug_guest\")\n\n");
    out.push_str("local M = {}\n\n");

    for contract in &ir.contracts {
        let contract_prefix: String = contract_name_to_prefix(&contract.name);
        for func in &contract.functions {
            generate_host_caller_function(&mut out, func, &contract_prefix);
            out.push('\n');
        }
    }

    out.push_str("return M\n");
    out
}

fn generate_guest_contracts_file(ir: &ValidatedIr) -> String {
    let mut out: String = String::new();
    out.push_str(file_header());
    out.push_str("local ffi = require(\"ffi\")\n");
    out.push_str("local polyplug_guest = require(\"polyplug_guest\")\n\n");
    out.push_str("local M = {}\n\n");

    for contract in &ir.contracts {
        generate_guest_contract_registration(&mut out, contract);
        out.push('\n');
    }

    out.push_str("return M\n");
    out
}

fn generate_lua_user_type(out: &mut String, ty: &ResolvedType) {
    out.push_str("    typedef struct {\n");
    for field in &ty.fields {
        let ty_name: String = lua_type_name(&field.ty);
        out.push_str(&format!(
            "        {ty_name} {field_name};\n",
            field_name = field.name
        ));
    }
    out.push_str(&format!("    }} {};\n", ty.name));
}

fn generate_host_caller_function(out: &mut String, func: &ResolvedFunction, prefix: &str) {
    let fn_name: String = format!("{}_{}", prefix, func.name);
    let sig_params: String = build_lua_sig_params(func);
    out.push_str(&format!("function M.{fn_name}(vtable{sig_params})\n"));
    emit_lua_host_args_setup(out, func, prefix);
    emit_lua_host_out_setup(out, &func.returns);
    out.push_str("    local fn_ptr = vtable.functions[");
    out.push_str(&format!("{}]", func.function_id));
    out.push('\n');
    out.push_str("    if fn_ptr == nil then\n");
    out.push_str("        error(\"missing function pointer\", 2)\n");
    out.push_str("    end\n");
    out.push_str("    local fn = ffi.cast(\"uint32_t (*)(const void*, void*)\", fn_ptr)\n");
    out.push_str("    local err = fn(args_ptr, out_ptr)\n");
    out.push_str("    if err ~= 0 then\n");
    out.push_str("        error(\"polyplug call failed\", 2)\n");
    out.push_str("    end\n");
    if has_return_value(&func.returns) {
        out.push_str("    return out_val\n");
    } else {
        out.push_str("    return nil\n");
    }
    out.push_str("end\n");
}

fn generate_guest_contract_registration(out: &mut String, contract: &ResolvedContract) {
    let contract_lower: String = contract.name.replace('.', "_");
    let plugin_name: String = format!("{}_plugin", contract_lower);
    let contract_version: u32 = contract.version.minor_patch_encoded();
    let function_count: usize = contract.functions.len();
    out.push_str(&format!(
        "function M.register_{contract_lower}(registrar_ptr)\n"
    ));
    out.push_str("    if registrar_ptr == nil then\n");
    out.push_str("        return\n");
    out.push_str("    end\n");
    out.push_str("    local registrar = polyplug_guest.cast_registrar(registrar_ptr)\n");
    out.push_str("    if registrar == nil then\n");
    out.push_str("        return\n");
    out.push_str("    end\n");
    out.push_str("    local descriptor = ffi.new(\"PluginDescriptor\")\n");
    out.push_str(&format!(
        "    descriptor.name = polyplug_guest.string_view(\"{plugin_name}\")\n",
        plugin_name = plugin_name
    ));
    out.push_str(&format!(
        "    descriptor.contract_name = polyplug_guest.string_view(\"{contract_name}\")\n",
        contract_name = contract.name
    ));
    out.push_str(&format!(
        "    descriptor.version_major = {major}\n",
        major = contract.version.major
    ));
    out.push_str(&format!(
        "    descriptor.version_minor = {minor}\n",
        minor = contract.version.minor
    ));
    out.push_str(&format!(
        "    descriptor.version_patch = {patch}\n",
        patch = contract.version.patch
    ));
    out.push_str("    local vtable = ffi.new(\"PluginVTable\")\n");
    out.push_str(&format!(
        "    vtable.contract_id = 0x{:016X}\n",
        contract.contract_id
    ));
    out.push_str(&format!(
        "    vtable.contract_version = {}\n",
        contract_version
    ));
    out.push_str(&format!("    vtable.function_count = {}\n", function_count));
    out.push_str("    vtable.functions = nil\n");
    out.push_str("    local err = registrar.register_plugin(registrar, descriptor, vtable)\n");
    out.push_str("    if err.code ~= 0 then\n");
    out.push_str("        error(\"plugin registration failed\", 2)\n");
    out.push_str("    end\n");
    out.push_str("end\n");
}

fn build_lua_sig_params(func: &ResolvedFunction) -> String {
    if func.params.is_empty() {
        return String::new();
    }
    let params: Vec<String> = func
        .params
        .iter()
        .map(|p: &ResolvedParam| format!(", {}", p.name))
        .collect();
    params.join("")
}

fn emit_lua_host_args_setup(out: &mut String, func: &ResolvedFunction, contract_prefix: &str) {
    if func.params.is_empty() {
        out.push_str("    local args_ptr = nil\n");
        return;
    }
    if func.params.len() == 1 {
        let param: &ResolvedParam = &func.params[0];
        match &param.ty {
            ResolvedTypeRef::UserDefined(_) => {
                out.push_str(&format!(
                    "    local args_ptr = ffi.cast(\"const void*\", {} )\n",
                    param.name
                ));
            }
            _ => {
                let ty_name: String = lua_type_name(&param.ty);
                out.push_str(&format!(
                    "    local {name}_val = ffi.new(\"{ty}\", {name})\n",
                    name = param.name,
                    ty = ty_name
                ));
                out.push_str(&format!(
                    "    local args_ptr = ffi.cast(\"const void*\", {name}_val)\n",
                    name = param.name
                ));
            }
        }
        return;
    }
    let contract_struct: String = contract_name_to_struct(contract_prefix);
    let pack_struct: String = arg_pack_struct_name(&contract_struct, &func.name);
    out.push_str(&format!(
        "    local args_val = ffi.new(\"{pack_struct}\")\n",
    ));
    for param in &func.params {
        out.push_str(&format!("    args_val.{0} = {0}\n", param.name));
    }
    out.push_str("    local args_ptr = ffi.cast(\"const void*\", args_val)\n");
}

fn emit_lua_host_out_setup(out: &mut String, returns: &Option<ResolvedTypeRef>) {
    if !has_return_value(returns) {
        out.push_str("    local out_ptr = nil\n");
        return;
    }
    let ret_ty: String = match returns {
        Some(ret) => lua_type_name(ret),
        None => "void".to_owned(),
    };
    out.push_str(&format!("    local out_val = ffi.new(\"{ret_ty}\")\n"));
    out.push_str("    local out_ptr = ffi.cast(\"void*\", out_val)\n");
}

fn lua_type_name(ty: &ResolvedTypeRef) -> String {
    match ty {
        ResolvedTypeRef::Primitive(p) => p.cpp_name().to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => "StringView".to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => "Buffer".to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::Ptr) => "void*".to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::Void) => "void".to_owned(),
        ResolvedTypeRef::UserDefined(name) => name.clone(),
    }
}

fn has_return_value(returns: &Option<ResolvedTypeRef>) -> bool {
    match returns {
        Some(ty) => !matches!(ty, ResolvedTypeRef::AbiType(AbiBuiltin::Void)),
        None => false,
    }
}

fn contract_name_to_prefix(name: &str) -> String {
    name.replace('.', "_")
}

fn contract_name_to_struct(name: &str) -> String {
    name.split('.')
        .map(|p: &str| {
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

fn needs_arg_pack(params: &[ResolvedParam]) -> bool {
    params.len() >= 2
}

fn emit_lua_arg_pack_struct(out: &mut String, contract_struct: &str, func: &ResolvedFunction) {
    let struct_name: String = arg_pack_struct_name(contract_struct, &func.name);
    out.push_str("    typedef struct {\n");
    for param in &func.params {
        let ty_name: String = lua_type_name(&param.ty);
        out.push_str(&format!(
            "        {ty_name} {param_name};\n",
            param_name = param.name
        ));
    }
    out.push_str(&format!("    }} {struct_name};\n"));
}

fn arg_pack_struct_name(contract_struct: &str, fn_name: &str) -> String {
    let fn_pascal: String = fn_name
        .split('_')
        .map(|seg: &str| {
            let mut chars: core::str::Chars<'_> = seg.chars();
            match chars.next() {
                Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join("");
    format!("{contract_struct}{fn_pascal}Args")
}

fn file_header() -> &'static str {
    "-- THIS FILE IS AUTO-GENERATED BY polyplugc\n\
     -- DO NOT EDIT BY HAND\n\
     -- Re-generate with: polyplugc generate --api <api.toml> --lang lua --out <dir>\n\n"
}

fn cdef_guarded_block() -> &'static str {
    "local function cdef_guarded(decl)\n\
    \tlocal ok, err = pcall(ffi.cdef, decl)\n\
    \tif not ok and not string.find(err, \"already defined\", 1, true) then\n\
    \t\terror(err, 2)\n\
    \tend\n\
     end\n\n"
}

const _: fn() = || {
    let _: String = lua_type_name(&ResolvedTypeRef::Primitive(PrimitiveType::U8));
};
