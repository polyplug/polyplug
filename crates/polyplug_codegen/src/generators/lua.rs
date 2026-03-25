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
use crate::ir::ResolvedFunction;
use crate::ir::ResolvedParam;
use crate::ir::ResolvedPlugin;
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
    ) -> Result<(), PolyplugcError> {
        let types_lua: String = generate_lua_types_file(ir);
        let callers_lua: String = generate_host_callers_file(ir);

        files.files.push(GeneratedFile {
            path: PathBuf::from("host/types.lua"),
            content: types_lua,
            force_regenerate: false,
        });
        files.files.push(GeneratedFile {
            path: PathBuf::from("host/callers.lua"),
            content: callers_lua,
            force_regenerate: false,
        });

        Ok(())
    }

    fn generate_guest(
        &self,
        ir: &ValidatedIr,
        files: &mut GeneratedFiles,
    ) -> Result<(), PolyplugcError> {
        let types_lua: String = generate_lua_types_file(ir);
        let contracts_lua: String = generate_guest_contracts_file(ir)?;

        files.files.push(GeneratedFile {
            path: PathBuf::from("guest/types.lua"),
            content: types_lua,
            force_regenerate: false,
        });
        files.files.push(GeneratedFile {
            path: PathBuf::from("guest/contracts.lua"),
            content: contracts_lua,
            force_regenerate: false,
        });
        let init_lua: String = generate_init_lua(ir);
        files.files.push(GeneratedFile {
            path: PathBuf::from("guest/init.lua"),
            content: init_lua,
            force_regenerate: false,
        });

        if ir.bundle.is_some() {
            files.files.push(GeneratedFile {
                path: PathBuf::from("manifest.toml"),
                content: generate_bundle_manifest_lua(ir),
                force_regenerate: true,
            });
        }

        Ok(())
    }
}

fn generate_bundle_manifest_lua(ir: &ValidatedIr) -> String {
    let bundle: &ResolvedBundle = match ir.bundle.as_ref() {
        Some(b) => b,
        None => return String::from("# ERROR: bundle manifest called without bundle IR\n"),
    };

    let name: &str = &bundle.name;
    let version: String = format!(
        "{}.{}.{}",
        bundle.version.major, bundle.version.minor, bundle.version.patch
    );
    let file_field: String = super::format_manifest_file_field(&bundle.file);

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

    let reinit: bool = bundle.needs_reinit_on_dep_reload;
    let runtime: &str = &bundle.runtime;

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

fn generate_lua_types_file(ir: &ValidatedIr) -> String {
    let mut out: String = String::new();
    out.push_str(file_header());
    // Conditionally require the bit library for bitwise enum support
    if needs_bit_library(&ir.enums) {
        out.push_str("local bit = require(\"bit\")\n");
    }
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
    out.push_str("]]) \n");
    // Emit enum tables (outside cdef — Lua tables, not C structs)
    for e in &ir.enums {
        generate_lua_enum(&mut out, e);
        out.push('\n');
    }
    for ty in &ir.types {
        out.push_str(&format!("ffi.metatype(\"{}\", {{}})\n", ty.name));
    }
    out
}

fn generate_host_callers_file(ir: &ValidatedIr) -> String {
    let mut out: String = String::new();
    out.push_str(file_header());
    out.push_str("local ffi = require(\"ffi\")\n\n");

    // ABI constants for host
    out.push_str("-- ABI constants\n");
    out.push_str("local ABI_OK = 0\n");
    out.push_str("local ABI_ERROR_GENERIC = 1\n\n");

    // Contract ID constants
    out.push_str("-- Contract ID constants\n");
    for contract in &ir.contracts {
        let upper_name: String = contract.name.to_uppercase().replace(['.', '-'], "_");
        out.push_str(&format!(
            "local {}_CONTRACT_ID = 0x{:016X}ULL\n",
            upper_name, contract.contract_id
        ));
    }
    out.push('\n');

    out.push_str("local M = {}\n\n");

    // Export contract ID constants
    for contract in &ir.contracts {
        let upper_name: String = contract.name.to_uppercase().replace(['.', '-'], "_");
        out.push_str(&format!(
            "M.{}_CONTRACT_ID = {}_CONTRACT_ID\n",
            upper_name, upper_name
        ));
    }
    out.push('\n');

    // Cached FFI types for hot path performance
    out.push_str("-- Cached FFI types for hot path performance\n");
    out.push_str("local DispatchFnType = ffi.typeof(\"uint32_t (*)(const void*, void*)\")\n\n");

    for contract in &ir.contracts {
        generate_host_contract_caller(&mut out, contract);
        out.push('\n');
    }

    out.push_str("return M\n");
    out
}

fn generate_guest_contracts_file(ir: &ValidatedIr) -> Result<String, PolyplugcError> {
    let mut out: String = String::new();
    out.push_str(file_header());
    out.push_str("local ffi = require(\"ffi\")\n");
    out.push_str("local polyplug_guest = require(\"polyplug_guest\")\n\n");
    out.push_str("local M = {}\n\n");

    if let Some(bundle) = &ir.bundle {
        for plugin in &bundle.plugins {
            for contract_impl in &plugin.implements {
                if let Some(contract) = ir.contracts.iter().find(|c| {
                    let contract_full =
                        format!("{}@{}.{}", c.name, c.version.major, c.version.minor);
                    &contract_full == contract_impl
                }) {
                    generate_guest_plugin_vtable(&mut out, &plugin.name, contract)?;
                }
            }
        }
    }

    out.push_str("return M\n");
    Ok(out)
}

fn generate_init_lua(ir: &ValidatedIr) -> String {
    let mut out: String = String::new();
    out.push_str("-- THIS FILE IS AUTO-GENERATED BY polyplugc. DO NOT EDIT.\n");
    out.push_str(
        "-- Re-generate with: polyplugc generate --bundle bundle.toml --lang lua --out <dir>\n\n",
    );
    out.push_str("local ffi = require(\"ffi\")\n");
    out.push_str("local polyplug_guest = require(\"polyplug_guest\")\n\n");

    let has_trace: bool = ir.bundle.as_ref().is_some_and(|b: &ResolvedBundle| {
        b.plugins
            .iter()
            .any(|p: &ResolvedPlugin| p.optional.contains(&"trace".to_owned()))
    });

    out.push_str("-- ABI constants\n");
    out.push_str("local ABI_OK = 0\n");
    out.push_str("local ABI_ERROR_GENERIC = 1\n\n");

    if has_trace {
        out.push_str("local EXT_TRACE_ID = 0xC4EB9AEE\n");
        out.push_str("-- Optional: trace extension\n");
        out.push_str("local trace_vtable_ptr = polyplug.get_extension(EXT_TRACE_ID)\n");
        out.push_str("-- trace_vtable_ptr is nil/0 if not available\n\n");
    } else {
        out.push_str("-- No optional extensions requested.\n\n");
    }

    out.push_str("--- Register all plugin vtables with the host.\n");
    out.push_str("--- @param rt_ctx userdata Runtime context pointer from host.\n");
    out.push_str("--- @param host_ptr userdata HostVTable pointer from host.\n");
    out.push_str("--- @param ctx_ptr userdata PluginContext pointer from host.\n");
    out.push_str("--- @return number error_code 0 on success, non-zero on failure.\n");
    out.push_str("function polyplug_init(rt_ctx, host_ptr, ctx_ptr)\n");
    out.push_str("    if rt_ctx == nil then\n");
    out.push_str("        return ABI_ERROR_GENERIC\n");
    out.push_str("    end\n");
    out.push_str("    if host_ptr == nil then\n");
    out.push_str("        return ABI_ERROR_GENERIC\n");
    out.push_str("    end\n");
    out.push_str("    if ctx_ptr == nil then\n");
    out.push_str("        return ABI_ERROR_GENERIC\n");
    out.push_str("    end\n");
    out.push_str("    local ctx = polyplug_guest.cast_context(ctx_ptr)\n");
    out.push_str("    local host = ffi.cast(\"HostVTable*\", host_ptr)\n\n");

    if let Some(bundle) = &ir.bundle {
        for plugin in &bundle.plugins {
            let plugin_upper: String = plugin.name.to_uppercase().replace('.', "_");
            let contract_impl = plugin.implements.first().map(|s| s.as_str()).unwrap_or("");
            let (contract_name, version_str) = contract_impl
                .split_once('@')
                .unwrap_or((contract_impl, "1.0.0"));
            let (version_major, _version_minor_patch) =
                version_str.split_once('.').unwrap_or((version_str, "0"));
            let _contract_name_full = format!("{}@{}", contract_name, version_major);

            out.push_str(&format!(
                "    local err_{plugin_upper} = host.register_plugin(rt_ctx, {plugin_upper}_DESCRIPTOR, {plugin_upper}_VTABLE)\n"
            ));
            out.push_str(&format!("    if err_{plugin_upper}.code ~= ABI_OK then\n"));
            out.push_str(&format!("        return err_{plugin_upper}.code\n"));
            out.push_str("    end\n\n");
        }
    }

    out.push_str("    return ABI_OK\n");
    out.push_str("end\n");
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

/// Generate the full host caller for a contract with factory pattern.
/// Creates methods table, metatable, and factory function.
fn generate_host_contract_caller(out: &mut String, contract: &ResolvedContract) {
    let contract_prefix: String = contract_name_to_prefix(&contract.name);
    let contract_struct: String = contract_name_to_struct(&contract.name);
    let contract_upper: String = contract.name.to_uppercase().replace(['.', '-'], "_");
    let contract_id_const: String = format!("{}_CONTRACT_ID", contract_upper);

    // Methods table
    out.push_str(&format!(
        "-- Methods for {contract_struct}\n",
        contract_struct = contract_struct
    ));
    out.push_str(&format!(
        "local {contract_struct}_methods = {{\n",
        contract_struct = contract_struct
    ));

    // is_valid method
    out.push_str("    is_valid = function(self)\n");
    out.push_str("        return self._guard ~= nil\n");
    out.push_str("    end,\n\n");

    // reset method
    out.push_str("    reset = function(self)\n");
    out.push_str("        self._guard = nil\n");
    out.push_str("    end,\n\n");

    // Contract function methods
    for func in &contract.functions {
        generate_host_caller_method(out, func, &contract_prefix, &contract_struct);
        out.push_str(",\n\n");
    }

    out.push_str("}\n\n");

    // Metatable
    out.push_str(&format!(
        "-- Metatable for {contract_struct}\n",
        contract_struct = contract_struct
    ));
    out.push_str(&format!(
        "local {contract_struct}_mt = {{\n",
        contract_struct = contract_struct
    ));
    out.push_str(&format!(
        "    __index = {contract_struct}_methods\n",
        contract_struct = contract_struct
    ));
    out.push_str("}\n\n");

    // Factory function
    out.push_str(&format!(
        "-- Factory function for {contract_struct}\n",
        contract_struct = contract_struct
    ));
    out.push_str(&format!(
        "function M.{contract_struct}_create(runtime, min_version)\n",
        contract_struct = contract_struct
    ));
    out.push_str("    if min_version == nil then min_version = 0 end\n");
    out.push_str(&format!(
        "    local handle = runtime:find_by_contract({contract_id_const}, min_version)\n"
    ));
    out.push_str("    if handle == nil then\n");
    out.push_str("        return nil\n");
    out.push_str("    end\n");
    out.push_str("    local guard = runtime:resolve_plugin(handle)\n");
    out.push_str("    if guard == nil then\n");
    out.push_str("        return nil\n");
    out.push_str("    end\n");
    out.push_str("    local instance = {\n");
    out.push_str("        _guard = guard\n");
    out.push_str("    }\n");
    out.push_str(&format!(
        "    setmetatable(instance, {contract_struct}_mt)\n",
        contract_struct = contract_struct
    ));
    out.push_str("    return instance\n");
    out.push_str("end\n");
}

/// Generate a single caller method for a contract function.
fn generate_host_caller_method(
    out: &mut String,
    func: &ResolvedFunction,
    contract_prefix: &str,
    _contract_struct: &str,
) {
    let fn_id: u32 = func.function_id;
    let sig_params: String = build_lua_sig_params(func);
    out.push_str(&format!("    {} = function(self{sig_params})\n", func.name));

    // Guard validity check
    out.push_str("        if self._guard == nil then\n");
    out.push_str("            error(\"invalid caller: guard is nil\", 2)\n");
    out.push_str("        end\n");

    // Get vtable from guard
    out.push_str("        local vtable = self._guard:_resolve_vtable()\n");
    out.push_str("        if vtable == nil then\n");
    out.push_str("            error(\"failed to resolve vtable\", 2)\n");
    out.push_str("        end\n");

    // Setup args and out
    emit_lua_host_args_setup(out, func, contract_prefix);
    emit_lua_host_out_setup(out, &func.returns);

    out.push_str("        local interface = ffi.cast(\"PluginInterface*\", vtable)\n");
    out.push_str(&format!(
        "        if {fn_id} >= interface.function_count then\n"
    ));
    out.push_str("            error(\"function not available in vtable\", 2)\n");
    out.push_str("        end\n");
    out.push_str(&format!(
        "        local fn_ptr = interface.dispatch.native.functions[{fn_id}]\n"
    ));
    out.push_str("        local fn = ffi.cast(DispatchFnType, fn_ptr)\n");
    out.push_str("        local err = fn(args_ptr, out_ptr)\n");
    out.push_str("        if err ~= 0 then\n");
    out.push_str("            error(\"polyplug call failed\", 2)\n");
    out.push_str("        end\n");

    if has_return_value(&func.returns) {
        out.push_str("        return out_val\n");
    } else {
        out.push_str("        return nil\n");
    }
    out.push_str("    end");
}

/// Legacy function - kept for reference but no longer used.
/// The new pattern uses generate_host_contract_caller and generate_host_caller_method.
#[allow(dead_code)]
fn generate_host_caller_function(out: &mut String, func: &ResolvedFunction, prefix: &str) {
    let fn_name: String = format!("{}_{}", prefix, func.name);
    let sig_params: String = build_lua_sig_params(func);
    out.push_str(&format!("function M.{fn_name}(vtable{sig_params})\n"));
    emit_lua_host_args_setup(out, func, prefix);
    emit_lua_host_out_setup(out, &func.returns);
    out.push_str("    local fn_ptr = vtable.dispatch.native.functions[");
    out.push_str(&format!("{}]", func.function_id));
    out.push('\n');
    out.push_str("    if fn_ptr == nil then\n");
    out.push_str("        error(\"missing function pointer\", 2)\n");
    out.push_str("    end\n");
    out.push_str("    local fn = ffi.cast(DispatchFnType, fn_ptr)\n");
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

fn generate_guest_plugin_vtable(
    out: &mut String,
    plugin_name: &str,
    contract: &ResolvedContract,
) -> Result<(), PolyplugcError> {
    let plugin_var: String = plugin_name.to_uppercase().replace(['.', '-'], "_");
    let contract_name_full: String = format!("{}@{}", contract.name, contract.version.major);
    let function_count: usize = contract.functions.len();

    out.push_str(&format!(
        "-- Function pointer type for {plugin_name} ({contract_name_full})\n"
    ));
    for func in &contract.functions {
        let fn_name: String = func.name.replace('.', "_");
        let params: Vec<String> = func
            .params
            .iter()
            .map(|p| format!("{}: {}", p.name, lua_type_name(&p.ty)))
            .collect();
        let ret_ty: String = match &func.returns {
            Some(ty) => lua_type_name(ty),
            None => "()".to_owned(),
        };
        out.push_str(&format!(
            "--   {fn_name}({}) -> {ret_ty}\n",
            params.join(", ")
        ));
    }

    out.push_str(&format!(
        "local {plugin_var}_VTABLE = ffi.new(\"PluginInterface\")\n"
    ));
    out.push_str(&format!(
        "{plugin_var}_VTABLE.contract_id = 0x{:016X}\n",
        contract.contract_id
    ));
    out.push_str(&format!(
        "{plugin_var}_VTABLE.contract_version = {}\n",
        contract.version.minor_patch_encoded()
    ));
    out.push_str(&format!(
        "{plugin_var}_VTABLE.function_count = {function_count}\n"
    ));
    out.push_str(&format!(
        "{plugin_var}_VTABLE.dispatch_type = polyplug_guest.DispatchType.VirtualMachine\n"
    ));
    out.push_str(&format!("{plugin_var}_VTABLE.functions = nil\n\n"));

    out.push_str(&format!(
        "local {plugin_var}_DESCRIPTOR = ffi.new(\"PluginDescriptor\")\n"
    ));
    out.push_str(&format!(
        "{plugin_var}_DESCRIPTOR.name = polyplug_guest.string_view(\"{plugin_name}\")\n"
    ));
    out.push_str(&format!(
        "{plugin_var}_DESCRIPTOR.contract_name = polyplug_guest.string_view(\"{contract_name_full}\")\n"
    ));
    out.push_str(&format!(
        "{plugin_var}_DESCRIPTOR.version_major = {}\n",
        contract.version.major
    ));
    out.push_str(&format!(
        "{plugin_var}_DESCRIPTOR.version_minor = {}\n",
        contract.version.minor
    ));
    out.push_str(&format!(
        "{plugin_var}_DESCRIPTOR.version_patch = {}\n\n",
        contract.version.patch
    ));

    let set_impl_name: String = format!(
        "set_{}_impl",
        plugin_name.to_lowercase().replace(['.', '-'], "_")
    );
    out.push_str(&format!("\nfunction M.{set_impl_name}("));
    let impl_params: Vec<String> = contract
        .functions
        .iter()
        .map(|f| format!("{}_fn", f.name))
        .collect();
    out.push_str(&impl_params.join(", "));
    out.push_str(")\n");

    out.push_str(&format!(
        "    local functions = ffi.new(\"PluginFunction[{function_count}]\")\n"
    ));
    for (idx, func) in contract.functions.iter().enumerate() {
        let fn_name: String = func.name.replace('.', "_");
        out.push_str(&format!(
            "    functions[{idx}] = ffi.cast(\"uintptr_t\", {fn_name}_fn)\n"
        ));
    }
    out.push_str(&format!("    {plugin_var}_VTABLE.functions = functions\n"));
    out.push_str("end\n");

    Ok(())
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

/// Returns true if any enum in `enums` has a variant value that uses `<<`, `|`, or `~`.
fn needs_bit_library(enums: &[EnumDef]) -> bool {
    for e in enums {
        for variant in &e.variants {
            if variant.value.contains("<<")
                || variant.value.contains('|')
                || variant.value.contains('~')
            {
                return true;
            }
        }
    }
    false
}

fn substitute_variant_refs_lua(declared_variants: &[EnumVariant], expr: &str) -> String {
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

/// Transform a value expression for LuaJIT compatibility.
/// Converts `<<` to `bit.lshift(lhs, rhs)`, `|` to `bit.bor(lhs, rhs)`, `~` to `bit.bnot(inner)`.
/// Operates on post-substitution expression strings.
///
/// Precedence: `~` > `<<` > `|` (from tightest to loosest binding)
/// Implementation: simple recursive approach on the constrained grammar.
fn lua_transform_value_expr(expr: &str) -> String {
    let expr: &str = expr.trim();

    // Try to split on `|` at top level (respecting parens) — lowest precedence
    if let Some(parts) = split_on_top_level(expr, '|') {
        let transformed: Vec<String> = parts
            .iter()
            .map(|p| lua_transform_value_expr(p.trim()))
            .collect();
        if transformed.len() == 1 {
            return transformed.into_iter().next().unwrap_or_default();
        }
        // bit.bor(a, b) — but bit.bor only takes 2 args; chain for 3+
        return transformed
            .into_iter()
            .reduce(|acc, next| format!("bit.bor({}, {})", acc, next))
            .unwrap_or_default();
    }

    // Try to split on `<<` — higher precedence than |
    if let Some(parts) = split_on_top_level_two_char(expr, '<', '<')
        && parts.len() == 2
    {
        let lhs: String = lua_transform_value_expr(parts[0].trim());
        let rhs: String = lua_transform_value_expr(parts[1].trim());
        return format!("bit.lshift({}, {})", lhs, rhs);
    }

    // Handle ~ prefix
    if let Some(stripped) = expr.strip_prefix('~') {
        let inner: String = lua_transform_value_expr(stripped.trim());
        return format!("bit.bnot({})", inner);
    }

    // Parenthesized expression — recurse inside
    if expr.starts_with('(') && expr.ends_with(')') {
        let inner: &str = &expr[1..expr.len() - 1];
        return lua_transform_value_expr(inner.trim());
    }

    // Pure integer literal or simple token — return as-is
    expr.to_owned()
}

/// Split expr on a top-level single char operator (respecting parentheses).
/// Returns None if char not found at top level.
fn split_on_top_level(expr: &str, op: char) -> Option<Vec<&str>> {
    let chars: Vec<char> = expr.chars().collect();
    let len: usize = chars.len();
    let mut depth: i32 = 0;
    let mut splits: Vec<usize> = Vec::new();
    let mut i: usize = 0;
    while i < len {
        match chars[i] {
            '(' => {
                depth += 1;
            }
            ')' => {
                depth -= 1;
            }
            c if c == op && depth == 0 => {
                splits.push(i);
            }
            _ => {}
        }
        i += 1;
    }
    if splits.is_empty() {
        return None;
    }
    let mut parts: Vec<&str> = Vec::new();
    let mut prev: usize = 0;
    for &pos in &splits {
        parts.push(&expr[prev..pos]);
        prev = pos + 1;
    }
    parts.push(&expr[prev..]);
    Some(parts)
}

/// Split expr on a top-level two-char operator (e.g., `<<`).
fn split_on_top_level_two_char(expr: &str, op1: char, op2: char) -> Option<Vec<&str>> {
    let chars: Vec<char> = expr.chars().collect();
    let len: usize = chars.len();
    let mut depth: i32 = 0;
    let mut split_pos: Option<usize> = None;
    let mut i: usize = 0;
    while i < len {
        match chars[i] {
            '(' => {
                depth += 1;
            }
            ')' => {
                depth -= 1;
            }
            c if c == op1 && depth == 0 && i + 1 < len && chars[i + 1] == op2 => {
                split_pos = Some(i);
                break;
            }
            _ => {}
        }
        i += 1;
    }
    let pos: usize = split_pos?;
    Some(vec![&expr[..pos], &expr[pos + 2..]])
}

fn generate_lua_enum(out: &mut String, e: &EnumDef) {
    if e.bitflag {
        out.push_str(&format!("--- Bitflag enum {}\n", e.name));
    } else {
        out.push_str(&format!("--- Enum {}\n", e.name));
    }
    out.push_str(&format!("local {} = {{\n", e.name));
    for variant in &e.variants {
        let subst_value: String = substitute_variant_refs_lua(&e.variants, &variant.value);
        let final_value: String = lua_transform_value_expr(&subst_value);
        out.push_str(&format!("    {} = {},\n", variant.name, final_value));
    }
    out.push_str("}\n");
}

// Compile-time assertion that lua_type_name compiles for primitive types.
const _: fn() = || {
    let _: String = lua_type_name(&ResolvedTypeRef::Primitive(PrimitiveType::U8));
};

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use super::*;
    use crate::ir::ReprType;

    #[test]
    fn generate_lua_enum_non_bitflag() {
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
        generate_lua_enum(&mut out, &e);
        assert!(
            out.contains("local PixelFormat = {"),
            "missing table def: {out}"
        );
        assert!(out.contains("Unknown = 0"), "missing Unknown: {out}");
    }

    #[test]
    fn generate_lua_enum_bitflag_with_bit_library() {
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
                EnumVariant {
                    name: "Hdr".to_owned(),
                    value: "1 << 1".to_owned(),
                },
                EnumVariant {
                    name: "CompressedHdr".to_owned(),
                    value: "Compressed | Hdr".to_owned(),
                },
            ],
        };
        let mut out: String = String::new();
        generate_lua_enum(&mut out, &e);
        assert!(
            out.contains("local ImageFlags = {"),
            "missing table def: {out}"
        );
        assert!(
            out.contains("bit.lshift(1, 1)"),
            "missing bit.lshift for Hdr: {out}"
        );
        assert!(
            out.contains("bit.bor("),
            "missing bit.bor for CompressedHdr: {out}"
        );
    }
}
