use std::collections::HashSet;
use std::path::PathBuf;

use super::CodeGenerator;
use super::GeneratedFile;
use super::GeneratedFiles;
use super::collect_peer_contracts;
use super::peer_min_version;

use super::docs::write_luals_docs;
use crate::PolyplugcError;
use crate::ir::AbiBuiltin;
use crate::ir::EnumDef;
use crate::ir::EnumVariant;
use crate::ir::PrimitiveType;
use crate::ir::ResolvedBundle;
use crate::ir::ResolvedContract;
use crate::ir::ResolvedField;
use crate::ir::ResolvedFunction;
use crate::ir::ResolvedHostContract;
use crate::ir::ResolvedParam;
use crate::ir::ResolvedPlugin;
use crate::ir::ResolvedType;
use crate::ir::ResolvedTypeRef;
use crate::ir::ValidatedIr;
use crate::ir::array_element_name;
use langprint::backends::lua_backend::{
    LuaBackend, LuaEnum, LuaEnumMember, LuaFunction, LuaFunctionRenderOptions,
};
use langprint::renderers::{EnumRenderer, FunctionRenderer};
use langprint::{ImportEntry, ImportSet, TargetLanguage};
use std::io;

pub struct LuaGenerator;
impl LuaGenerator {
    /// Generate the opt-in internal Lua profile without changing either default
    /// host or external guest output.
    pub(crate) fn generate_internal_bundle(
        &self,
        ir: &ValidatedIr,
        bundle_name: &str,
        files: &mut GeneratedFiles,
    ) -> Result<(), PolyplugcError> {
        let bundle: &ResolvedBundle =
            ir.bundle
                .as_ref()
                .ok_or_else(|| PolyplugcError::ValidationFailed {
                    message: "internal Lua generation requires a bundle manifest".to_owned(),
                })?;
        let host_types: String = generate_lua_types_file(ir)?;
        files.files.push(GeneratedFile {
            path: PathBuf::from("host/types.lua"),
            content: host_types,
            force_regenerate: false,
        });
        files.files.push(GeneratedFile {
            path: PathBuf::from("host/callers.lua"),
            content: generate_lua_internal_host_callers_file(ir),
            force_regenerate: false,
        });
        let types: String = generate_lua_types_file(ir)?;
        let profile: String = generate_lua_internal_profile_file(ir, bundle, bundle_name)?;
        files.files.push(GeneratedFile {
            path: PathBuf::from("guest/types.lua"),
            content: types,
            force_regenerate: false,
        });
        files.files.push(GeneratedFile {
            path: PathBuf::from("guest/internal.lua"),
            content: profile,
            force_regenerate: false,
        });
        files.files.push(GeneratedFile {
            path: PathBuf::from("init.lua"),
            content: format!(
                "{}local source = debug.getinfo(1, \"S\").source\n\
                 local root = assert(source:match(\"^@(.+)[/\\\\]init%.lua$\"), \"generated internal Lua bindings need a file path\")\n\
                 return {{ guest = dofile(root .. \"/guest/internal.lua\"), host = dofile(root .. \"/host/callers.lua\") }}\n",
                file_header()
            ),
            force_regenerate: true,
        });
        Ok(())
    }
}

/// Render grouped Lua `local m = require("m")` blocks through langprint's
/// [`ImportSet`] so the `require` syntax lives in one place rather than in
/// hand-written `push_str("local … = require(…)\n")` sequences. Each inner slice
/// is one `(binding, module)` group (deduped + sorted by binding name); non-empty
/// groups are separated by a blank line. Empty groups are skipped, so callers can
/// pass a conditional group unconditionally. The result ends in a single newline.
fn lua_require_block(groups: &[&[(&str, &str)]]) -> String {
    let mut blocks: Vec<String> = Vec::new();
    for group in groups {
        let mut set: ImportSet = ImportSet::new(TargetLanguage::Lua);
        for (name, module) in *group {
            set.add(ImportEntry::Require {
                name: (*name).to_string(),
                module: (*module).to_string(),
            });
        }
        let rendered: String = set.render();
        if !rendered.is_empty() {
            blocks.push(rendered);
        }
    }
    blocks.join("\n")
}

impl CodeGenerator for LuaGenerator {
    fn generate_host(
        &self,
        ir: &ValidatedIr,
        files: &mut GeneratedFiles,
    ) -> Result<(), PolyplugcError> {
        let types_lua: String = generate_lua_types_file(ir)?;
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

        // Emit host/contracts.lua if there are host contracts
        if !ir.host_contracts.is_empty() {
            let contracts_lua: String = generate_host_contracts_file(ir);
            files.files.push(GeneratedFile {
                path: PathBuf::from("host/contracts.lua"),
                content: contracts_lua,
                force_regenerate: false,
            });
            // Emit host/interface_factories.lua if there are host contracts
            let interface_factories_lua: String = generate_lua_host_interface_factories_file(ir);
            files.files.push(GeneratedFile {
                path: PathBuf::from("host/interface_factories.lua"),
                content: interface_factories_lua,
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
        let types_lua: String = generate_lua_types_file(ir)?;
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

        if ir.bundle.is_some() {
            files.files.push(GeneratedFile {
                path: PathBuf::from("manifest.toml"),
                content: generate_bundle_manifest_lua(ir),
                force_regenerate: true,
            });
        }

        if !ir.host_contracts.is_empty() {
            let host_contracts_lua: String = generate_guest_host_contracts_file(ir)?;
            files.files.push(GeneratedFile {
                path: PathBuf::from("guest/host_contracts.lua"),
                content: host_contracts_lua,
                force_regenerate: false,
            });
        }

        // ── guest/peer_callers.lua ─────────────────────────────────────────────
        let peer_contracts: Vec<&ResolvedContract> = collect_peer_contracts(ir);
        if !peer_contracts.is_empty() {
            let peer_callers_lua: String =
                generate_lua_guest_peer_callers_file(ir, &peer_contracts);
            files.files.push(GeneratedFile {
                path: PathBuf::from("guest/peer_callers.lua"),
                content: peer_callers_lua,
                force_regenerate: false,
            });
        }

        Ok(())
    }
}
fn generate_lua_internal_profile_file(
    ir: &ValidatedIr,
    bundle: &ResolvedBundle,
    bundle_name: &str,
) -> Result<String, PolyplugcError> {
    let providers: Vec<(&ResolvedPlugin, &ResolvedContract)> = lua_internal_providers(ir, bundle)?;
    let manifest: String = generate_lua_internal_profile_manifest(ir, bundle);
    let mut out: String = String::new();
    out.push_str(file_header());
    out.push_str(&lua_require_block(&[
        &[("ffi", "ffi")],
        &[("lua_loader", "polyplug.loaders.lua")],
    ]));
    out.push_str("\nlocal source = debug.getinfo(1, \"S\").source\n");
    out.push_str("local root = assert(source:match(\"^@(.+)[/\\\\]guest[/\\\\]internal%.lua$\"), \"generated internal Lua bindings need a file path\")\n");
    out.push_str("local types = dofile(root .. \"/guest/types.lua\")\n");
    out.push_str("local native_bridge = lua_loader.internal_plugin_bridge()\n");
    out.push_str("local string_view_ptr_t = ffi.typeof(\"StringView*\")\n");
    out.push_str("local buffer_ptr_t = ffi.typeof(\"Buffer*\")\n");
    out.push_str("local const_uint8_ptr_t = ffi.typeof(\"const uint8_t*\")\n");
    out.push_str("local void_ptr_t = ffi.typeof(\"void*\")\n");
    out.push_str("local uint64_ptr_t = ffi.typeof(\"uint64_t*\")\n");
    out.push_str("local uint32_ptr_t = ffi.typeof(\"uint32_t*\")\n");
    out.push_str("local uint16_ptr_t = ffi.typeof(\"uint16_t*\")\n");
    out.push_str("local uint8_ptr_t = ffi.typeof(\"uint8_t*\")\n");
    out.push_str("local int64_ptr_t = ffi.typeof(\"int64_t*\")\n");
    out.push_str("local int32_ptr_t = ffi.typeof(\"int32_t*\")\n");
    out.push_str("local int16_ptr_t = ffi.typeof(\"int16_t*\")\n");
    out.push_str("local int8_ptr_t = ffi.typeof(\"int8_t*\")\n");
    out.push_str("local float_ptr_t = ffi.typeof(\"float*\")\n");
    out.push_str("local double_ptr_t = ffi.typeof(\"double*\")\n");
    out.push_str("local bool_ptr_t = ffi.typeof(\"bool*\")\n");
    out.push_str("local void_ptr_ptr_t = ffi.typeof(\"void**\")\n");
    out.push_str("local callers = dofile(root .. \"/host/callers.lua\")\n");
    out.push_str("local AbiErrorCode = { Ok = 0, Generic = 1 }\nlocal M = {}\n");
    out.push_str(&format!(
        "M.INTERNAL_BUNDLE_NAME = {bundle_name:?}\nM.INTERNAL_BUNDLE_ID = 0x{:016X}ULL\n",
        bundle.bundle_id
    ));
    out.push_str(&format!("local INTERNAL_MANIFEST = {manifest:?}\n\n"));
    out.push_str("local function provider_factory(provider, name)\n");
    out.push_str("    if type(provider) == \"function\" then return provider end\n");
    out.push_str(
        "    error(\"internal provider \" .. name .. \" must be a factory function\", 3)\nend\n\n",
    );
    out.push_str("function M.providers(values)\n");
    out.push_str(
        "    if type(values) ~= \"table\" then error(\"providers must be a table\", 2) end\n",
    );
    out.push_str("    return { _values = values, _consumed = false }\nend\n\n");
    out.push_str("function M.register(runtime, providers)\n");
    out.push_str("    if type(providers) ~= \"table\" then error(\"register requires providers created by guest.internal.providers\", 2) end\n");
    out.push_str("    if providers._consumed then error(\"providers were consumed by a previous registration attempt; create fresh providers\", 2) end\n");
    out.push_str("    if providers._values == nil then error(\"register requires providers created by guest.internal.providers\", 2) end\n");
    out.push_str("    providers._consumed = true\n");
    out.push_str("    local values = providers._values\n    providers._values = nil\n");
    out.push_str("    local provider_host = runtime:host()\n");
    out.push_str("    local factories = {}\n");
    for (plugin, contract) in &providers {
        let field: String = lua_internal_provider_field(plugin, contract);
        out.push_str(&format!(
            "    factories[{field:?}] = provider_factory(values[{field:?}], {field:?})\n",
        ));
    }
    out.push_str("    local resident = native_bridge.create_resident(INTERNAL_MANIFEST)\n");
    for (plugin, contract) in &providers {
        generate_lua_internal_profile_provider(&mut out, plugin, contract, &ir.enums, &ir.types);
    }
    out.push_str("    local registration = runtime:register_internal_plugin(resident)\n");
    out.push_str("    local bundle_id = registration.bundle_id\n");
    out.push_str("    local handles = registration.handles\n");
    out.push_str("    local result = { bundle_id = bundle_id }\n");
    out.push_str("    local constructed_callers = {}\n");
    for (index, (plugin, contract)) in providers.iter().enumerate() {
        let field: String = lua_internal_provider_field(plugin, contract);
        let caller: String = contract_name_to_struct(&contract.name);
        out.push_str(&format!(
            "    local {field}_ok, {field}_caller = pcall(callers.{caller}_create_from_handle, runtime, provider_host, handles[{}])\n\
             \x20   if not {field}_ok or {field}_caller == nil then\n\
             \x20       local construction_error = {field}_ok and \"generated caller construction failed for {field}\" or {field}_caller\n\
             \x20       for _, previous in ipairs(constructed_callers) do pcall(function() previous:destroy() end) end\n\
             \x20       pcall(function() runtime:unload_bundle(bundle_id) end)\n\
             \x20       error(construction_error, 0)\n\
             \x20   end\n\
             \x20   result[{field:?}] = {field}_caller\n\
             \x20   constructed_callers[#constructed_callers + 1] = {field}_caller\n",
            index + 1
        ));
    }
    out.push_str("    return result\nend\n\nreturn M\n");
    Ok(out)
}

fn lua_internal_providers<'a>(
    ir: &'a ValidatedIr,
    bundle: &'a ResolvedBundle,
) -> Result<Vec<(&'a ResolvedPlugin, &'a ResolvedContract)>, PolyplugcError> {
    let mut providers: Vec<(&ResolvedPlugin, &ResolvedContract)> = Vec::new();
    for plugin in &bundle.plugins {
        for implementation in &plugin.implements {
            let contract: Option<&ResolvedContract> = ir.contracts.iter().find(|candidate| {
                implementation
                    == &format!(
                        "{}@{}.{}",
                        candidate.name, candidate.version.major, candidate.version.minor
                    )
            });
            let Some(contract) = contract else {
                return Err(PolyplugcError::ValidationFailed {
                    message: format!(
                        "internal Lua generation could not resolve {} for plugin {}",
                        implementation, plugin.name
                    ),
                });
            };
            providers.push((plugin, contract));
        }
    }
    Ok(providers)
}

fn lua_internal_provider_field(plugin: &ResolvedPlugin, contract: &ResolvedContract) -> String {
    format!(
        "{}_{}",
        plugin.name.replace(['.', '-'], "_"),
        contract.name.replace(['.', '-'], "_")
    )
}

fn generate_lua_internal_profile_manifest(ir: &ValidatedIr, bundle: &ResolvedBundle) -> String {
    let mut provides: Vec<String> = bundle
        .plugins
        .iter()
        .flat_map(|plugin| plugin.implements.iter())
        .map(|provider| {
            let (contract, version) = provider.split_once('@').unwrap_or((provider, "0"));
            let major: &str = version.split('.').next().unwrap_or(version);
            format!("{contract}@{major}")
        })
        .collect();
    provides.sort();
    provides.dedup();
    let provides_toml: String = provides
        .iter()
        .map(|provider| format!("{provider:?}"))
        .collect::<Vec<String>>()
        .join(", ");
    let function_counts: String = ir
        .contracts
        .iter()
        .filter(|contract| {
            provides.contains(&format!("{}@{}", contract.name, contract.version.major))
        })
        .map(|contract| {
            format!(
                "{:?} = {}",
                format!("{}@{}", contract.name, contract.version.major),
                contract.functions.len()
            )
        })
        .collect::<Vec<String>>()
        .join(", ");
    let dependencies: String = super::emit_manifest_dependencies(&bundle.dependencies);
    format!(
        "name = {:?}\n\
         id = {}\n\
         version = \"{}.{}.{}\"\n\
         provides = [{provides_toml}]\n\
         function_count = {{ {function_counts} }}\n\
         needs_reinit_on_dep_reload = {}\n\
         {dependencies}",
        bundle.name,
        bundle.bundle_id,
        bundle.version.major,
        bundle.version.minor,
        bundle.version.patch,
        bundle.needs_reinit_on_dep_reload,
    )
}

fn generate_lua_internal_profile_provider(
    out: &mut String,
    plugin: &ResolvedPlugin,
    contract: &ResolvedContract,
    enums: &[EnumDef],
    types: &[ResolvedType],
) {
    let field: String = lua_internal_provider_field(plugin, contract);
    let contract_struct: String = contract_name_to_struct(&contract.name);
    out.push_str(&format!("    do -- {field}\n"));
    out.push_str(&format!("        local factory = factories[{field:?}]\n"));
    out.push_str("        local function dispatch(impl, fn_id, args, out_ptr, return_roots)\n");
    for (function_index, function) in contract.functions.iter().enumerate() {
        out.push_str(&format!("            if fn_id == {function_index} then\n"));
        if function
            .returns
            .as_ref()
            .is_some_and(|return_type| !lua_return_is_scalar(return_type))
        {
            out.push_str("                return_roots.strings = {}\n                return_roots.buffers = {}\n                return_roots.values = {}\n");
        }
        generate_lua_host_dispatch_args(out, &contract_struct, function, enums);
        generate_lua_internal_profile_dispatch_call(out, function, enums, types);
        out.push_str("                return AbiErrorCode.Ok\n            end\n");
    }
    out.push_str("            return AbiErrorCode.Generic\n        end\n");
    out.push_str(&format!(
        "        if native_bridge.add_provider(resident, factory, dispatch, {:?}, {:?}, {}, {}, {}, {}, {}) == 0 then\n\
         \x20           native_bridge.release_resident(resident)\n\
         \x20           error(\"native provider registration failed for {field}\", 2)\n\
         \x20       end\n",
        plugin.name,
        contract.name,
        contract.contract_id as u32,
        (contract.contract_id >> 32) as u32,
        contract.version.major,
        contract.version.minor,
        contract.version.patch,
    ));
    out.push_str("    end\n");
}

fn generate_lua_internal_profile_dispatch_call(
    out: &mut String,
    func: &ResolvedFunction,
    enums: &[EnumDef],
    types: &[ResolvedType],
) {
    let call_args: String = func
        .params
        .iter()
        .map(|param| param.name.clone())
        .collect::<Vec<String>>()
        .join(", ");
    if !has_return_value(&func.returns) {
        out.push_str(&format!(
            "                impl:{}({call_args})\n",
            func.name
        ));
        out.push_str("                local _ = out_ptr\n");
        return;
    }
    out.push_str(&format!(
        "                local result = impl:{}({call_args})\n",
        func.name
    ));
    match func.returns.as_ref() {
        Some(ResolvedTypeRef::AbiType(AbiBuiltin::StringView)) => {
            out.push_str("                if type(result) ~= \"string\" then error(\"internal provider must return a Lua string\") end\n");
            out.push_str(
                "                return_roots.strings[#return_roots.strings + 1] = result\n",
            );
            out.push_str(
                "                local output = ffi.cast(string_view_ptr_t, out_ptr)[0]\n",
            );
            out.push_str("                output.ptr = ffi.cast(const_uint8_ptr_t, result)\n                output.len = #result\n");
        }
        Some(ResolvedTypeRef::AbiType(AbiBuiltin::Buffer)) => {
            out.push_str("                if type(result) == \"string\" then\n");
            out.push_str(
                "                    return_roots.buffers[#return_roots.buffers + 1] = result\n",
            );
            out.push_str("                    local output = ffi.cast(buffer_ptr_t, out_ptr)[0]\n");
            out.push_str("                    output.ptr = ffi.cast(void_ptr_t, result)\n                    output.len = #result\n");
            out.push_str("                else\n                    ffi.cast(buffer_ptr_t, out_ptr)[0] = result\n                end\n");
        }
        Some(return_type)
            if lua_enum_repr_c_type(return_type, enums).is_some()
                || lua_return_is_scalar(return_type) =>
        {
            let return_c_type: String = lua_c_type_name(return_type, enums);
            let pointer_type: &str = lua_internal_scalar_pointer_type(&return_c_type);
            out.push_str(&format!(
                "                ffi.cast({pointer_type}, out_ptr)[0] = result\n"
            ));
        }
        Some(return_type) => {
            generate_lua_internal_profile_composite_return(out, return_type, types, enums);
        }
        None => {}
    }
}

fn lua_internal_scalar_pointer_type(c_type: &str) -> &str {
    match c_type {
        "uint64_t" => "uint64_ptr_t",
        "uint32_t" => "uint32_ptr_t",
        "uint16_t" => "uint16_ptr_t",
        "uint8_t" => "uint8_ptr_t",
        "int64_t" => "int64_ptr_t",
        "int32_t" => "int32_ptr_t",
        "int16_t" => "int16_ptr_t",
        "int8_t" => "int8_ptr_t",
        "float" => "float_ptr_t",
        "double" => "double_ptr_t",
        "bool" => "bool_ptr_t",
        "void*" => "void_ptr_ptr_t",
        _ => unreachable!("unknown ABI scalar C type: {c_type}"),
    }
}

struct LuaInternalMarshalContext<'a> {
    types: &'a [ResolvedType],
    enums: &'a [EnumDef],
    uid: usize,
}

fn generate_lua_internal_profile_composite_return(
    out: &mut String,
    return_type: &ResolvedTypeRef,
    types: &[ResolvedType],
    enums: &[EnumDef],
) {
    let c_type: String = lua_c_type_name(return_type, enums);
    out.push_str(
        "                if result == nil then error(\"internal provider returned nil\") end\n",
    );
    out.push_str(&format!(
        "                local output = ffi.cast(\"{c_type}*\", out_ptr)\n"
    ));
    out.push_str("                if type(result) == \"cdata\" then\n");
    out.push_str("                    output[0] = result\n");
    out.push_str("                else\n");
    let mut context = LuaInternalMarshalContext {
        types,
        enums,
        uid: 0,
    };
    emit_lua_internal_profile_marshal_into(
        out,
        "output[0]",
        "result",
        &c_type,
        "                    ",
        &mut context,
    );
    out.push_str("                end\n");
}

fn emit_lua_internal_profile_marshal_into(
    out: &mut String,
    destination: &str,
    source: &str,
    c_type: &str,
    indent: &str,
    context: &mut LuaInternalMarshalContext<'_>,
) {
    if let Some(element) = array_element_name(c_type) {
        emit_lua_internal_profile_marshal_array(out, destination, source, element, indent, context);
        return;
    }
    if c_type == "StringView" {
        let id: usize = context.uid;
        out.push_str(&format!(
            "{indent}local string_value_{id} = tostring({source})\n\
             {indent}return_roots.strings[#return_roots.strings + 1] = string_value_{id}\n\
             {indent}{destination}.ptr = ffi.cast(const_uint8_ptr_t, string_value_{id})\n\
             {indent}{destination}.len = #string_value_{id}\n"
        ));
        context.uid += 1;
        return;
    }
    if c_type == "Buffer" {
        out.push_str(&format!("{indent}if type({source}) == \"string\" then\n"));
        out.push_str(&format!(
            "{indent}    return_roots.buffers[#return_roots.buffers + 1] = {source}\n\
             {indent}    {destination}.ptr = ffi.cast(void_ptr_t, {source})\n\
             {indent}    {destination}.len = #{source}\n\
             {indent}else\n\
             {indent}    {destination} = {source}\n\
             {indent}end\n"
        ));
        return;
    }
    if let Some(ty) = context.types.iter().find(|ty| ty.name == c_type) {
        for field in &ty.fields {
            let field_type: String = lua_c_type_name(&field.ty, context.enums);
            emit_lua_internal_profile_marshal_into(
                out,
                &format!("{destination}.{}", field.name),
                &format!("{source}.{}", field.name),
                &field_type,
                indent,
                context,
            );
        }
        return;
    }
    out.push_str(&format!("{indent}{destination} = {source}\n"));
}

fn emit_lua_internal_profile_marshal_array(
    out: &mut String,
    destination: &str,
    source: &str,
    element: &str,
    indent: &str,
    context: &mut LuaInternalMarshalContext<'_>,
) {
    let id: usize = context.uid;
    context.uid += 1;
    let c_type: String = lua_c_type_name(&lua_element_type_ref(element), context.enums);
    out.push_str(&format!("{indent}local count_{id} = #{source}\n"));
    out.push_str(&format!("{indent}if count_{id} == 0 then\n"));
    out.push_str(&format!("{indent}    {destination}.items = 0\n"));
    out.push_str(&format!("{indent}    {destination}.len = 0\n"));
    out.push_str(&format!("{indent}else\n"));
    out.push_str(&format!(
        "{indent}    local values_{id} = ffi.new(\"{c_type}[?]\", count_{id})\n\
         {indent}    return_roots.values[#return_roots.values + 1] = values_{id}\n\
         {indent}    for index_{id} = 0, count_{id} - 1 do\n"
    ));
    emit_lua_internal_profile_marshal_into(
        out,
        &format!("values_{id}[index_{id}]"),
        &format!("{source}[index_{id} + 1]"),
        element,
        &format!("{indent}        "),
        context,
    );
    out.push_str(&format!(
        "{indent}    end\n\
         {indent}    {destination}.items = ffi.cast(\"uint64_t\", ffi.cast(\"uintptr_t\", values_{id}))\n\
         {indent}    {destination}.len = count_{id}\n\
         {indent}end\n"
    ));
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

    let provides_set: HashSet<String> = provides.iter().cloned().collect();
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

    let dep_toml: String = super::emit_manifest_dependencies(&bundle.dependencies);

    let reinit: bool = bundle.needs_reinit_on_dep_reload;
    let loader: &str = &bundle.loader;

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

fn generate_lua_types_file(ir: &ValidatedIr) -> Result<String, PolyplugcError> {
    let mut out: String = String::new();
    out.push_str(file_header());
    // Conditionally require the bit library for bitwise enum support (sorted
    // before `ffi` by ImportSet, matching the previous hand-written order).
    let mut requires: Vec<(&str, &str)> = Vec::new();
    if needs_bit_library(&ir.enums) {
        requires.push(("bit", "bit"));
    }
    requires.push(("ffi", "ffi"));
    out.push_str(&lua_require_block(&[requires.as_slice()]));
    out.push('\n');
    out.push_str(cdef_guarded_block());
    out.push_str("cdef_guarded([[\n");
    for ty in &ir.types {
        generate_lua_user_type(&mut out, ty, &ir.enums);
        out.push('\n');
    }
    for contract in &ir.contracts {
        let contract_struct: String = contract_name_to_struct(&contract.name);
        for func in &contract.functions {
            if needs_arg_pack(&func.params) {
                emit_lua_arg_pack_struct(&mut out, &contract_struct, func, &ir.enums);
                out.push('\n');
            }
        }
    }
    out.push_str("]]) \n");
    // Emit enum tables (outside cdef — Lua tables, not C structs)
    for e in &ir.enums {
        if e.docs.is_some() || e.variants.iter().any(|variant| variant.docs.is_some()) {
            write_luals_docs(&mut out, "", e.docs.as_deref());
            out.push_str(&format!("---@enum {}\n", e.name));
            for variant in &e.variants {
                write_luals_docs(&mut out, "", variant.docs.as_deref());
                if variant.docs.is_some() {
                    out.push_str(&format!("---@field {} integer\n", variant.name));
                }
            }
        }
        generate_lua_enum(&mut out, e)?;
        out.push('\n');
    }
    for ty in &ir.types {
        if ty.docs.is_some() || ty.fields.iter().any(|field| field.docs.is_some()) {
            write_luals_docs(&mut out, "", ty.docs.as_deref());
            out.push_str(&format!("---@class {}\n", ty.name));
            for field in &ty.fields {
                write_luals_docs(&mut out, "", field.docs.as_deref());
                out.push_str(&format!(
                    "---@field {} {}\n",
                    field.name,
                    lua_host_type_annotation(&field.ty)
                ));
            }
        }
        out.push_str(&format!("ffi.metatype(\"{}\", {{}})\n", ty.name));
    }
    out.push_str("\nreturn {\n");
    for e in &ir.enums {
        out.push_str(&format!("    {} = {},\n", e.name, e.name));
    }
    out.push_str("}\n");
    Ok(out)
}

fn generate_host_callers_file(ir: &ValidatedIr) -> String {
    let mut out: String = String::new();
    out.push_str(file_header());
    out.push_str(&lua_require_block(&[&[("ffi", "ffi")]]));
    out.push('\n');
    out.push_str("local CTypeCache = {}\n");
    out.push_str("local function ctype(name)\n");
    out.push_str("    local cached = CTypeCache[name]\n");
    out.push_str("    if cached == nil then\n");
    out.push_str("        cached = ffi.typeof(name)\n");
    out.push_str("        CTypeCache[name] = cached\n");
    out.push_str("    end\n");
    out.push_str("    return cached\nend\n\n");

    // ABI constants for host
    out.push_str("-- ABI error codes (match polyplug_abi.AbiErrorCode)\n");
    out.push_str("local AbiErrorCode = {\n");
    out.push_str("    Ok = 0,\n");
    out.push_str("    Generic = 1,\n");
    out.push_str("    InvalidPointer = 8,\n");
    out.push_str("}\n\n");

    // GuestContractHandle is `#[repr(C)] { index: u32, generation: u32 }` (8 bytes).
    // The handle is opaque to generated code: it is passed straight to
    // resolve_guest_contract, which returns nil for an out-of-bounds, empty, or stale
    // handle. Generated callers therefore never inspect the handle's fields directly,
    // matching the Rust generator's resolve-then-check flow.

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

    // Cached FFI types for hot path performance.
    // Native guest dispatch functions receive the generated adapter context and
    // instance by value, then write AbiError through the trailing out-param.
    out.push_str("-- Cached FFI types for hot path performance\n");
    out.push_str(
        "local NativeDispatchFnType = ffi.typeof(\"void (*)(void*, GuestContractInstance, const void*, void*, AbiError*)\")\n",
    );
    out.push('\n');

    for contract in &ir.contracts {
        generate_host_contract_caller(&mut out, contract, &ir.enums);
        out.push('\n');
    }

    out.push_str("return M\n");
    out
}

fn generate_lua_internal_host_callers_file(ir: &ValidatedIr) -> String {
    let mut out: String = generate_host_callers_file(ir).replacen(
        "local ffi = require(\"ffi\")\n",
        "local ffi = require(\"ffi\")\n\
         local native_bridge = require(\"polyplug.loaders.lua\").internal_plugin_bridge()\n\
         local uintptr_t = ffi.typeof(\"uintptr_t\")\n\
         local function native_pointer(value)\n\
             if value == nil then return 0 end\n\
             if type(value) == \"number\" then return value end\n\
             return tonumber(ffi.cast(uintptr_t, ffi.cast(\"void *\", value)))\n\
         end\n",
        1,
    );
    let suffix: &str = "return M\n";
    assert!(
        out.ends_with(suffix),
        "Lua host caller module must return its exports"
    );
    out.truncate(out.len() - suffix.len());
    for contract in &ir.contracts {
        generate_lua_exact_handle_caller(&mut out, contract);
        out.push('\n');
    }
    out.push_str(suffix);
    out
}

fn generate_lua_exact_handle_caller(out: &mut String, contract: &ResolvedContract) {
    let contract_struct = contract_name_to_struct(&contract.name);
    out.push_str(&format!(
        "function M.{contract_struct}_create_from_handle(runtime, host, handle)\n"
    ));
    out.push_str("    local _ = runtime\n");
    out.push_str(
        "    local valid, handle_index, handle_generation, interface, cached_revision, factory = native_bridge.caller_resolve_from_handle(native_pointer(host), tonumber(handle.index), tonumber(handle.generation))\n",
    );
    out.push_str("    if valid == 0 then return nil end\n");
    out.push_str("    local factory_ok, implementation = pcall(factory, host)\n");
    out.push_str(
        "    if not factory_ok or type(implementation) ~= \"table\" then return nil end\n",
    );
    out.push_str("    local instance = native_bridge.caller_create_with_implementation(native_pointer(host), interface, implementation)\n");
    out.push_str("    if instance == 0 then return nil end\n");
    out.push_str("    local interface_ptr = ffi.cast(\"GuestContractInterface*\", ffi.cast(uintptr_t, interface))\n");
    out.push_str("    local instance_value = ffi.new(\"GuestContractInstance\")\n");
    out.push_str("    instance_value.data = ffi.cast(\"void*\", ffi.cast(uintptr_t, instance))\n");
    out.push_str("    instance_value.contract_id = interface_ptr.contract_id\n");
    out.push_str("    local retained_handle = ffi.new(\"GuestContractHandle\")\n");
    out.push_str("    retained_handle.index = handle_index\n");
    out.push_str("    retained_handle.generation = handle_generation\n");
    out.push_str("    local wrapper = {\n");
    out.push_str("        _interface = interface_ptr,\n");
    out.push_str("        _instance = instance_value,\n");
    out.push_str("        _host = host,\n");
    out.push_str("        _handle = retained_handle,\n");
    out.push_str("        _handle_index = handle_index,\n");
    out.push_str("        _handle_generation = handle_generation,\n");
    out.push_str("        _cached_revision = cached_revision,\n");
    out.push_str("        _destroyed = false\n");
    out.push_str("    }\n");
    out.push_str(
        "    function wrapper:is_valid()\n\
         \x20   return self._interface ~= nil and not self._destroyed\n\
         end\n\
         \n\
         \x20   function wrapper:destroy()\n\
         \x20       if self._interface ~= nil and not self._destroyed then\n\
         \x20           native_bridge.caller_destroy(native_pointer(self._host), self._handle_index, self._handle_generation, self._cached_revision, native_pointer(self._interface), native_pointer(self._instance.data))\n\
         \x20           self._destroyed = true\n\
         \x20       end\n\
         \x20   end\n\
         \n\
         \x20   function wrapper:reset()\n\
         \x20       local valid, raw_interface, raw_instance, revision = native_bridge.caller_reset(native_pointer(self._host), self._handle_index, self._handle_generation, self._cached_revision, native_pointer(self._interface), native_pointer(self._instance.data))\n\
         \x20       if valid == 0 then\n\
         \x20           self._interface = nil\n\
         \x20           self._destroyed = true\n\
         \x20           return\n\
         \x20       end\n\
         \x20       local interface = ffi.cast(\"GuestContractInterface*\", ffi.cast(uintptr_t, raw_interface))\n\
         \x20       local instance = ffi.new(\"GuestContractInstance\")\n\
         \x20       instance.data = ffi.cast(\"void*\", ffi.cast(uintptr_t, raw_instance))\n\
         \x20       instance.contract_id = interface.contract_id\n\
         \x20       self._interface = interface\n\
         \x20       self._instance = instance\n\
         \x20       self._cached_revision = revision\n\
         \x20       self._destroyed = false\n\
         \x20   end\n",
    );
    out.push_str(&format!(
        "    setmetatable(wrapper, {contract_struct}_mt)\n"
    ));
    out.push_str("    return wrapper\nend\n");
}

fn generate_guest_contracts_file(ir: &ValidatedIr) -> Result<String, PolyplugcError> {
    let mut out: String = String::new();
    out.push_str(file_header());
    out.push_str(&lua_require_block(&[&[
        ("ffi", "ffi"),
        ("polyplug_guest", "polyplug_guest"),
    ]]));
    out.push('\n');
    out.push_str("local M = {}\n\n");

    // The LuaLoader (Rust side) drives registration: after it execs the bundle
    // script and calls polyplug_init, it reads the per-contract registrations
    // table polyplug_init RETURNS (nothing is deposited into any global — Rule 12)
    // and builds the GuestContractInterface itself, wrapping each Lua handler in an
    // extern "C" trampoline. Guest code therefore NEVER constructs a
    // GuestContractInterface cdata or ffi.cast()s a Lua function into a
    // struct-returning C function pointer — LuaJIT cannot create callbacks for
    // function types that return a struct by value (e.g. GuestContractInstance,
    // StringView), so any such cast fails at load. We instead register pure Lua
    // handlers, mirroring tests/fixtures/test_plugin_lua/test_plugin.lua.
    //
    // Each handler has the low-level dispatch signature (instance, args_ptr,
    // out_ptr, arena_ptr, arena_alloc): `instance` is the resolved per-instance
    // impl object the loader passes as the first argument, args_ptr/out_ptr/arena_ptr
    // are i64 integers, and `arena_alloc(size, arena)` is the loader-supplied arena
    // allocator threaded as the final argument (see polyplug_lua::loader::lua_dispatch).
    // The generated wrapper marshals args/out around a method call ON the instance —
    // the loader owns per-instance state and builds each impl from the author factory
    // registered via `set_<plugin>_factory`.

    // Collect the (plugin, contract) pairs to register, preserving order.
    let mut registrations: Vec<(&str, &ResolvedContract)> = Vec::new();
    if let Some(bundle) = &ir.bundle {
        for plugin in &bundle.plugins {
            for contract_impl in &plugin.implements {
                if let Some(contract) = ir.contracts.iter().find(|c: &&ResolvedContract| {
                    let contract_full: String =
                        format!("{}@{}.{}", c.name, c.version.major, c.version.minor);
                    &contract_full == contract_impl
                }) {
                    generate_guest_plugin_interface(
                        &mut out,
                        &plugin.name,
                        contract,
                        &ir.enums,
                        &ir.types,
                    )?;
                    registrations.push((plugin.name.as_str(), contract));
                }
            }
        }
    }

    // Define the global polyplug_init the LuaLoader calls. It RETURNS the
    // per-contract registrations table (and an AbiError); nothing is deposited into
    // any global namespace (Rule 12). The example
    // guests require this module and call set_<plugin>_factory at module top
    // level, so the author factory is already stored by the time polyplug_init
    // runs; the loader calls it to build the default impl and each per-instance
    // impl.
    out.push_str("\n-- Registration entry point called by the LuaLoader.\n");
    out.push_str("-- Returns (registrations, abi_error): the per-contract handler table the\n");
    out.push_str(
        "-- loader consumes, plus the canonical AbiError ({ code, message }). Nothing is\n",
    );
    out.push_str(
        "-- deposited into any global/module namespace (Rule 12) — the loader reads BOTH\n",
    );
    out.push_str("-- return values. The host pointer threads to each author factory; no host\n");
    out.push_str("-- pointer or handler table is stored in this module.\n");
    // langprint renders the `function polyplug_init(host_ptr, ctx_ptr) … end` FORM;
    // the registration body below is built into `body` and handed back verbatim.
    let mut body: String = String::new();
    body.push_str("    if host_ptr == nil or ctx_ptr == nil then\n");
    body.push_str("        return {}, { code = polyplug_guest.AbiErrorCode.Generic, message = \"null host or ctx pointer in polyplug_init\" }\n");
    body.push_str("    end\n");
    body.push_str("    local registrations = {}\n");
    for (plugin_name, _contract) in &registrations {
        let plugin_var: String = plugin_name.to_uppercase().replace(['.', '-'], "_");
        let plugin_lower: String = plugin_name.to_lowercase().replace(['.', '-'], "_");
        // The author factory must have been registered at import time. Mirror
        // python: surface a Generic AbiError (not a raise) so the loader fails the
        // load cleanly through the return channel.
        body.push_str(&format!("    if {plugin_var}_FACTORY == nil then\n"));
        body.push_str(&format!(
            "        return {{}}, {{ code = polyplug_guest.AbiErrorCode.Generic, message = \"set_{plugin_lower}_factory(...) was not called at import time\" }}\n"
        ));
        body.push_str("    end\n");
        body.push_str(&format!("    M._register_{plugin_var}(registrations)\n"));
    }
    body.push_str("    return registrations, { code = polyplug_guest.AbiErrorCode.Ok }");
    out.push_str(&render_lua_defn_fn(
        "polyplug_init",
        vec!["host_ptr".to_owned(), "ctx_ptr".to_owned()],
        body,
    )?);
    out.push('\n');

    out.push_str("return M\n");
    Ok(out)
}

fn generate_lua_user_type(out: &mut String, ty: &ResolvedType, enums: &[EnumDef]) {
    out.push_str("    typedef struct {\n");
    for field in &ty.fields {
        let ty_name: String = lua_c_type_name(&field.ty, enums);
        out.push_str(&format!(
            "        {ty_name} {field_name};\n",
            field_name = field.name
        ));
    }
    out.push_str(&format!("    }} {};\n", ty.name));
}

/// Generate the full host caller for a contract with instance-based RAII pattern.
/// Creates methods table, metatable with __gc, and factory function.
fn generate_host_contract_caller(out: &mut String, contract: &ResolvedContract, enums: &[EnumDef]) {
    let contract_prefix: String = contract_name_to_prefix(&contract.name);
    let contract_struct: String = contract_name_to_struct(&contract.name);
    let contract_upper: String = contract.name.to_uppercase().replace(['.', '-'], "_");
    let contract_id_const: String = format!("{}_CONTRACT_ID", contract_upper);

    write_luals_docs(out, "", contract.docs.as_deref());
    // Methods table
    out.push_str(&format!(
        "-- Methods for {contract_struct} (instance wrapper)\n",
        contract_struct = contract_struct
    ));
    out.push_str(&format!(
        "local {contract_struct}_methods = {{\n",
        contract_struct = contract_struct
    ));

    // is_valid method - validity keys off the resolved interface pointer.
    // Stateless contracts return a null `instance.data` from create_instance and
    // use it as an opaque dispatch token, so instance data must NOT gate validity.
    out.push_str("    is_valid = function(self)\n");
    out.push_str("        return self._interface ~= nil and not self._destroyed\n");
    out.push_str("    end,\n\n");

    // live_revision reads the synchronized value through HostApi.
    out.push_str("    live_revision = function(self)\n");
    out.push_str("        return self._host.registry_revision(self._host)\n");
    out.push_str("    end,\n\n");

    // revalidate - the registry changed under us. Re-resolve via the retained handle:
    // a hot-reload swaps a new interface into the same slot, an unload vacates it, and
    // an unrelated registry revision leaves the exact interface unchanged. Retain the
    // current instance for the unchanged-interface case; replacing it would leak the
    // live instance and discard state. When the interface changed, the runtime already
    // reclaimed the old instance, so construct a fresh one without destroying it.
    out.push_str("    revalidate = function(self)\n");
    out.push_str("        if self._destroyed or self._interface == nil then\n");
    out.push_str("            return false\n");
    out.push_str("        end\n");
    out.push_str(
        "        local interface = self._host.resolve_guest_contract(self._host, self._handle)\n",
    );
    out.push_str("        if interface == nil then\n");
    out.push_str("            self._interface = nil\n");
    out.push_str("            self._instance = ffi.new(\"GuestContractInstance\")\n");
    out.push_str("            self._cached_revision = self:live_revision()\n");
    out.push_str("            self._destroyed = true\n");
    out.push_str("            return false\n");
    out.push_str("        end\n");
    out.push_str("        if interface == self._interface then\n");
    out.push_str("            self._cached_revision = self:live_revision()\n");
    out.push_str("            return true\n");
    out.push_str("        end\n");
    out.push_str("        local new_instance = ffi.new(\"GuestContractInstance\")\n");
    out.push_str(
        "        self._host.create_guest_instance(self._host, interface, nil, new_instance)\n",
    );
    out.push_str("        self._interface = interface\n");
    out.push_str("        self._instance = new_instance\n");
    out.push_str("        self._cached_revision = self:live_revision()\n");
    out.push_str("        self._destroyed = false\n");
    out.push_str("        return true\n");
    out.push_str("    end,\n\n");

    // destroy method - routes destruction through the host so the runtime drops the
    // instance from its live-instance accounting, then marks the wrapper destroyed.
    out.push_str("    destroy = function(self)\n");
    out.push_str("        if self._interface == nil or self._destroyed then\n");
    out.push_str("            return\n");
    out.push_str("        end\n");
    out.push_str("        if self:live_revision() ~= self._cached_revision then\n");
    out.push_str("            local interface = self._host.resolve_guest_contract(self._host, self._handle)\n");
    out.push_str("            if interface == nil or interface ~= self._interface then\n");
    out.push_str("                self._interface = nil\n");
    out.push_str("                self._instance = ffi.new(\"GuestContractInstance\")\n");
    out.push_str("                self._destroyed = true\n");
    out.push_str("                return\n");
    out.push_str("            end\n");
    out.push_str("        end\n");
    out.push_str("        local interface = self._interface\n");
    out.push_str("        local instance = self._instance\n");
    out.push_str("        self._interface = nil\n");
    out.push_str("        self._instance = ffi.new(\"GuestContractInstance\")\n");
    out.push_str("        self._destroyed = true\n");
    out.push_str("        self._host.destroy_guest_instance(self._host, interface, instance)\n");
    out.push_str("    end,\n\n");

    // reset method is destructive even when a registry revision belongs to an
    // unrelated bundle. Re-resolve once to avoid destroying an epoch-reclaimed
    // instance after a replacement or unload.
    out.push_str("    reset = function(self)\n");
    out.push_str("        if self._interface == nil or self._destroyed then return end\n");
    out.push_str("        local revision = self:live_revision()\n");
    out.push_str("        local interface = self._interface\n");
    out.push_str("        if revision ~= self._cached_revision then\n");
    out.push_str(
        "            interface = self._host.resolve_guest_contract(self._host, self._handle)\n",
    );
    out.push_str("            if interface == nil then\n");
    out.push_str("                self._interface = nil\n");
    out.push_str("                self._instance = ffi.new(\"GuestContractInstance\")\n");
    out.push_str("                self._cached_revision = revision\n");
    out.push_str("                self._destroyed = true\n");
    out.push_str("                return\n");
    out.push_str("            end\n");
    out.push_str("        end\n");
    out.push_str(
        "        if revision == self._cached_revision or interface == self._interface then\n",
    );
    out.push_str("            self._host.destroy_guest_instance(self._host, self._interface, self._instance)\n");
    out.push_str("        end\n");
    out.push_str("        local new_instance = ffi.new(\"GuestContractInstance\")\n");
    out.push_str(
        "        self._host.create_guest_instance(self._host, interface, nil, new_instance)\n",
    );
    out.push_str("        self._interface = interface\n");
    out.push_str("        self._instance = new_instance\n");
    out.push_str("        self._cached_revision = self:live_revision()\n");
    out.push_str("        self._destroyed = false\n");
    out.push_str("    end,\n\n");

    // Contract function methods - pass instance as first argument
    for func in &contract.functions {
        generate_host_caller_method(out, func, &contract_prefix, &contract_struct, enums);
        out.push_str(",\n\n");
    }

    out.push_str("}\n\n");

    // Metatable with __gc for automatic cleanup
    out.push_str(&format!(
        "-- Metatable for {contract_struct} with __gc cleanup\n",
        contract_struct = contract_struct
    ));
    out.push_str(&format!(
        "local {contract_struct}_mt = {{\n",
        contract_struct = contract_struct
    ));
    out.push_str(&format!(
        "    __index = {contract_struct}_methods,\n",
        contract_struct = contract_struct
    ));
    out.push_str("    __gc = function(self) self:destroy() end\n");
    out.push_str("}\n\n");

    // Factory function - resolves interface, creates instance
    out.push_str(&format!(
        "-- Factory function for {contract_struct} (instance wrapper)\n",
        contract_struct = contract_struct
    ));
    out.push_str(&format!(
        "function M.{contract_struct}_create(runtime, host)\n",
        contract_struct = contract_struct
    ));
    out.push_str(&format!(
        "    local handle = runtime:find_guest_contract({contract_id_const}, 0)\n"
    ));
    out.push_str("    -- The handle is opaque: pass it straight to resolve_guest_contract,\n");
    out.push_str("    -- which returns nil for an out-of-bounds, empty, or stale handle.\n");
    out.push_str("    local interface = runtime:resolve_guest_contract(handle)\n");
    out.push_str("    if interface == nil then\n");
    out.push_str("        return nil\n");
    out.push_str("    end\n");
    out.push_str(
        "    -- A null `instance.data` is valid: stateless contracts (and all VM-dispatch\n",
    );
    out.push_str(
        "    -- guests) return a null handle from create_instance and use it as an opaque\n",
    );
    out.push_str(
        "    -- dispatch token. Validity is keyed off the interface pointer, not the instance.\n",
    );
    out.push_str("    -- Route creation through the host so the runtime tracks the instance.\n");
    out.push_str("    -- create_guest_instance is an out-param ABI fn: (this, interface, args, out_instance) -> void.\n");
    out.push_str("    local instance = ffi.new(\"GuestContractInstance\")\n");
    out.push_str("    host.create_guest_instance(host, interface, nil, instance)\n");
    // Capture the synchronized revision for the resolved interface.
    out.push_str("    local cached_revision = host.registry_revision(host)\n");
    out.push_str("    local wrapper = {\n");
    out.push_str("        _interface = interface,\n");
    out.push_str("        _instance = instance,\n");
    out.push_str("        _host = host,\n");
    // Retain the opaque handle so revalidate() can re-resolve after a hot-reload
    // (same slot, new interface) or report a gone contract (slot vacated).
    out.push_str("        _handle = handle,\n");
    out.push_str("        _cached_revision = cached_revision,\n");
    out.push_str("        _destroyed = false\n");
    out.push_str("    }\n");
    out.push_str(&format!(
        "    setmetatable(wrapper, {contract_struct}_mt)\n",
        contract_struct = contract_struct
    ));
    out.push_str("    return wrapper\n");
    out.push_str("end\n");
}

/// Generate a single caller method for a contract function (instance-based).
fn generate_host_caller_method(
    out: &mut String,
    func: &ResolvedFunction,
    contract_prefix: &str,
    _contract_struct: &str,
    enums: &[EnumDef],
) {
    let fn_id: u32 = func.function_id;
    let sig_params: String = build_lua_sig_params(func);
    if func.docs.is_some()
        || func.params.iter().any(|param| param.docs.is_some())
        || func.return_docs.is_some()
    {
        write_luals_docs(out, "    ", func.docs.as_deref());
        out.push_str("    ---@param self table\n");
        for param in &func.params {
            if let Some(docs) = param.docs.as_deref() {
                write_luals_docs(out, "    ", Some(docs));
                out.push_str(&format!(
                    "    ---@param {} {} {}\n",
                    param.name,
                    lua_type_name(&param.ty),
                    docs.replace('\n', " ")
                ));
            }
        }
        if let Some(docs) = func.return_docs.as_deref() {
            write_luals_docs(out, "    ", Some(docs));
            let return_type: String = match &func.returns {
                Some(ty) => lua_type_name(ty),
                None => "nil".to_owned(),
            };
            out.push_str(&format!(
                "    ---@return {} {}\n",
                return_type,
                docs.replace('\n', " ")
            ));
        }
    }
    out.push_str(&format!("    {} = function(self{sig_params})\n", func.name));

    // Validity keys off the resolved interface pointer, NOT instance.data:
    // stateless and VM-dispatch guests carry a null instance handle.
    out.push_str("        if self._interface == nil or self._destroyed then\n");
    out.push_str("            error(\"invalid caller: interface is nil\", 2)\n");
    out.push_str("        end\n");
    // Cheap per-call staleness check: read the registry revision directly through
    // the cached pointer (one atomic load, no call into the runtime). While it
    // matches the value cached when this caller resolved, the cached interface
    // pointer is current and we dispatch directly; on any change (hot-reload or
    // unload) we re-resolve first, so the cached pointer is never used once it
    // dangles. A failed revalidate means the contract is gone.
    out.push_str(
        "        if self:live_revision() ~= self._cached_revision and not self:revalidate() then\n",
    );
    out.push_str("            error(\"invalid caller: interface is nil\", 2)\n");
    out.push_str("        end\n");

    // Setup args and out
    emit_lua_host_args_setup(out, func, contract_prefix, enums);
    emit_lua_host_out_setup(out, &func.returns, enums);

    // Dispatch on the interface's dispatch_type. Native guests (C++/Rust/native
    // Python) call the function pointer directly; VM guests (Lua, JS) route
    // through the loader's vm.call trampoline. Both return an AbiError by value.
    // DispatchType: 0 == Native, 1 == VirtualMachine.
    out.push_str(
        "        -- Out-param ABI: dispatch writes the AbiError through a trailing pointer.\n",
    );
    out.push_str("        local err = ffi.new(ctype(\"AbiError\"))\n");
    out.push_str("        if self._interface.dispatch_type == 0 then\n");
    // Function-id bounds check inside the Native arm only: on a VM interface
    // dispatch.native.function_count aliases bits of dispatch.vm.call through
    // the union (garbage). The VM-side loader enforces its own bounds
    // (FunctionNotAvailable).
    out.push_str(&format!(
        "            if {fn_id} >= self._interface.dispatch.native.function_count then\n"
    ));
    out.push_str("                error(\"function not available in interface\", 2)\n");
    out.push_str("            end\n");
    out.push_str(&format!(
        "            local fn_ptr = self._interface.dispatch.native.functions[{fn_id}]\n"
    ));
    out.push_str("            local fn = ffi.cast(NativeDispatchFnType, fn_ptr)\n");
    out.push_str(
        "            fn(self._interface.adapter_context, self._instance, args_ptr, out_ptr, err)\n",
    );
    out.push_str("        else\n");
    // The arena is nil: a Lua host caller cannot soundly hold a per-caller
    // CallArena (the 40-byte arena owns a borrowed primary buffer plus a host
    // overflow chain that must be reset between calls, which has no safe owner in
    // the LuaJIT FFI caller object). A null arena makes the guest bridge fall back
    // to per-value host->alloc — correct, just not zero-allocation. Native Rust/C++
    // hosts (rust.rs fn_needs_arena) carry real per-caller arenas.
    out.push_str(&format!(
        "            self._interface.dispatch.vm.call(self._interface.adapter_context, self._interface.dispatch.vm.loader_data, self._instance, {fn_id}, args_ptr, out_ptr, nil, err)\n"
    ));
    out.push_str("        end\n");
    out.push_str("        if err.code ~= AbiErrorCode.Ok then\n");
    out.push_str(
        "            error(\"polyplug call failed (code \" .. tonumber(err.code) .. \")\", 2)\n",
    );
    out.push_str("        end\n");

    if has_return_value(&func.returns) {
        out.push_str(&format!(
            "        return {}\n",
            lua_return_expr(&func.returns, enums)
        ));
    } else {
        out.push_str("        return nil\n");
    }
    out.push_str("    end");
}

/// Render the `M.set_<plugin>_factory(factory)` registration function via
/// langprint's Lua backend (the `function … end` shell is FORM; the single
/// assignment is the body slot). Byte-identical to the former hand-written form —
/// Lua output has no formatter, so langprint emits the exact bytes.
fn render_lua_set_factory(
    set_factory_name: &str,
    plugin_var: &str,
) -> Result<String, PolyplugcError> {
    let function: LuaFunction = LuaFunction {
        name: format!("M.{set_factory_name}"),
        parameters: vec!["factory".to_owned()],
        doc: None,
        body: Some(vec![format!("{plugin_var}_FACTORY = factory")]),
    };
    // polyplugc's Lua output indents 4, not the Lua-idiomatic 2.
    let backend: LuaBackend = LuaBackend {
        indent_size: 4,
        ..LuaBackend::default()
    };
    let mut indent_level: i32 = 0;
    backend
        .render_function(
            &function,
            None::<&str>,
            None::<&str>,
            None,
            &mut indent_level,
        )
        .map_err(|source: io::Error| PolyplugcError::WriteFailed {
            path: "guest/contracts.lua".to_owned(),
            source,
        })
}

/// Render a Lua function DEFINITION via langprint with a verbatim body: langprint
/// owns the `function name(params) … end` FORM; polyplugc owns the body, passed
/// as one verbatim String (exact whitespace + nested blocks baked in, no trailing
/// newline). Lua output has no formatter, so the body is emitted byte-for-byte.
fn render_lua_defn_fn(
    name: &str,
    parameters: Vec<String>,
    body: String,
) -> Result<String, PolyplugcError> {
    let function: LuaFunction = LuaFunction {
        name: name.to_owned(),
        parameters,
        doc: None,
        body: Some(vec![body]),
    };
    // polyplugc's Lua output indents 4, not the Lua-idiomatic 2.
    let backend: LuaBackend = LuaBackend {
        indent_size: 4,
        ..LuaBackend::default()
    };
    let options: LuaFunctionRenderOptions = LuaFunctionRenderOptions {
        render_doc: false,
        verbatim_body: true,
    };
    let mut indent_level: i32 = 0;
    backend
        .render_function(
            &function,
            None::<&str>,
            None::<&str>,
            Some(&options),
            &mut indent_level,
        )
        .map_err(|source: io::Error| PolyplugcError::WriteFailed {
            path: "guest/contracts.lua".to_owned(),
            source,
        })
}

fn generate_guest_plugin_interface(
    out: &mut String,
    plugin_name: &str,
    contract: &ResolvedContract,
    enums: &[EnumDef],
    types: &[ResolvedType],
) -> Result<(), PolyplugcError> {
    let plugin_var: String = plugin_name.to_uppercase().replace(['.', '-'], "_");
    let contract_name_full: String = format!("{}@{}", contract.name, contract.version.major);
    let plugin_lower: String = plugin_name.to_lowercase().replace(['.', '-'], "_");

    out.push_str(&format!(
        "-- Guest contract: {plugin_name} ({contract_name_full})\n"
    ));
    write_luals_docs(out, "", contract.docs.as_deref());
    for func in &contract.functions {
        let params: Vec<String> = func
            .params
            .iter()
            .map(|p: &ResolvedParam| format!("{}: {}", p.name, lua_type_name(&p.ty)))
            .collect();
        let ret_ty: String = match &func.returns {
            Some(ty) => lua_type_name(ty),
            None => "()".to_owned(),
        };
        write_luals_docs(out, "", func.docs.as_deref());
        for param in &func.params {
            if let Some(docs) = param.docs.as_deref() {
                write_luals_docs(out, "", Some(docs));
                out.push_str(&format!(
                    "---@param {} {} {}\n",
                    param.name,
                    lua_type_name(&param.ty),
                    docs.replace('\n', " ")
                ));
            }
        }
        if let Some(docs) = func.return_docs.as_deref() {
            write_luals_docs(out, "", Some(docs));
            out.push_str(&format!(
                "---@return {} {}\n",
                ret_ty,
                docs.replace('\n', " ")
            ));
        }
        out.push_str(&format!(
            "--   {fn_name}({}) -> {ret_ty}\n",
            params.join(", "),
            fn_name = func.name.replace('.', "_")
        ));
    }

    // Per-plugin storage for the author factory. The loader owns per-instance
    // state: it calls this factory once per create_instance (and once at load for
    // the stateless default impl). `factory(host_ptr) -> impl` returns an object
    // whose methods are the contract functions.
    out.push_str(&format!("local {plugin_var}_FACTORY = nil\n"));

    // set_<plugin>_factory(factory) registers the author factory. The author
    // calls this once at module import time; the loader reads it from the handler
    // entry and calls it to build each impl instance.
    let set_factory_name: String = format!("set_{plugin_lower}_factory");
    // The factory-registration function is FORM — langprint's Lua backend renders
    // the `function … end` shell; the single assignment is the body slot.
    out.push_str(&render_lua_set_factory(&set_factory_name, &plugin_var)?);

    // _register_<plugin>(registrations) builds the low-level dispatch handlers and
    // stores them under a per-contract entry in the `registrations` table that
    // polyplug_init returns to the loader (keyed by contract name) — no global.
    // The loader iterates every entry and registers one GuestContractInterface per
    // contract, so multi-contract bundles register ALL their contracts. Each handler
    // has the signature (instance, args_ptr, out_ptr, arena_ptr, arena_alloc):
    // `instance` is the resolved per-instance impl object the loader passes,
    // args_ptr/out_ptr/arena_ptr are i64 integers, and `arena_alloc(size, arena)` is
    // the loader-supplied arena allocator. The handler marshals inputs, invokes the
    // contract method ON the instance, and writes the result to out_ptr. The handler
    // entry also carries the author factory the loader calls to build each impl.
    // langprint renders the `function M._register_<plugin>(registrations) … end`
    // FORM; the handler-table body below is built into `body` and handed back verbatim.
    let mut body: String = String::new();
    body.push_str("    local functions = {}\n");
    for (idx, func) in contract.functions.iter().enumerate() {
        body.push_str(&format!(
            "    functions[{idx}] = function(instance, args_ptr, out_ptr, arena_ptr, arena_alloc)\n"
        ));
        emit_lua_guest_handler_body(&mut body, func, enums, &contract.name, types);
        body.push_str("    end\n");
    }
    body.push_str(&format!("    registrations[\"{}\"] = {{\n", contract.name));
    body.push_str(&format!(
        "        contract_version = {},\n",
        contract.version.major
    ));
    body.push_str(&format!("        plugin_name = \"{plugin_name}\",\n"));
    body.push_str(&format!("        factory = {plugin_var}_FACTORY,\n"));
    body.push_str("        functions = functions,\n");
    body.push_str("    }");
    out.push_str(&render_lua_defn_fn(
        &format!("M._register_{plugin_var}"),
        vec!["registrations".to_owned()],
        body,
    )?);
    out.push('\n');

    Ok(())
}

/// Emit the body of one low-level dispatch handler: marshal args from
/// `args_ptr`, call the contract method ON the resolved `instance`, marshal the
/// result to `out_ptr`. Pointers arrive as i64 integers (see lua_dispatch); the
/// `instance` is the per-instance impl object the loader passes as the handler's
/// first argument.
fn emit_lua_guest_handler_body(
    out: &mut String,
    func: &ResolvedFunction,
    enums: &[EnumDef],
    contract_name: &str,
    types: &[ResolvedType],
) {
    let method: String = func.name.replace('.', "_");
    // A missing instance or method must NOT fall through to success (the loader
    // treats a normal return as Ok, leaving a zeroed out-slot). Raising makes the
    // loader return AbiErrorCode.Generic to the caller.
    out.push_str(&format!(
        "        if instance == nil or instance.{method} == nil then error(\"polyplug: no implementation for {method}\") end\n"
    ));

    // Unpack the args pointer into typed values the impl receives, mirroring the
    // host caller's pack layout (emit_lua_host_args_setup): a single param is the
    // pointee of a typed slot; multiple params are fields of the cdef'd arg-pack
    // struct. The impl is invoked on the instance
    // (`instance:method(...)` == `instance.method(instance, ...)`), so it receives
    // the raw cdata/value per arg (StringView/Buffer/struct cdata, or a number for
    // scalars/enums) exactly as the host caller passed it.
    let call_args: String = if func.params.is_empty() {
        String::new()
    } else if func.params.len() == 1 {
        emit_lua_guest_unpack_single_arg(out, &func.params[0], enums)
    } else {
        let contract_struct: String = contract_name_to_struct(contract_name);
        let pack_struct: String = arg_pack_struct_name(&contract_struct, &func.name);
        out.push_str(&format!(
            "        local args_pack = ffi.cast(\"const {pack_struct}*\", ffi.cast(\"uintptr_t\", args_ptr))\n"
        ));
        func.params
            .iter()
            .map(|p: &ResolvedParam| {
                // Enum fields are repr integers in the pack; collapse to a Lua
                // number. Every other field (scalar/struct/StringView/Buffer) is
                // passed through as read from the pack.
                if lua_enum_repr_c_type(&p.ty, enums).is_some() {
                    format!("tonumber(args_pack[0].{})", p.name)
                } else {
                    format!("args_pack[0].{}", p.name)
                }
            })
            .collect::<Vec<String>>()
            .join(", ")
    };
    out.push_str(&format!(
        "        local result = instance:{method}({call_args})\n"
    ));

    emit_lua_guest_marshal_return(out, &func.returns, enums, types);
}

/// Unpack a single guest-handler argument from `args_ptr` and return the Lua
/// expression the impl is called with. Mirrors the single-param branch of
/// `emit_lua_host_args_setup`: the host passes the ADDRESS of a typed slot, so the
/// guest casts that address back to the matching pointer type and reads `[0]`.
fn emit_lua_guest_unpack_single_arg(
    out: &mut String,
    param: &ResolvedParam,
    enums: &[EnumDef],
) -> String {
    let addr: &str = "ffi.cast(\"uintptr_t\", args_ptr)";
    match &param.ty {
        ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => {
            out.push_str(&format!(
                "        local args_sv = ffi.cast(\"const StringView*\", {addr})\n"
            ));
            "args_sv[0]".to_owned()
        }
        ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => {
            out.push_str(&format!(
                "        local args_buf = ffi.cast(\"const Buffer*\", {addr})\n"
            ));
            "args_buf[0]".to_owned()
        }
        ResolvedTypeRef::UserDefined(_) => match lua_enum_repr_c_type(&param.ty, enums) {
            // Enum: the slot is a repr integer; hand the impl a plain Lua number.
            Some(repr) => {
                out.push_str(&format!(
                    "        local args_enum = ffi.cast(\"const {repr}*\", {addr})\n"
                ));
                "tonumber(args_enum[0])".to_owned()
            }
            // Struct: the slot is the cdef'd struct; hand the impl the struct cdata.
            None => {
                let struct_name: String = lua_type_name(&param.ty);
                out.push_str(&format!(
                    "        local args_struct = ffi.cast(\"const {struct_name}*\", {addr})\n"
                ));
                "args_struct[0]".to_owned()
            }
        },
        _ => {
            // Scalar / pointer: the slot is a 1-element array of the C type.
            let c_type: String = lua_type_name(&param.ty);
            out.push_str(&format!(
                "        local args_val = ffi.cast(\"const {c_type}*\", {addr})\n"
            ));
            "args_val[0]".to_owned()
        }
    }
}

/// Marshal the impl's `result` into `out_ptr`, covering EVERY return shape:
/// StringView/Buffer/struct are reference cdata written through a typed pointer;
/// scalars and enums are written through a repr-typed scalar slot. A nil result
/// for any non-void return raises (the loader maps the error to Generic) rather
/// than silently leaving a zeroed out-slot.
fn emit_lua_guest_marshal_return(
    out: &mut String,
    returns: &Option<ResolvedTypeRef>,
    enums: &[EnumDef],
    types: &[ResolvedType],
) {
    let Some(ret) = returns else {
        return;
    };
    match ret {
        ResolvedTypeRef::AbiType(AbiBuiltin::Void) => {}
        ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => {
            emit_lua_guest_marshal_string_return(out);
        }
        ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => {
            emit_lua_guest_marshal_ref_return(out, "Buffer");
        }
        _ => match lua_enum_repr_c_type(ret, enums) {
            // Enum return: repr-integer scalar slot.
            Some(repr) => emit_lua_guest_marshal_scalar_return(out, &repr),
            None if lua_return_is_scalar(ret) => {
                emit_lua_guest_marshal_scalar_return(out, &lua_type_name(ret));
            }
            // Array-wrapper (`ArrayOf_T`) or struct-by-value: the impl returns an
            // ergonomic Lua value (an array of tables / a table) and the generated
            // glue marshals it into the caller's arena field-by-field.
            None => emit_lua_guest_marshal_composite_return(out, &lua_type_name(ret), types, enums),
        },
    }
}

/// Marshal an array-wrapper or struct return: the impl returns a plain Lua value
/// (an array of tables for `ArrayOf_T`, or a table for a struct) and this glue
/// bump-allocates the elements + their embedded strings into the caller's arena.
/// A nil result raises (the loader maps it to Generic).
fn emit_lua_guest_marshal_composite_return(
    out: &mut String,
    c_type: &str,
    types: &[ResolvedType],
    enums: &[EnumDef],
) {
    out.push_str("        if out_ptr ~= 0 and result == nil then\n");
    out.push_str(&format!(
        "            error(\"polyplug: implementation returned nil for a {c_type}-returning function\")\n"
    ));
    out.push_str("        end\n");
    out.push_str("        if out_ptr ~= 0 then\n");
    out.push_str(&format!(
        "            local out_ref = ffi.cast(\"{c_type}*\", ffi.cast(\"uintptr_t\", out_ptr))\n"
    ));
    let mut uid: usize = 0;
    let ctx: LuaMarshalCtx = LuaMarshalCtx { types, enums };
    emit_lua_marshal_into(
        out,
        "out_ref[0]",
        "result",
        c_type,
        &ctx,
        "            ",
        &mut uid,
    );
    out.push_str("        end\n");
}

/// Recursively marshal the Lua value `src` into the cdata lvalue `dest`, which has
/// C type named `c_type`. Scalars/enums copy directly; `StringView` fields
/// arena-allocate their bytes; nested structs recurse field-by-field; array
/// wrappers (`ArrayOf_T`) allocate `#src` elements in the arena and recurse per
/// element. `uid` names loop-local temporaries uniquely across nesting levels.
/// Type context threaded through the recursive Lua marshaler: the struct type
/// table (to marshal a struct element/field by field) plus the enum table
/// (enums have no cdef'd C type, so they resolve to their repr C integer type).
struct LuaMarshalCtx<'a> {
    types: &'a [ResolvedType],
    enums: &'a [EnumDef],
}

fn emit_lua_marshal_into(
    out: &mut String,
    dest: &str,
    src: &str,
    c_type: &str,
    ctx: &LuaMarshalCtx,
    indent: &str,
    uid: &mut usize,
) {
    if let Some(element) = array_element_name(c_type) {
        emit_lua_marshal_array_into(out, dest, src, element, ctx, indent, uid);
        return;
    }
    // A `StringView` element/value: the impl produced a plain Lua string, so
    // arena-allocate its bytes and store the resulting view (a direct `dest = src`
    // would try to assign a Lua string into a `StringView` cdata, which LuaJIT
    // rejects). StringView *fields* are handled in `emit_lua_marshal_field_into`;
    // this covers a StringView *array element* (`Array<StringView>`).
    if c_type == "StringView" {
        out.push_str(&format!(
            "{indent}{dest} = polyplug_guest.alloc_string_arena(arena_alloc, arena_ptr, {src})\n"
        ));
        return;
    }
    // A struct in the type table: marshal each field into the destination cdata.
    if let Some(ty) = ctx
        .types
        .iter()
        .find(|t: &&ResolvedType| t.name.as_str() == c_type)
    {
        for field in &ty.fields {
            emit_lua_marshal_field_into(out, dest, src, field, ctx, indent, uid);
        }
        return;
    }
    // Fallback: a scalar/enum (a Lua number written into a repr-typed cdata slot)
    // or a bare cdata the impl already produced (e.g. Buffer) — copy it directly.
    out.push_str(&format!("{indent}{dest} = {src}\n"));
}

/// Marshal one struct field `field` of `src` into `dest.<field>`.
fn emit_lua_marshal_field_into(
    out: &mut String,
    dest: &str,
    src: &str,
    field: &ResolvedField,
    ctx: &LuaMarshalCtx,
    indent: &str,
    uid: &mut usize,
) {
    let dest_field: String = format!("{dest}.{}", field.name);
    let src_field: String = format!("{src}.{}", field.name);
    match &field.ty {
        ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => {
            out.push_str(&format!(
                "{indent}{dest_field} = polyplug_guest.alloc_string_arena(arena_alloc, arena_ptr, {src_field})\n"
            ));
        }
        // A nested struct or array field recurses into its own marshaling.
        ResolvedTypeRef::UserDefined(name)
            if array_element_name(name).is_some()
                || ctx
                    .types
                    .iter()
                    .any(|t: &ResolvedType| t.name.as_str() == name.as_str()) =>
        {
            emit_lua_marshal_into(out, &dest_field, &src_field, name, ctx, indent, uid);
        }
        // Scalars, enums (an integer the impl returns as a number), and Buffer/Ptr
        // copy directly into the repr-typed cdata field.
        _ => {
            out.push_str(&format!("{indent}{dest_field} = {src_field}\n"));
        }
    }
}

/// Marshal a Lua array `src` into the `ArrayOf_<element>` wrapper lvalue `dest`:
/// allocate `#src` elements in the arena (`arena_alloc` is align-1, so over-
/// allocate and round the base up to the element alignment), fill each, and set
/// `items`/`len`.
fn emit_lua_marshal_array_into(
    out: &mut String,
    dest: &str,
    src: &str,
    element: &str,
    ctx: &LuaMarshalCtx,
    indent: &str,
    uid: &mut usize,
) {
    let id: usize = *uid;
    *uid += 1;
    let n: String = format!("n{id}");
    let raw: String = format!("raw{id}");
    let base: String = format!("base{id}");
    let align: String = format!("al{id}");
    let elems: String = format!("elems{id}");
    let i: String = format!("i{id}");
    let item: String = format!("it{id}");
    // The FFI C type of the element: a primitive maps to its C integer/float name
    // (`u32` → `uint32_t`), an enum to its repr's C integer (`Status` → `uint32_t`)
    // since enums have no cdef'd C type, and StringView/Buffer/structs are cdef'd
    // under their own name. Using the raw polyplug type name here would emit an
    // `ffi.sizeof("u32")` / `ffi.sizeof("Status")` that LuaJIT cannot resolve.
    let cname: String = lua_c_type_name(&lua_element_type_ref(element), ctx.enums);
    out.push_str(&format!("{indent}local {n} = #{src}\n"));
    out.push_str(&format!("{indent}if {n} == 0 then\n"));
    out.push_str(&format!("{indent}    {dest}.items = 0\n"));
    out.push_str(&format!("{indent}    {dest}.len = 0\n"));
    out.push_str(&format!("{indent}else\n"));
    let inner: String = format!("{indent}    ");
    out.push_str(&format!(
        "{inner}local {align} = ffi.alignof(\"{cname}\")\n"
    ));
    out.push_str(&format!(
        "{inner}local {raw} = ffi.cast(\"uintptr_t\", arena_alloc({n} * ffi.sizeof(\"{cname}\") + {align} - 1, arena_ptr))\n"
    ));
    out.push_str(&format!(
        "{inner}local {base} = ({raw} + {align} - 1) - (({raw} + {align} - 1) % {align})\n"
    ));
    out.push_str(&format!(
        "{inner}local {elems} = ffi.cast(\"{cname}*\", {base})\n"
    ));
    out.push_str(&format!("{inner}for {i} = 0, {n} - 1 do\n"));
    out.push_str(&format!("{inner}    local {item} = {src}[{i} + 1]\n"));
    emit_lua_marshal_into(
        out,
        &format!("{elems}[{i}]"),
        &item,
        element,
        ctx,
        &format!("{inner}    "),
        uid,
    );
    out.push_str(&format!("{inner}end\n"));
    out.push_str(&format!(
        "{inner}{dest}.items = ffi.cast(\"uint64_t\", {base})\n"
    ));
    out.push_str(&format!("{inner}{dest}.len = {n}\n"));
    out.push_str(&format!("{indent}end\n"));
}

/// Marshal a `StringView` return: the impl returns a plain Lua string and the
/// GENERATED handler arena-allocates it into the caller's out slot via the
/// threaded `arena_alloc`/`arena_ptr` (no per-VM global, no author-side arena —
/// mirrors the python reference). A nil result raises (loader maps to Generic).
fn emit_lua_guest_marshal_string_return(out: &mut String) {
    out.push_str("        if out_ptr ~= 0 and result == nil then\n");
    out.push_str(
        "            error(\"polyplug: implementation returned nil for a StringView-returning function\")\n",
    );
    out.push_str("        end\n");
    out.push_str("        if out_ptr ~= 0 then\n");
    out.push_str(
        "            local out_ref = ffi.cast(\"StringView*\", ffi.cast(\"uintptr_t\", out_ptr))\n",
    );
    out.push_str(
        "            out_ref[0] = polyplug_guest.alloc_string_arena(arena_alloc, arena_ptr, result)\n",
    );
    out.push_str("        end\n");
}

/// Marshal a reference-cdata return (Buffer/struct): the impl returns a
/// cdata of `c_type`; copy it into the caller's out slot. A nil result raises.
fn emit_lua_guest_marshal_ref_return(out: &mut String, c_type: &str) {
    out.push_str("        if out_ptr ~= 0 and result ~= nil then\n");
    out.push_str(&format!(
        "            local out_ref = ffi.cast(\"{c_type}*\", ffi.cast(\"uintptr_t\", out_ptr))\n"
    ));
    out.push_str("            out_ref[0] = result\n");
    out.push_str("        end\n");
    out.push_str("        if out_ptr ~= 0 and result == nil then\n");
    out.push_str(&format!(
        "            error(\"polyplug: implementation returned nil for a {c_type}-returning function\")\n"
    ));
    out.push_str("        end\n");
}

/// Marshal a scalar (primitive or enum) return: the impl returns a plain Lua
/// number/boolean; write it through a `c_type`-typed pointer over the out slot.
fn emit_lua_guest_marshal_scalar_return(out: &mut String, c_type: &str) {
    out.push_str("        if out_ptr ~= 0 and result == nil then\n");
    out.push_str(&format!(
        "            error(\"polyplug: implementation returned nil for a {c_type}-returning function\")\n"
    ));
    out.push_str("        end\n");
    out.push_str("        if out_ptr ~= 0 then\n");
    out.push_str(&format!(
        "            local out_scalar = ffi.cast(\"{c_type}*\", ffi.cast(\"uintptr_t\", out_ptr))\n"
    ));
    out.push_str("            out_scalar[0] = result\n");
    out.push_str("        end\n");
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

fn emit_lua_host_args_setup(
    out: &mut String,
    func: &ResolvedFunction,
    contract_prefix: &str,
    enums: &[EnumDef],
) {
    if func.params.is_empty() {
        out.push_str("    local args_ptr = nil\n");
        return;
    }
    if func.params.len() == 1 {
        let param: &ResolvedParam = &func.params[0];
        match &param.ty {
            ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => {
                // Accept a plain Lua string; marshal into a StringView (ptr + len)
                // over a kept-alive byte buffer that outlives the dispatch call.
                out.push_str(&format!(
                    "    local {name}_bytes = tostring({name})\n",
                    name = param.name
                ));
                out.push_str(&format!(
                    "    local {name}_view = ffi.new(\"StringView\")\n",
                    name = param.name
                ));
                out.push_str(&format!(
                    "    {name}_view.ptr = ffi.cast(\"const uint8_t*\", {name}_bytes)\n",
                    name = param.name
                ));
                out.push_str(&format!(
                    "    {name}_view.len = #{name}_bytes\n",
                    name = param.name
                ));
                out.push_str(&format!(
                    "    local args_ptr = ffi.cast(\"const void*\", {name}_view)\n",
                    name = param.name
                ));
            }
            ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => {
                out.push_str(&format!(
                    "    local {name}_bytes = tostring({name})\n",
                    name = param.name
                ));
                out.push_str(&format!(
                    "    local {name}_buf = ffi.new(\"Buffer\")\n",
                    name = param.name
                ));
                out.push_str(&format!(
                    "    {name}_buf.ptr = ffi.cast(\"void*\", {name}_bytes)\n",
                    name = param.name
                ));
                out.push_str(&format!(
                    "    {name}_buf.len = #{name}_bytes\n",
                    name = param.name
                ));
                out.push_str(&format!(
                    "    local args_ptr = ffi.cast(\"const void*\", {name}_buf)\n",
                    name = param.name
                ));
            }
            ResolvedTypeRef::UserDefined(_) => {
                match lua_enum_repr_c_type(&param.ty, enums) {
                    // Enum: the value is a plain Lua number. Write it into a
                    // repr-integer slot and pass the SLOT's address — casting
                    // the bare number to void* would make the enum VALUE the
                    // address (same class the factory-side fix removed).
                    Some(repr) => {
                        out.push_str(&format!(
                            "    local {name}_val = ffi.new(\"{repr}[1]\", {name})\n",
                            name = param.name
                        ));
                        out.push_str(&format!(
                            "    local args_ptr = ffi.cast(\"const void*\", {name}_val)\n",
                            name = param.name
                        ));
                    }
                    // Struct: a cdef'd struct cdata is a reference cdata, so
                    // the cast yields its address.
                    None => {
                        out.push_str(&format!(
                            "    local args_ptr = ffi.cast(\"const void*\", {} )\n",
                            param.name
                        ));
                    }
                }
            }
            _ => {
                // Scalar/pointer params need a 1-element array slot for the same
                // reason as scalar out slots (see lua_return_is_scalar): a scalar
                // ffi.new("T", v) is a VALUE cdata and ffi.cast("void*", value)
                // converts the value instead of taking its address.
                let ty_name: String = lua_type_name(&param.ty);
                out.push_str(&format!(
                    "    local {name}_val = ffi.new(\"{ty}[1]\", {name})\n",
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

fn emit_lua_host_out_setup(out: &mut String, returns: &Option<ResolvedTypeRef>, enums: &[EnumDef]) {
    if !has_return_value(returns) {
        out.push_str("    local out_ptr = nil\n");
        return;
    }
    // Enum returns: the out slot is the enum's repr C integer type (the enum
    // itself has no cdef'd C type), as a 1-element array like other scalars.
    let enum_repr: Option<String> = match returns {
        Some(ret) => lua_enum_repr_c_type(ret, enums),
        None => None,
    };
    if let Some(repr) = enum_repr {
        out.push_str(&format!(
            "    local out_val = ffi.new(ctype(\"{repr}[1]\"))\n"
        ));
        out.push_str("    local out_ptr = ffi.cast(ctype(\"void*\"), out_val)\n");
        return;
    }
    let ret_ty: String = match returns {
        Some(ret) => lua_type_name(ret),
        None => "void".to_owned(),
    };
    let is_scalar: bool = matches!(returns, Some(ret) if lua_return_is_scalar(ret));
    if is_scalar {
        out.push_str(&format!(
            "    local out_val = ffi.new(ctype(\"{ret_ty}[1]\"))\n"
        ));
    } else {
        out.push_str(&format!(
            "    local out_val = ffi.new(ctype(\"{ret_ty}\"))\n"
        ));
    }
    out.push_str("    local out_ptr = ffi.cast(ctype(\"void*\"), out_val)\n");
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

/// LuaJIT represents primitives and raw pointers as *value* cdata. A value cdata
/// cannot serve as an out-pointer (`ffi.cast("void*", value)` reinterprets the
/// value and yields NULL), so a scalar out slot must be a 1-element array (a
/// reference cdata whose cast yields its address) and the result is read with
/// `out_val[0]` — which also produces a native Lua number/boolean instead of a
/// value cdata. Struct/StringView/Buffer returns are already reference cdata.
fn lua_return_is_scalar(ty: &ResolvedTypeRef) -> bool {
    matches!(
        ty,
        ResolvedTypeRef::Primitive(_) | ResolvedTypeRef::AbiType(AbiBuiltin::Ptr)
    )
}

fn lua_return_expr(returns: &Option<ResolvedTypeRef>, enums: &[EnumDef]) -> String {
    match returns {
        // Enum out slots are repr-integer arrays; tonumber() collapses any
        // boxed 64-bit cdata element into a plain Lua number.
        Some(ret) if lua_enum_repr_c_type(ret, enums).is_some() => {
            "tonumber(out_val[0])".to_owned()
        }
        Some(ret) if lua_return_is_scalar(ret) => "out_val[0]".to_owned(),
        _ => "out_val".to_owned(),
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

/// Parse an array-element type NAME (extracted from `ArrayOf_<element>`) back
/// into a `ResolvedTypeRef` so it can be mapped to a LuaJIT C type. Primitives
/// and ABI builtins are recognised by name; anything else is a user-defined
/// struct or enum.
fn lua_element_type_ref(name: &str) -> ResolvedTypeRef {
    if let Some(p) = PrimitiveType::parse(name) {
        ResolvedTypeRef::Primitive(p)
    } else if let Some(b) = AbiBuiltin::parse(name) {
        ResolvedTypeRef::AbiType(b)
    } else {
        ResolvedTypeRef::UserDefined(name.to_owned())
    }
}

/// C type name for cdef / ffi.cast / ffi.new emission. Contract ENUMS have no
/// cdef'd C type (the generator emits them as plain Lua tables), so they map
/// to their repr's C integer type — naming the enum directly only ever worked
/// when it collided with an identically named ABI cdef (e.g. `LogLevel`).
fn lua_c_type_name(ty: &ResolvedTypeRef, enums: &[EnumDef]) -> String {
    match ty {
        ResolvedTypeRef::UserDefined(name) => {
            match enums.iter().find(|e: &&EnumDef| &e.name == name) {
                Some(e) => e.repr.cpp_name().to_owned(),
                None => name.clone(),
            }
        }
        _ => lua_type_name(ty),
    }
}

/// Resolve `ty` to its enum repr C integer type name when it names a contract
/// enum. Caller-side marshalling needs this distinction: an enum value is a
/// plain Lua NUMBER (the generator emits enums as Lua tables, not cdefs), so it
/// must travel through a repr-typed slot — while a non-enum `UserDefined` is a
/// struct cdata that already carries its own address.
fn lua_enum_repr_c_type(ty: &ResolvedTypeRef, enums: &[EnumDef]) -> Option<String> {
    match ty {
        ResolvedTypeRef::UserDefined(name) => enums
            .iter()
            .find(|e: &&EnumDef| &e.name == name)
            .map(|e: &EnumDef| e.repr.cpp_name().to_owned()),
        _ => None,
    }
}

fn emit_lua_arg_pack_struct(
    out: &mut String,
    contract_struct: &str,
    func: &ResolvedFunction,
    enums: &[EnumDef],
) {
    let struct_name: String = arg_pack_struct_name(contract_struct, &func.name);
    out.push_str("    typedef struct {\n");
    for param in &func.params {
        let ty_name: String = lua_c_type_name(&param.ty, enums);
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

fn generate_lua_enum(out: &mut String, e: &EnumDef) -> Result<(), PolyplugcError> {
    let doc: String = if e.bitflag {
        format!("Bitflag enum {}", e.name)
    } else {
        format!("Enum {}", e.name)
    };
    let lua_enum: LuaEnum = LuaEnum {
        name: e.name.clone(),
        members: e
            .variants
            .iter()
            .map(|variant| {
                let subst_value: String = substitute_variant_refs_lua(&e.variants, &variant.value);
                LuaEnumMember {
                    name: variant.name.clone(),
                    value: lua_transform_value_expr(&subst_value),
                }
            })
            .collect(),
        doc: Some(doc),
    };
    // polyplug Lua output is 4-space indented, not the langprint default of 2.
    let backend: LuaBackend = LuaBackend {
        indent_size: 4,
        ..LuaBackend::default()
    };
    let mut indent_level: i32 = 0;
    let rendered: String = backend
        .render_enum(
            &lua_enum,
            None::<&str>,
            None::<&str>,
            None,
            &mut indent_level,
        )
        .map_err(|source: io::Error| PolyplugcError::WriteFailed {
            path: "types.lua".to_owned(),
            source,
        })?;
    out.push_str(&rendered);
    Ok(())
}

// ─── Host Contract Metatable Generation ────────────────────────────────────────

/// Convert host contract name to Lua class name.
/// e.g. "host.logger" -> "HostLogger", "host.fs.reader" -> "HostFsReader"
fn host_contract_name_to_lua_class(name: &str) -> String {
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
        format!("Host{}", pascal)
    }
}

/// Convert host contract name to Lua guest caller class name.
/// e.g. "host.logger" -> "HostLoggerContract", "host.fs.reader" -> "HostFsReaderContract"
fn host_contract_name_to_lua_caller(name: &str) -> String {
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
        format!("Host{}Contract", pascal)
    }
}

/// Generate Lua type annotation for host contract method parameters.
/// Generate the Lua docstring type annotation for a host contract method's
/// params and returns. The mapping is identical for both positions — Lua's
/// annotation type universe (number / string / userdata / nil) does not
/// distinguish param vs return spellings:
/// - StringView -> string
/// - Buffer -> string (Lua uses strings for byte buffers)
/// - UserDefined -> userdata
/// - Primitives -> number (Lua's numeric type)
fn lua_host_type_annotation(ty: &ResolvedTypeRef) -> String {
    match ty {
        ResolvedTypeRef::Primitive(_) => "number".to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => "string".to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => "string".to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::Ptr) => "userdata".to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::Void) => "nil".to_owned(),
        ResolvedTypeRef::UserDefined(_) => "userdata".to_owned(),
    }
}

/// Generate the metatable definition for one host contract.
fn generate_lua_host_contract_metatable(out: &mut String, contract: &ResolvedHostContract) {
    let class_name: String = host_contract_name_to_lua_class(&contract.name);

    out.push_str(&format!(
        "-- Host contract `{}` (id=0x{:016X})\n",
        contract.name, contract.contract_id
    ));
    write_luals_docs(out, "", contract.docs.as_deref());
    out.push_str(&format!("{} = {{}}\n", class_name));
    out.push_str(&format!("{}.__index = {}\n\n", class_name, class_name));

    out.push_str(&format!("--- @return {}\n", class_name));
    out.push_str(&format!("function {}:new()\n", class_name));
    out.push_str("    local obj = {}\n");
    out.push_str("    setmetatable(obj, self)\n");
    out.push_str("    return obj\n");
    out.push_str("end\n\n");

    for func in &contract.functions {
        let return_type: String = match &func.returns {
            Some(ty) => lua_host_type_annotation(ty),
            None => "nil".to_owned(),
        };

        write_luals_docs(out, "", func.docs.as_deref());
        out.push_str("--- @param self table\n");
        for param in &func.params {
            if let Some(docs) = param.docs.as_deref() {
                write_luals_docs(out, "", Some(docs));
                out.push_str(&format!(
                    "---@param {} {} {}\n",
                    param.name,
                    lua_host_type_annotation(&param.ty),
                    docs.replace('\n', " ")
                ));
            } else {
                out.push_str(&format!(
                    "---@param {} {}\n",
                    param.name,
                    lua_host_type_annotation(&param.ty)
                ));
            }
        }
        if let Some(docs) = func.return_docs.as_deref() {
            write_luals_docs(out, "", Some(docs));
            out.push_str(&format!(
                "---@return {} {}\n",
                return_type,
                docs.replace('\n', " ")
            ));
        } else {
            out.push_str(&format!("--- @return {}\n", return_type));
        }
        out.push_str(&format!(
            "function {}:{}({})\n",
            class_name,
            func.name,
            if func.params.is_empty() {
                "self".to_owned()
            } else {
                func.params
                    .iter()
                    .map(|p: &ResolvedParam| p.name.clone())
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        ));
        out.push_str(&format!(
            "    error(\"abstract method: {} must be implemented by host\", 2)\n",
            func.name
        ));
        out.push_str("end\n\n");
    }

    out.push('\n');
}

/// Generate `host/contracts.lua` — metatables for each host contract.
fn generate_host_contracts_file(ir: &ValidatedIr) -> String {
    let mut out: String = String::new();
    out.push_str(file_header());
    out.push_str("local M = {}\n\n");

    for contract in &ir.host_contracts {
        generate_lua_host_contract_metatable(&mut out, contract);
    }

    out.push_str("-- Contract ID constants\n");
    for contract in &ir.host_contracts {
        let class_name: String = host_contract_name_to_lua_class(&contract.name);
        let const_name: String = format!("{}_CONTRACT_ID", class_name.to_uppercase());
        out.push_str(&format!(
            "M.{} = 0x{:016X}ULL\n",
            const_name, contract.contract_id
        ));
    }
    out.push('\n');

    out.push_str("-- Export host contract classes\n");
    for contract in &ir.host_contracts {
        let class_name: String = host_contract_name_to_lua_class(&contract.name);
        out.push_str(&format!("M.{} = {}\n", class_name, class_name));
    }
    out.push('\n');

    out.push_str("return M\n");
    out
}

// ─── Guest Host Contract Caller Generation ─────────────────────────────────────

/// Generate one guest-side host contract caller class.
fn generate_lua_guest_host_contract_caller(
    out: &mut String,
    contract: &ResolvedHostContract,
    enums: &[EnumDef],
) -> Result<(), PolyplugcError> {
    let class_name: String = host_contract_name_to_lua_caller(&contract.name);

    out.push_str(&format!(
        "-- Guest caller for host contract `{}` (id=0x{:016X})\n",
        contract.name, contract.contract_id
    ));
    write_luals_docs(out, "", contract.docs.as_deref());
    out.push_str(&format!("{} = {{}}\n", class_name));
    out.push_str(&format!("{}.__index = {}\n\n", class_name, class_name));

    // langprint renders each `function Class:method(...) … end` FORM (the colon/dot
    // is part of the name); the bodies are verbatim.
    out.push_str(&render_lua_defn_fn(
        &format!("{class_name}:new"),
        vec!["interface".to_owned(), "instance".to_owned()],
        "    local obj = { _interface = interface, _instance = instance }\n    setmetatable(obj, self)\n    return obj".to_owned(),
    )?);
    out.push('\n');

    // `host_ptr` is the threaded host pointer (a plain Lua number), passed in by
    // the caller — no per-VM global (Rule 12). Cast through uintptr_t first, exactly
    // like the host-side caller path; mirrors the canonical Rust host-contract caller.
    let mut from_host_body: String = String::new();
    from_host_body.push_str("    if min_version == nil then min_version = 0 end\n");
    from_host_body.push_str("    if host_ptr == nil then\n");
    from_host_body.push_str("        return nil\n");
    from_host_body.push_str("    end\n");
    from_host_body
        .push_str("    local host = ffi.cast(\"HostApi*\", ffi.cast(\"uintptr_t\", host_ptr))\n");
    from_host_body.push_str(&format!(
        "    local interface_ptr = host.resolve_host_contract_interface(host, 0x{:016X}ULL, min_version)\n",
        contract.contract_id
    ));
    from_host_body.push_str("    if interface_ptr == nil then\n");
    from_host_body.push_str("        return nil\n");
    from_host_body.push_str("    end\n");
    from_host_body.push_str(&format!(
        "    local instance = host.get_host_contract(host, 0x{:016X}ULL, min_version)\n",
        contract.contract_id
    ));
    from_host_body.push_str(&format!(
        "    return {}:new(interface_ptr, instance)",
        class_name
    ));
    out.push_str(&render_lua_defn_fn(
        &format!("{class_name}.from_host"),
        vec!["host_ptr".to_owned(), "min_version".to_owned()],
        from_host_body,
    )?);
    out.push('\n');

    out.push_str(&render_lua_defn_fn(
        &format!("{class_name}:is_valid"),
        Vec::new(),
        "    return self._interface ~= nil".to_owned(),
    )?);
    out.push('\n');

    for func in &contract.functions {
        generate_lua_guest_host_contract_method(out, func, &class_name, enums)?;
    }

    out.push('\n');
    Ok(())
}

/// Generate one method for a guest-side host contract caller.
fn generate_lua_guest_host_contract_method(
    dst: &mut String,
    func: &ResolvedFunction,
    class_name: &str,
    enums: &[EnumDef],
) -> Result<(), PolyplugcError> {
    let fn_id: u32 = func.function_id;
    let has_return: bool = func.returns.is_some();

    // Colon-method syntax (`Class:method`) already binds an implicit `self`, so the
    // parameter list must NOT re-declare it. Emitting `:method(self, ...)` shifts
    // every real argument by one (the caller's first arg lands in the redundant
    // `self` slot and the last real parameter becomes nil) — the bug that silently
    // dropped the message a guest passed to host.logger:log(). langprint renders the
    // `function Class:method(params) … end` FORM (the colon+name is the name); the
    // body is verbatim.
    let parameters: Vec<String> = func
        .params
        .iter()
        .map(|p: &ResolvedParam| p.name.clone())
        .collect::<Vec<String>>();

    // The body is accumulated into `body` (aliased as `out` so the existing
    // push_str lines below are unchanged), then rendered as the verbatim slot.
    let mut body: String = String::new();
    let out: &mut String = &mut body;

    out.push_str("    if self._interface == nil then\n");
    if has_return {
        out.push_str("        return nil\n");
    } else {
        out.push_str("        return\n");
    }
    out.push_str("    end\n");

    // The resolved interface is a flat `HostContractInterface` (80 bytes): there is
    // no `HostContractVTable`/`header` wrapper in the ABI. Read dispatch metadata
    // directly from the struct, mirroring the canonical Rust host-contract caller.
    out.push_str("    local interface = ffi.cast(\"HostContractInterface*\", self._interface)\n");
    out.push_str("    local dispatch_type = interface.dispatch_type\n");

    emit_lua_guest_host_contract_args_setup(out, func, class_name, enums);
    emit_lua_guest_host_contract_out_setup(out, &func.returns, enums);

    out.push_str(
        "    -- Out-param ABI: dispatch writes the AbiError through a trailing pointer.\n",
    );
    out.push_str("    local err = ffi.new(\"AbiError\")\n");
    out.push_str("    if dispatch_type == 0 then\n");
    // Function-id bounds check inside the Native arm only: on a VM interface
    // dispatch.native.function_count aliases bits of dispatch.vm.call through
    // the union (garbage). The VM-side loader enforces its own bounds
    // (FunctionNotAvailable).
    out.push_str(&format!(
        "        if {fn_id} >= interface.dispatch.native.function_count then\n"
    ));
    if has_return {
        out.push_str("            return nil\n");
    } else {
        out.push_str("            return\n");
    }
    out.push_str("        end\n");
    // Native dispatch: the thunk receives the per-instance state pointer as its
    // first argument (the `this`/impl pointer), exactly as the canonical Rust
    // caller passes `self.instance.data`.
    out.push_str(&format!(
        "        local fn_ptr = interface.dispatch.native.functions[{fn_id}]\n"
    ));
    out.push_str("        local impl_ptr = nil\n");
    out.push_str("        if self._instance ~= nil then impl_ptr = self._instance.data end\n");
    out.push_str("        local fn = ffi.cast(DispatchFnType, fn_ptr)\n");
    out.push_str("        fn(impl_ptr, args_ptr, out_ptr, err)\n");
    out.push_str("    elseif dispatch_type == 1 then\n");
    // The VM dispatch ABI receives the registrant's user-data bridge as its
    // adapter context, then loader data and a null guest instance. This is the
    // host-contract analogue of a guest interface's adapter_context.
    out.push_str("        local _null_instance = ffi.new(\"GuestContractInstance\")\n");
    out.push_str(&format!(
        "        interface.dispatch.vm.call(interface.user_data, interface.dispatch.vm.loader_data, _null_instance, {fn_id}, args_ptr, out_ptr, nil, err)\n"
    ));
    out.push_str("    else\n");
    if has_return {
        out.push_str("        return nil\n");
    } else {
        out.push_str("        return\n");
    }
    out.push_str("    end\n");

    // err is a 24-byte AbiError struct returned by value; check its code field.
    out.push_str("    if err.code ~= 0 then\n");
    if has_return {
        out.push_str("        return nil\n");
    } else {
        out.push_str("        return\n");
    }
    out.push_str("    end\n");

    if has_return {
        out.push_str(&format!(
            "    return {}\n",
            lua_return_expr(&func.returns, enums)
        ));
    }
    if body.ends_with('\n') {
        body.pop();
    }
    write_luals_docs(dst, "", func.docs.as_deref());
    for param in &func.params {
        if let Some(docs) = param.docs.as_deref() {
            write_luals_docs(dst, "", Some(docs));
            dst.push_str(&format!(
                "---@param {} {} {}\n",
                param.name,
                lua_host_type_annotation(&param.ty),
                docs.replace('\n', " ")
            ));
        }
    }
    if let Some(docs) = func.return_docs.as_deref() {
        write_luals_docs(dst, "", Some(docs));
        let return_type: String = match &func.returns {
            Some(ty) => lua_host_type_annotation(ty),
            None => "nil".to_owned(),
        };
        dst.push_str(&format!(
            "---@return {} {}\n",
            return_type,
            docs.replace('\n', " ")
        ));
    }
    dst.push_str(&render_lua_defn_fn(
        &format!("{class_name}:{}", func.name),
        parameters,
        body,
    )?);
    dst.push('\n');
    Ok(())
}

/// Emit the args_ptr setup for a Lua guest host contract method.
///
/// `pack_prefix` names the caller class owning the per-function argument-pack
/// struct (cdef'd at file top for multi-param functions).
fn emit_lua_guest_host_contract_args_setup(
    out: &mut String,
    func: &ResolvedFunction,
    pack_prefix: &str,
    enums: &[EnumDef],
) {
    if func.params.is_empty() {
        out.push_str("    local args_ptr = nil\n");
        return;
    }

    if func.params.len() == 1 {
        let param: &ResolvedParam = &func.params[0];
        match &param.ty {
            ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => {
                out.push_str(&format!(
                    "    local {name}_bytes = tostring({name})\n",
                    name = param.name
                ));
                out.push_str(&format!(
                    "    local {name}_view = ffi.new(\"StringView\")\n",
                    name = param.name
                ));
                out.push_str(&format!(
                    "    {name}_view.ptr = ffi.cast(\"const char*\", {name}_bytes)\n",
                    name = param.name
                ));
                out.push_str(&format!(
                    "    {name}_view.len = #{name}_bytes\n",
                    name = param.name
                ));
                out.push_str(&format!(
                    "    local args_ptr = ffi.cast(\"const void*\", {name}_view)\n",
                    name = param.name
                ));
            }
            ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => {
                out.push_str(&format!(
                    "    local {name}_buf = ffi.new(\"Buffer\")\n",
                    name = param.name
                ));
                out.push_str(&format!(
                    "    {name}_buf.ptr = ffi.cast(\"void*\", {name})\n",
                    name = param.name
                ));
                out.push_str(&format!(
                    "    {name}_buf.len = #{name}\n",
                    name = param.name
                ));
                out.push_str(&format!(
                    "    local args_ptr = ffi.cast(\"const void*\", {name}_buf)\n",
                    name = param.name
                ));
            }
            ResolvedTypeRef::UserDefined(_) => {
                match lua_enum_repr_c_type(&param.ty, enums) {
                    // Enum: the value is a plain Lua number. Write it into a
                    // repr-integer slot and pass the SLOT's address — casting
                    // the bare number to void* would make the enum VALUE the
                    // address (same class the factory-side fix removed).
                    Some(repr) => {
                        out.push_str(&format!(
                            "    local {name}_val = ffi.new(\"{repr}[1]\", {name})\n",
                            name = param.name
                        ));
                        out.push_str(&format!(
                            "    local args_ptr = ffi.cast(\"const void*\", {name}_val)\n",
                            name = param.name
                        ));
                    }
                    // Struct: a cdef'd struct cdata is a reference cdata, so
                    // the cast yields its address.
                    None => {
                        out.push_str(&format!(
                            "    local args_ptr = ffi.cast(\"const void*\", {})\n",
                            param.name
                        ));
                    }
                }
            }
            ResolvedTypeRef::Primitive(_) | ResolvedTypeRef::AbiType(_) => {
                // Scalar/pointer params need a 1-element array slot for the same
                // reason as scalar out slots (see lua_return_is_scalar): a scalar
                // ffi.new("T", v) is a VALUE cdata and ffi.cast("void*", value)
                // converts the value instead of taking its address.
                let ty_name: String = lua_type_name(&param.ty);
                out.push_str(&format!(
                    "    local {name}_val = ffi.new(\"{ty}[1]\", {name})\n",
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

    // Multiple params: pack into the cdef'd per-function argument-pack struct.
    // A plain Lua table cannot be ffi.cast to a pointer (it always raises), so
    // the pack is an ffi.new struct, mirroring the host-caller pack path.
    let pack_struct: String = arg_pack_struct_name(pack_prefix, &func.name);
    out.push_str(&format!(
        "    local args_val = ffi.new(\"{pack_struct}\")\n"
    ));
    for param in &func.params {
        match &param.ty {
            ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => {
                // The {name}_bytes local anchors the Lua string for the call's
                // duration so the StringView's ptr stays valid.
                out.push_str(&format!(
                    "    local {name}_bytes = tostring({name})\n",
                    name = param.name
                ));
                out.push_str(&format!(
                    "    args_val.{name}.ptr = ffi.cast(\"const char*\", {name}_bytes)\n",
                    name = param.name
                ));
                out.push_str(&format!(
                    "    args_val.{name}.len = #{name}_bytes\n",
                    name = param.name
                ));
            }
            ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => {
                out.push_str(&format!(
                    "    args_val.{name}.ptr = ffi.cast(\"void*\", {name})\n",
                    name = param.name
                ));
                out.push_str(&format!(
                    "    args_val.{name}.len = #{name}\n",
                    name = param.name
                ));
            }
            _ => {
                out.push_str(&format!("    args_val.{0} = {0}\n", param.name));
            }
        }
    }
    out.push_str("    local args_ptr = ffi.cast(\"const void*\", args_val)\n");
}

/// Emit the out_ptr setup for a Lua guest host contract method.
fn emit_lua_guest_host_contract_out_setup(
    out: &mut String,
    returns: &Option<ResolvedTypeRef>,
    enums: &[EnumDef],
) {
    if let Some(ret_ty) = returns {
        // Enum returns: the out slot is the enum's repr C integer type (the
        // enum itself has no cdef'd C type), as a 1-element array like other
        // scalars; read back via lua_return_expr's tonumber(out_val[0]).
        if let Some(repr) = lua_enum_repr_c_type(ret_ty, enums) {
            out.push_str(&format!("    local out_val = ffi.new(\"{repr}[1]\")\n"));
            out.push_str("    local out_ptr = ffi.cast(\"void*\", out_val)\n");
        } else if matches!(ret_ty, ResolvedTypeRef::AbiType(AbiBuiltin::StringView)) {
            out.push_str("    local out_val = ffi.new(\"StringView\")\n");
            out.push_str("    local out_ptr = ffi.cast(\"void*\", out_val)\n");
        } else if matches!(ret_ty, ResolvedTypeRef::AbiType(AbiBuiltin::Buffer)) {
            out.push_str("    local out_val = ffi.new(\"Buffer\")\n");
            out.push_str("    local out_ptr = ffi.cast(\"void*\", out_val)\n");
        } else if lua_return_is_scalar(ret_ty) {
            let ty_name: String = lua_type_name(ret_ty);
            out.push_str(&format!("    local out_val = ffi.new(\"{ty_name}[1]\")\n"));
            out.push_str("    local out_ptr = ffi.cast(\"void*\", out_val)\n");
        } else {
            let ty_name: String = lua_type_name(ret_ty);
            out.push_str(&format!("    local out_val = ffi.new(\"{ty_name}\")\n"));
            out.push_str("    local out_ptr = ffi.cast(\"void*\", out_val)\n");
        }
    } else {
        out.push_str("    local out_ptr = nil\n");
    }
}

// ─── Guest Peer Caller Generation ─────────────────────────────────────────────

/// Convert a guest contract name to the Lua peer-caller class name.
/// e.g. "pipeline.Validator" -> "PipelineValidatorPeer"
fn contract_name_to_lua_peer_class(name: &str) -> String {
    let pascal: String = name
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
    format!("{pascal}Peer")
}

/// Generate the full `guest/peer_callers.lua` file for all peer contracts.
fn generate_lua_guest_peer_callers_file(ir: &ValidatedIr, peers: &[&ResolvedContract]) -> String {
    let mut out: String = String::new();
    out.push_str(file_header());
    // polyplug_abi declares GuestContractInterface, GuestContractInstance,
    // GuestContractHandle, AbiError, StringView, Buffer, HostApi — all needed below.
    out.push_str(&lua_require_block(&[&[
        ("ffi", "ffi"),
        ("polyplug_abi", "polyplug_abi"),
        ("polyplug_guest", "polyplug_guest"),
    ]]));
    out.push('\n');

    // cdef the per-function argument-pack structs (multi-param functions only).
    // Guarded: another generated module may have declared the same packs.
    let mut pack_cdefs: String = String::new();
    for contract in peers {
        let class_name: String = contract_name_to_lua_peer_class(&contract.name);
        for func in &contract.functions {
            if needs_arg_pack(&func.params) {
                emit_lua_arg_pack_struct(&mut pack_cdefs, &class_name, func, &ir.enums);
            }
        }
    }
    if !pack_cdefs.is_empty() {
        out.push_str(cdef_guarded_block());
        out.push_str("cdef_guarded([[\n");
        out.push_str(&pack_cdefs);
        out.push_str("]])\n\n");
    }

    out.push('\n');

    out.push_str("local M = {}\n\n");

    for contract in peers {
        let min_ver: u32 = peer_min_version(ir, contract.contract_id);
        generate_lua_guest_peer_caller(&mut out, contract, min_ver, &ir.enums);
    }

    // Export peer classes and their contract-ID constants.
    out.push_str("-- Contract ID constants\n");
    for contract in peers {
        let class_name: String = contract_name_to_lua_peer_class(&contract.name);
        let const_name: String = format!("{}_ID", class_name.to_uppercase());
        out.push_str(&format!(
            "M.{} = 0x{:016X}ULL\n",
            const_name, contract.contract_id
        ));
    }
    out.push('\n');

    out.push_str("-- Export peer caller classes\n");
    for contract in peers {
        let class_name: String = contract_name_to_lua_peer_class(&contract.name);
        out.push_str(&format!("M.{} = {}\n", class_name, class_name));
    }
    out.push('\n');

    out.push_str("return M\n");
    out
}

/// Generate one guest-side peer caller class for `contract`.
fn generate_lua_guest_peer_caller(
    out: &mut String,
    contract: &ResolvedContract,
    min_version: u32,
    enums: &[EnumDef],
) {
    let class_name: String = contract_name_to_lua_peer_class(&contract.name);

    out.push_str(&format!(
        "-- Peer caller for guest contract `{}` (id=0x{:016X})\n",
        contract.name, contract.contract_id
    ));
    out.push_str(&format!("{} = {{}}\n", class_name));
    out.push_str(&format!("{}.__index = {}\n\n", class_name, class_name));

    // :new(interface, instance, host, handle, cached_revision) is the low-level
    // constructor used by resolve().
    out.push_str(&format!(
        "function {}:new(interface, instance, host, handle, cached_revision)\n",
        class_name
    ));
    out.push_str(
        "    local obj = { _interface = interface, _instance = instance, _host = host, _handle = handle, _cached_revision = cached_revision }\n",
    );
    out.push_str("    setmetatable(obj, self)\n");
    out.push_str("    return obj\n");
    out.push_str("end\n\n");

    // .resolve(host_ptr) — factory: find → resolve → create_instance.
    // `host_ptr` is threaded in explicitly by the caller (the author factory
    // captured it; no per-VM global — Rule 12). It is a plain Lua number; cast
    // through uintptr_t first (matching the host-contract caller's from_host path)
    // — a direct ffi.cast("HostApi*", number) is rejected by LuaJIT as the first
    // FFI argument.
    out.push_str(&format!("function {}.resolve(host_ptr)\n", class_name));
    out.push_str("    if host_ptr == nil or host_ptr == 0 then\n");
    out.push_str("        return nil\n");
    out.push_str("    end\n");
    out.push_str("    local host = ffi.cast(\"HostApi*\", ffi.cast(\"uintptr_t\", host_ptr))\n");
    // find_guest_contract: returns an opaque GuestContractHandle — do NOT inspect
    // its fields; pass it straight to resolve_guest_contract and nil-check there.
    out.push_str(&format!(
        "    local handle = host.find_guest_contract(host, 0x{:016X}ULL, {})\n",
        contract.contract_id, min_version
    ));
    out.push_str("    local interface = host.resolve_guest_contract(host, handle)\n");
    out.push_str("    if interface == nil then\n");
    out.push_str("        return nil\n");
    out.push_str("    end\n");
    // A null instance.data is valid: stateless contracts and all VM-dispatch guests
    // return a null handle from create_instance and use it as an opaque dispatch token.
    // Validity is keyed off the interface pointer, not the instance.
    out.push_str("    -- Route creation through the host so the runtime tracks the instance.\n");
    out.push_str("    -- create_guest_instance is an out-param ABI fn: (this, interface, args, out_instance) -> void.\n");
    out.push_str("    local instance = ffi.new(\"GuestContractInstance\")\n");
    out.push_str("    host.create_guest_instance(host, interface, nil, instance)\n");
    // Capture the synchronized revision for the resolved interface.
    out.push_str("    local cached_revision = host.registry_revision(host)\n");
    out.push_str(&format!(
        "    return {}:new(interface, instance, host, handle, cached_revision)\n",
        class_name
    ));
    out.push_str("end\n\n");

    out.push_str(&format!("function {}:is_valid()\n", class_name));
    out.push_str("    return self._interface ~= nil\n");
    out.push_str("end\n\n");

    // live_revision reads the synchronized value through HostApi.
    out.push_str(&format!("function {}:live_revision()\n", class_name));
    out.push_str("    return self._host.registry_revision(self._host)\n");
    out.push_str("end\n\n");

    // revalidate - the peer registry changed under us (a reload/unload reclaimed the
    // cached interface and instance). Re-resolve via the retained handle: a reload
    // swapped a new interface into the same slot (handle resolves to it); an unload
    // vacated the slot (resolves to nil → return false, peer gone). The old instance
    // is ABANDONED — its interface is already epoch-reclaimed, so dispatching it
    // directly would be UB. A fresh instance is created against the new interface.
    out.push_str(&format!("function {}:revalidate()\n", class_name));
    out.push_str(
        "    local interface = self._host.resolve_guest_contract(self._host, self._handle)\n",
    );
    out.push_str("    if interface == nil then\n");
    out.push_str("        self._interface = nil\n");
    out.push_str("        self._instance = ffi.new(\"GuestContractInstance\")\n");
    out.push_str("        self._cached_revision = self:live_revision()\n");
    out.push_str("        return false\n");
    out.push_str("    end\n");
    out.push_str("    local new_instance = ffi.new(\"GuestContractInstance\")\n");
    out.push_str(
        "    self._host.create_guest_instance(self._host, interface, nil, new_instance)\n",
    );
    out.push_str("    self._interface = interface\n");
    out.push_str("    self._instance = new_instance\n");
    out.push_str("    self._cached_revision = self:live_revision()\n");
    out.push_str("    return true\n");
    out.push_str("end\n\n");

    for func in &contract.functions {
        generate_lua_guest_peer_method(out, func, &class_name, enums);
    }

    out.push('\n');
}

/// Generate one method on a guest peer caller class.
///
/// Dispatches directly through the cached peer interface — same near-bare-metal
/// path as the host->guest caller; no host-mediated round-trip, no per-call
/// registry resolve, no epoch pin. The declared dependency keeps the peer alive;
/// a hot-reload is caught by the cached revision counter.
fn generate_lua_guest_peer_method(
    out: &mut String,
    func: &ResolvedFunction,
    class_name: &str,
    enums: &[EnumDef],
) {
    let fn_id: u32 = func.function_id;
    let has_return: bool = func.returns.is_some();

    // Colon-method syntax (`Class:method`) binds `self` implicitly — do NOT
    // re-declare it in the parameter list (same rule as the host-contract caller).
    let params_str: String = func
        .params
        .iter()
        .map(|p: &ResolvedParam| p.name.clone())
        .collect::<Vec<String>>()
        .join(", ");

    out.push_str(&format!(
        "function {}:{}({})\n",
        class_name, func.name, params_str
    ));

    out.push_str("    if self._interface == nil then\n");
    if has_return {
        out.push_str("        return nil\n");
    } else {
        out.push_str("        return\n");
    }
    out.push_str("    end\n");

    // Cheap per-call staleness check: read the registry revision directly through
    // the cached pointer (one atomic load, no call into the runtime). On any change
    // (hot-reload or unload of the peer) we re-resolve first, so the cached
    // interface and instance are never dispatched once they dangle. A failed
    // revalidate means the peer is gone.
    out.push_str(
        "    if self:live_revision() ~= self._cached_revision and not self:revalidate() then\n",
    );
    if has_return {
        out.push_str("        return nil\n");
    } else {
        out.push_str("        return\n");
    }
    out.push_str("    end\n");

    // Cast the stored interface pointer to GuestContractInterface so we can read
    // dispatch_type and the native/VM union — mirrors the host-caller path.
    out.push_str("    local interface = ffi.cast(\"GuestContractInterface*\", self._interface)\n");
    out.push_str("    local dispatch_type = interface.dispatch_type\n");

    // Args and out setup — reuse the same helpers as the host-contract caller so
    // marshalling is identical (no extra tostring() layer = avoids the a3-parity
    // double-conversion Lua footgun).
    emit_lua_guest_host_contract_args_setup(out, func, class_name, enums);
    emit_lua_guest_host_contract_out_setup(out, &func.returns, enums);

    out.push_str(
        "    -- Out-param ABI: dispatch writes the AbiError through a trailing pointer.\n",
    );
    out.push_str("    local err = ffi.new(\"AbiError\")\n");
    out.push_str("    if dispatch_type == 0 then\n");
    // Function-id bounds check inside the Native arm only: on a VM interface
    // dispatch.native.function_count aliases bits of dispatch.vm.call through
    // the union (garbage). The VM-side loader enforces its own bounds
    // (FunctionNotAvailable).
    out.push_str(&format!(
        "        if {fn_id} >= interface.dispatch.native.function_count then\n"
    ));
    if has_return {
        out.push_str("            return nil\n");
    } else {
        out.push_str("            return\n");
    }
    out.push_str("        end\n");
    // Native dispatch calls the guest function pointer directly through the
    // cached interface, forwarding its immutable generated adapter context.
    out.push_str(&format!(
        "        local fn_ptr = interface.dispatch.native.functions[{fn_id}]\n"
    ));
    out.push_str("        local fn = ffi.cast(NativeDispatchFnType, fn_ptr)\n");
    out.push_str("        fn(interface.adapter_context, self._instance, args_ptr, out_ptr, err)\n");
    out.push_str("    elseif dispatch_type == 1 then\n");
    // VM dispatch receives the immutable adapter context carried by the
    // cached guest interface, followed by loader data and the caller instance.
    out.push_str(&format!(
        "        interface.dispatch.vm.call(interface.adapter_context, interface.dispatch.vm.loader_data, self._instance, {fn_id}, args_ptr, out_ptr, nil, err)\n"
    ));
    out.push_str("    else\n");
    if has_return {
        out.push_str("        return nil\n");
    } else {
        out.push_str("        return\n");
    }
    out.push_str("    end\n");

    // err.code == 0 means AbiErrorCode::Ok.
    out.push_str("    if err.code ~= 0 then\n");
    if has_return {
        out.push_str("        return nil\n");
    } else {
        out.push_str("        return\n");
    }
    out.push_str("    end\n");

    if has_return {
        out.push_str(&format!(
            "    return {}\n",
            lua_return_expr(&func.returns, enums)
        ));
    }
    out.push_str("end\n\n");
}

/// Generate `guest/host_contracts.lua` — caller classes for guest-side host contract callers.
fn generate_guest_host_contracts_file(ir: &ValidatedIr) -> Result<String, PolyplugcError> {
    let mut out: String = String::new();
    out.push_str(file_header());
    // Require the polyplug_abi Lua SDK so the HostContractInterface / AbiError /
    // GuestContractInstance cdefs this module casts to are declared. Without this
    // require the ffi.cast(\"HostContractInterface*\", ...) below would fail at load.
    out.push_str(&lua_require_block(&[&[
        ("ffi", "ffi"),
        ("polyplug_abi", "polyplug_abi"),
    ]]));
    out.push('\n');

    // cdef the per-function argument-pack structs (multi-param functions only).
    // Guarded: another generated module may have declared the same packs.
    let mut pack_cdefs: String = String::new();
    for contract in &ir.host_contracts {
        let class_name: String = host_contract_name_to_lua_caller(&contract.name);
        for func in &contract.functions {
            if needs_arg_pack(&func.params) {
                emit_lua_arg_pack_struct(&mut pack_cdefs, &class_name, func, &ir.enums);
            }
        }
    }
    if !pack_cdefs.is_empty() {
        out.push_str(cdef_guarded_block());
        out.push_str("cdef_guarded([[\n");
        out.push_str(&pack_cdefs);
        out.push_str("]])\n\n");
    }

    out.push_str("local M = {}\n\n");

    // Native host-contract dispatch returns an AbiError (24-byte struct) by value,
    // taking (this, args, out) where `this` is the per-instance state pointer.
    // This mirrors the canonical Rust host-contract caller's native fn signature.
    out.push_str("-- Cached FFI types for hot path performance\n");
    out.push_str(
        "local DispatchFnType = ffi.typeof(\"void (*)(const void*, const void*, void*, AbiError*)\")\n\n",
    );

    for contract in &ir.host_contracts {
        generate_lua_guest_host_contract_caller(&mut out, contract, &ir.enums)?;
    }

    out.push_str("-- Contract ID constants\n");
    for contract in &ir.host_contracts {
        let class_name: String = host_contract_name_to_lua_caller(&contract.name);
        let const_name: String = format!("{}_ID", class_name.to_uppercase());
        out.push_str(&format!(
            "M.{} = 0x{:016X}ULL\n",
            const_name, contract.contract_id
        ));
    }
    out.push('\n');

    out.push_str("-- Export guest caller classes\n");
    for contract in &ir.host_contracts {
        let class_name: String = host_contract_name_to_lua_caller(&contract.name);
        out.push_str(&format!("M.{} = {}\n", class_name, class_name));
    }
    out.push('\n');

    out.push_str("return M\n");
    Ok(out)
}

// ─── Host Interface Factories Generation ─────────────────────────────────────────

/// Generate all host-side interface factories into a single file.
///
/// LuaJIT FFI callbacks cannot return structs by value (a documented NYI), so a
/// LuaJIT host can never produce native-dispatch thunks (which return `AbiError`
/// by value) nor `create_instance` stubs (which return `HostContractInstance` by
/// value) in pure Lua. The factories therefore register host contracts with VM
/// dispatch routed through the native trampoline exported by the lua loader
/// cdylib (`polyplug_lua_host_vm_dispatch` plus the instance stubs in
/// `crates/polyplug_lua/src/ffi.rs`); all per-contract marshalling lives in a
/// scalar-only LuaJIT dispatcher callback that the trampoline forwards to.
fn generate_lua_host_interface_factories_file(ir: &ValidatedIr) -> String {
    let mut out: String = String::new();
    out.push_str(file_header());
    out.push_str("-- Requires the polyplug_abi cdefs (HostContractInterface, AbiError, ...);\n");
    out.push_str("-- the host must require(\"polyplug_abi\") before requiring this module.\n");
    out.push_str(&lua_require_block(&[&[("ffi", "ffi")]]));
    out.push('\n');

    out.push_str("-- ABI error codes (match polyplug_abi.AbiErrorCode)\n");
    out.push_str("local AbiErrorCode = {\n");
    out.push_str("    Ok = 0,\n");
    out.push_str("    Panic = 3,\n");
    out.push_str("    FunctionNotAvailable = 6,\n");
    out.push_str("}\n\n");

    out.push_str(cdef_guarded_block());

    // Bridge + trampoline declarations, resolved from the lua loader cdylib
    // (libpolyplug_lua). Layout must match `PolyplugLuaHostDispatchBridge` and
    // the exported trampoline signatures in crates/polyplug_lua/src/ffi.rs.
    out.push_str("cdef_guarded([[\n");
    out.push_str(
        "    typedef uint32_t (*PolyplugLuaHostDispatchCallback)(void* /*instance_data*/, uint32_t, const void*, void*);\n",
    );
    out.push_str("    typedef void (*PolyplugLuaHostDestroyCallback)(void* /*instance_data*/);\n");
    out.push_str(
        "    typedef void (*PolyplugLuaHostCreateInstanceFn)(const HostContractInterface*, const void*, HostContractInstance*);\n",
    );
    out.push_str("    typedef struct PolyplugLuaHostDispatchBridge {\n");
    out.push_str("        PolyplugLuaHostDispatchCallback callback;\n");
    out.push_str("        PolyplugLuaHostDestroyCallback destroy_callback;\n");
    out.push_str("    } PolyplugLuaHostDispatchBridge;\n");
    out.push_str(
        "    void polyplug_lua_host_vm_dispatch(void*, VmLoaderData, GuestContractInstance, uint32_t, const void*, void*, CallArena*, AbiError*);\n",
    );
    out.push_str(
        "    void polyplug_lua_host_destroy_instance(const HostContractInterface*, HostContractInstance);\n",
    );
    out.push_str("]])\n\n");

    // Arg-pack structs for multi-parameter host contract functions. Layout
    // mirrors the guest-side callers' packs (same field order/types); the
    // guest callers cdef identically laid out structs under their own names.
    let mut pack_cdefs: String = String::new();
    for contract in &ir.host_contracts {
        let class_name: String = host_contract_name_to_lua_class(&contract.name);
        for func in &contract.functions {
            if needs_arg_pack(&func.params) {
                emit_lua_arg_pack_struct(&mut pack_cdefs, &class_name, func, &ir.enums);
            }
        }
    }
    if !pack_cdefs.is_empty() {
        out.push_str("cdef_guarded([[\n");
        out.push_str(&pack_cdefs);
        out.push_str("]])\n\n");
    }

    out.push_str("local M = {}\n\n");

    out.push_str("-- Anchors for cdata that must stay alive after a factory returns: the\n");
    out.push_str("-- runtime keeps the interface pointer for its whole lifetime and every\n");
    out.push_str("-- dispatch reaches the bridge + callback. Module-local (per-VM) state.\n");
    out.push_str("local _anchors = {}\n\n");

    for contract in &ir.host_contracts {
        generate_lua_host_interface_factory(&mut out, contract, &ir.enums);
    }

    out.push_str("return M\n");
    out
}

/// Generate the interface factory for one host contract.
///
/// The factory takes the implementation table plus the lua loader cdylib handle
/// (an `ffi.load` clib exposing the `polyplug_lua_host_*` trampolines) and
/// returns a fully populated `HostContractInterface` with VM dispatch. The
/// per-function marshalling runs in a scalar-only LuaJIT dispatcher callback —
/// the only callback shape LuaJIT can create (no struct-by-value args/returns).
fn generate_lua_host_interface_factory(
    out: &mut String,
    contract: &ResolvedHostContract,
    enums: &[EnumDef],
) {
    let class_name: String = host_contract_name_to_lua_class(&contract.name);
    let factory_name: String = format!(
        "create_{}_interface",
        contract.name.replace('.', "_").to_lowercase()
    );
    let contract_id: u64 = contract.contract_id;
    let major: u32 = contract.version.major;
    let minor: u32 = contract.version.minor;
    let patch: u32 = contract.version.patch;
    let singleton: u8 = if contract.singleton { 1_u8 } else { 0_u8 };
    let singleton_comment: &str = if contract.singleton {
        "singleton"
    } else {
        "multi-instance"
    };

    out.push_str(&format!(
        "-- Create a host contract interface for `{}` (VM dispatch via the lua\n",
        contract.name
    ));
    out.push_str("-- loader trampoline — see the file header for why native dispatch is\n");
    out.push_str("-- impossible under LuaJIT).\n");
    out.push_str("--\n");
    out.push_str("-- Arguments:\n");
    out.push_str("--     factory: a function () -> impl that builds a FRESH implementation\n");
    out.push_str("--         table (methods matching the contract) per instance. The runtime\n");
    out.push_str("--         calls create_instance once per non-singleton caller (so each gets\n");
    out.push_str("--         independent state) and once total for singletons (shared state).\n");
    out.push_str("--     lua_bridge_lib: ffi.load handle for the lua loader cdylib\n");
    out.push_str(
        "--         (libpolyplug_lua), e.g. require('polyplug.loaders.lua').bridge_lib()\n",
    );
    out.push_str("--\n");
    out.push_str("-- Memory:\n");
    out.push_str(
        "-- The returned interface is anchored and lives for the lifetime of the program.\n",
    );
    out.push_str(&format!(
        "function M.{factory_name}(factory, lua_bridge_lib)\n"
    ));
    out.push_str(&format!(
        "    if factory == nil then\n        error(\"{factory_name}: factory is nil (pass a function () -> impl)\")\n    end\n"
    ));
    out.push_str(&format!(
        "    if lua_bridge_lib == nil then\n        error(\"{factory_name}: lua_bridge_lib is nil (pass the lua loader cdylib handle)\")\n    end\n\n"
    ));

    // Per-interface instance registry (closure-captured, NOT module-global —
    // each factory call has its own). `instances[id]` holds the per-instance
    // impl; `default_impl` serves null/id-0 dispatch (built once at registration).
    out.push_str("    local instances = {}\n");
    out.push_str("    local next_id = 1\n");
    out.push_str("    local default_impl = factory()\n\n");

    // Scalar-only dispatcher: (instance_data, fn_id, args, out) -> AbiErrorCode.
    // Resolve the per-instance impl from instance_data (an instance id cast to a
    // pointer); a null/0 handle uses the default impl.
    out.push_str("    local function dispatch(instance_data, fn_id, args, out)\n");
    out.push_str("        local ok, code = pcall(function()\n");
    out.push_str("            local impl\n");
    out.push_str("            local inst_id = tonumber(ffi.cast(\"uintptr_t\", instance_data))\n");
    out.push_str("            if inst_id == 0 then\n");
    out.push_str("                impl = default_impl\n");
    out.push_str("            else\n");
    out.push_str("                impl = instances[inst_id]\n");
    out.push_str("            end\n");
    out.push_str("            if impl == nil then\n");
    out.push_str("                return AbiErrorCode.FunctionNotAvailable\n");
    out.push_str("            end\n");
    for (idx, func) in contract.functions.iter().enumerate() {
        out.push_str(&format!("            if fn_id == {idx} then\n"));
        generate_lua_host_dispatch_args(out, &class_name, func, enums);
        generate_lua_host_dispatch_call(out, func, enums);
        out.push_str("                return AbiErrorCode.Ok\n");
        out.push_str("            end\n");
    }
    out.push_str("            return AbiErrorCode.FunctionNotAvailable\n");
    out.push_str("        end)\n");
    out.push_str("        if not ok then\n");
    out.push_str("            return AbiErrorCode.Panic\n");
    out.push_str("        end\n");
    out.push_str("        return code\n");
    out.push_str("    end\n\n");

    // create_instance is a Lua callback (pointer args + out-pointer, no struct by
    // value — LuaJIT can create it). Each call builds a fresh impl, keys it by a
    // fresh id, and stamps the id into the out HostContractInstance.data.
    out.push_str("    local function create_instance(this, args, out_ptr)\n");
    out.push_str("        local _ = this\n");
    out.push_str("        local _ = args\n");
    out.push_str("        local inst_impl = factory()\n");
    out.push_str("        local id = next_id\n");
    out.push_str("        next_id = next_id + 1\n");
    out.push_str("        instances[id] = inst_impl\n");
    out.push_str("        local hci = ffi.cast(\"HostContractInstance*\", out_ptr)\n");
    out.push_str("        hci.data = ffi.cast(\"void*\", id)\n");
    out.push_str("    end\n\n");

    // destroy_instance is a scalar callback invoked by the native
    // polyplug_lua_host_destroy_instance trampoline; it drops the per-instance
    // impl keyed by the instance id.
    out.push_str("    local function destroy_instance(instance_data)\n");
    out.push_str("        local id = tonumber(ffi.cast(\"uintptr_t\", instance_data))\n");
    out.push_str("        if id ~= 0 then\n");
    out.push_str("            instances[id] = nil\n");
    out.push_str("        end\n");
    out.push_str("    end\n\n");

    // Bridge + interface construction. The callback casts anchor the LuaJIT
    // callback objects; bridge and interface are plain cdata.
    out.push_str(
        "    local dispatch_cb = ffi.cast(\"PolyplugLuaHostDispatchCallback\", dispatch)\n",
    );
    out.push_str(
        "    local create_cb = ffi.cast(\"PolyplugLuaHostCreateInstanceFn\", create_instance)\n",
    );
    out.push_str(
        "    local destroy_cb = ffi.cast(\"PolyplugLuaHostDestroyCallback\", destroy_instance)\n",
    );
    out.push_str("    local bridge = ffi.new(\"PolyplugLuaHostDispatchBridge\")\n");
    out.push_str("    bridge.callback = dispatch_cb\n");
    out.push_str("    bridge.destroy_callback = destroy_cb\n\n");

    out.push_str("    local interface = ffi.new(\"HostContractInterface\")\n");
    out.push_str(&format!(
        "    interface.contract_id = 0x{contract_id:016X}ULL\n"
    ));
    out.push_str(&format!("    interface.contract_version.major = {major}\n"));
    out.push_str(&format!("    interface.contract_version.minor = {minor}\n"));
    out.push_str(&format!("    interface.contract_version.patch = {patch}\n"));
    out.push_str(&format!(
        "    interface.singleton = {singleton}  -- {singleton_comment}\n"
    ));
    out.push_str("    interface.dispatch_type = ffi.C.DispatchType_VirtualMachine\n");
    out.push_str("    interface.runtime = nil  -- set by the runtime during registration\n");
    out.push_str("    interface.user_data = ffi.cast(\"void*\", bridge)\n");
    out.push_str("    interface.create_instance = create_cb\n");
    out.push_str(
        "    interface.destroy_instance = lua_bridge_lib.polyplug_lua_host_destroy_instance\n",
    );
    out.push_str("    interface.dispatch.vm.call = lua_bridge_lib.polyplug_lua_host_vm_dispatch\n");
    out.push_str("    interface.dispatch.vm.loader_data.data = ffi.cast(\"void*\", bridge)\n\n");

    // Anchor everything that must outlive the factory: the interface cdata, the
    // bridge, all three LuaJIT callbacks, the instances registry, and the
    // default impl. Without this the LuaJIT callbacks/tables would be GC'd while
    // the runtime still holds the interface pointer.
    out.push_str(
        "    _anchors[#_anchors + 1] = { interface = interface, bridge = bridge, dispatch_cb = dispatch_cb, create_cb = create_cb, destroy_cb = destroy_cb, instances = instances, default_impl = default_impl }\n",
    );
    out.push_str("    return interface\n");
    out.push_str("end\n\n");
}

/// Emit argument extraction for one host-contract dispatcher branch.
///
/// Single-parameter functions receive a pointer directly to the value;
/// multi-parameter functions receive a pointer to the arg-pack struct emitted
/// by `emit_lua_arg_pack_struct` (same layout as the guest callers' packs).
fn generate_lua_host_dispatch_args(
    out: &mut String,
    class_name: &str,
    func: &ResolvedFunction,
    enums: &[EnumDef],
) {
    if func.params.is_empty() {
        return;
    }
    if func.params.len() == 1 {
        let param: &ResolvedParam = &func.params[0];
        match &param.ty {
            ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => {
                out.push_str(&format!(
                    "                local {name}_sv = ffi.cast(\"const StringView*\", args)[0]\n",
                    name = param.name
                ));
                out.push_str(&format!(
                    "                local {name} = ffi.string({name}_sv.ptr, {name}_sv.len)\n",
                    name = param.name
                ));
            }
            ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => {
                out.push_str(&format!(
                    "                local {name}_buf = ffi.cast(\"const Buffer*\", args)[0]\n",
                    name = param.name
                ));
                out.push_str(&format!(
                    "                local {name} = ffi.string({name}_buf.ptr, {name}_buf.len)\n",
                    name = param.name
                ));
            }
            other => {
                let ty_name: String = lua_c_type_name(other, enums);
                out.push_str(&format!(
                    "                local {name} = ffi.cast(\"const {ty}*\", args)[0]\n",
                    name = param.name,
                    ty = ty_name
                ));
            }
        }
    } else {
        let pack_struct: String = arg_pack_struct_name(class_name, &func.name);
        out.push_str(&format!(
            "                local packed = ffi.cast(\"const {pack_struct}*\", args)[0]\n"
        ));
        for param in &func.params {
            match &param.ty {
                ResolvedTypeRef::AbiType(AbiBuiltin::StringView)
                | ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => {
                    out.push_str(&format!(
                        "                local {name} = ffi.string(packed.{name}.ptr, packed.{name}.len)\n",
                        name = param.name
                    ));
                }
                _ => {
                    out.push_str(&format!(
                        "                local {name} = packed.{name}\n",
                        name = param.name
                    ));
                }
            }
        }
    }
}

/// Emit the implementation call (and result store) for one dispatcher branch.
///
/// Scalar returns are written through a typed out-pointer; struct returns
/// (StringView/Buffer/user types) require the implementation to return cdata of
/// the matching C type, which is copied into the out slot by assignment.
fn generate_lua_host_dispatch_call(out: &mut String, func: &ResolvedFunction, enums: &[EnumDef]) {
    let call_args: String = func
        .params
        .iter()
        .map(|p: &ResolvedParam| p.name.clone())
        .collect::<Vec<_>>()
        .join(", ");

    if has_return_value(&func.returns) {
        out.push_str(&format!(
            "                local result = impl:{func_name}({call_args})\n",
            func_name = func.name
        ));
        let ret_ty: String = match func.returns.as_ref() {
            Some(ret) => lua_c_type_name(ret, enums),
            None => String::from("void"),
        };
        out.push_str(&format!(
            "                ffi.cast(\"{ret_ty}*\", out)[0] = result\n"
        ));
    } else {
        out.push_str(&format!(
            "                impl:{func_name}({call_args})\n",
            func_name = func.name
        ));
        out.push_str("                local _ = out\n");
    }
}

// Compile-time assertion that lua_type_name compiles for primitive types.
const _: fn() = || {
    let _: String = lua_type_name(&ResolvedTypeRef::Primitive(PrimitiveType::U8));
};

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use super::*;
    use crate::ResolvedBundleFile;
    use crate::ir::ReprType;
    use crate::ir::ResolvedDependency;
    use crate::ir::Version;

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
                    docs: None,
                },
                EnumVariant {
                    name: "Rgba8".to_owned(),
                    value: "1".to_owned(),
                    docs: None,
                },
            ],
            docs: None,
        };
        let mut out: String = String::new();
        generate_lua_enum(&mut out, &e).expect("render enum");
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
                    docs: None,
                },
                EnumVariant {
                    name: "Compressed".to_owned(),
                    value: "1".to_owned(),
                    docs: None,
                },
                EnumVariant {
                    name: "Hdr".to_owned(),
                    value: "1 << 1".to_owned(),
                    docs: None,
                },
                EnumVariant {
                    name: "CompressedHdr".to_owned(),
                    value: "Compressed | Hdr".to_owned(),
                    docs: None,
                },
            ],
            docs: None,
        };
        let mut out: String = String::new();
        generate_lua_enum(&mut out, &e).expect("render enum");
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

    #[test]
    fn lua_types_module_exports_enum_tables() {
        let ir: ValidatedIr = ValidatedIr {
            types: vec![],
            enums: vec![EnumDef {
                name: "PixelFormat".to_owned(),
                repr: ReprType::U32,
                bitflag: false,
                variants: vec![EnumVariant {
                    name: "Rgba8".to_owned(),
                    value: "1".to_owned(),
                    docs: None,
                }],
                docs: None,
            }],
            contracts: vec![],
            host_contracts: vec![],
            bundle: None,
        };

        let out = generate_lua_types_file(&ir).expect("generate Lua types module");
        assert!(
            out.contains("return {\n    PixelFormat = PixelFormat,\n}"),
            "enum table must be available to require('guest.types'): {out}"
        );
    }

    // ─── Host Contract Metatable Tests ─────────────────────────────────────────────

    #[test]
    fn host_contract_name_to_lua_class_basic() {
        assert_eq!(host_contract_name_to_lua_class("host.logger"), "HostLogger");
    }

    #[test]
    fn host_contract_name_to_lua_class_nested() {
        assert_eq!(
            host_contract_name_to_lua_class("host.fs.reader"),
            "HostFsReader"
        );
    }

    #[test]
    fn host_contract_name_to_lua_class_already_has_host() {
        assert_eq!(
            host_contract_name_to_lua_class("host.HostLogger"),
            "HostLogger"
        );
    }

    #[test]
    fn lua_host_param_type_stringview() {
        let ty: ResolvedTypeRef = ResolvedTypeRef::AbiType(AbiBuiltin::StringView);
        assert_eq!(lua_host_type_annotation(&ty), "string");
    }

    #[test]
    fn lua_host_param_type_buffer() {
        let ty: ResolvedTypeRef = ResolvedTypeRef::AbiType(AbiBuiltin::Buffer);
        assert_eq!(lua_host_type_annotation(&ty), "string");
    }

    #[test]
    fn lua_host_param_type_primitives() {
        assert_eq!(
            lua_host_type_annotation(&ResolvedTypeRef::Primitive(PrimitiveType::U32)),
            "number"
        );
        assert_eq!(
            lua_host_type_annotation(&ResolvedTypeRef::Primitive(PrimitiveType::I64)),
            "number"
        );
        assert_eq!(
            lua_host_type_annotation(&ResolvedTypeRef::Primitive(PrimitiveType::F64)),
            "number"
        );
        assert_eq!(
            lua_host_type_annotation(&ResolvedTypeRef::Primitive(PrimitiveType::Bool)),
            "number"
        );
    }

    #[test]
    fn generate_lua_host_contract_metatable_basic() {
        let contract: ResolvedHostContract = ResolvedHostContract {
            name: "host.logger".to_owned(),
            contract_id: 0x123456789ABCDEF0,
            version: Version {
                major: 1,
                minor: 0,
                patch: 0,
            },
            singleton: false,
            functions: vec![ResolvedFunction {
                name: "log".to_owned(),
                function_id: 0,
                params: vec![
                    ResolvedParam {
                        name: "level".to_owned(),
                        ty: ResolvedTypeRef::Primitive(PrimitiveType::U32),
                        docs: None,
                    },
                    ResolvedParam {
                        name: "message".to_owned(),
                        ty: ResolvedTypeRef::AbiType(AbiBuiltin::StringView),
                        docs: None,
                    },
                ],
                returns: None,
                docs: None,
                return_docs: None,
            }],
            docs: None,
        };
        let mut out: String = String::new();
        generate_lua_host_contract_metatable(&mut out, &contract);
        assert!(
            out.contains("HostLogger = {}"),
            "missing class table: {out}"
        );
        assert!(
            out.contains("HostLogger.__index = HostLogger"),
            "missing __index: {out}"
        );
        assert!(
            out.contains("function HostLogger:new()"),
            "missing new method: {out}"
        );
        assert!(
            out.contains("function HostLogger:log(level, message)"),
            "missing log method: {out}"
        );
        assert!(
            out.contains("error(\"abstract method: log must be implemented by host\", 2)"),
            "missing error: {out}"
        );
    }

    #[test]
    fn generate_lua_host_contract_metatable_with_return() {
        let contract: ResolvedHostContract = ResolvedHostContract {
            name: "host.fs.reader".to_owned(),
            contract_id: 0xDEADBEEF,
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
                    docs: None,
                }],
                returns: Some(ResolvedTypeRef::AbiType(AbiBuiltin::Buffer)),
                docs: None,
                return_docs: None,
            }],
            docs: None,
        };
        let mut out: String = String::new();
        generate_lua_host_contract_metatable(&mut out, &contract);
        assert!(
            out.contains("HostFsReader = {}"),
            "missing class table: {out}"
        );
        assert!(
            out.contains("function HostFsReader:read(path)"),
            "missing read method: {out}"
        );
        assert!(
            out.contains("--- @return string"),
            "missing return annotation: {out}"
        );
    }

    #[test]
    fn generate_host_contracts_file_empty() {
        let ir: ValidatedIr = ValidatedIr {
            types: vec![],
            enums: vec![],
            contracts: vec![],
            host_contracts: vec![],
            bundle: None,
        };
        let result: String = generate_host_contracts_file(&ir);
        assert!(result.contains("local M = {}"));
        assert!(result.contains("return M"));
        assert!(!result.contains("HostLogger"));
    }

    #[test]
    fn generate_host_contracts_file_with_contract() {
        let contract: ResolvedHostContract = ResolvedHostContract {
            name: "host.logger".to_owned(),
            contract_id: 0x123456789ABCDEF0,
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
                docs: None,
                return_docs: None,
            }],
            docs: None,
        };
        let ir: ValidatedIr = ValidatedIr {
            types: vec![],
            enums: vec![],
            contracts: vec![],
            host_contracts: vec![contract],
            bundle: None,
        };
        let result: String = generate_host_contracts_file(&ir);
        assert!(result.contains("HostLogger = {}"));
        assert!(result.contains("M.HOSTLOGGER_CONTRACT_ID = 0x123456789ABCDEF0ULL"));
        assert!(result.contains("M.HostLogger = HostLogger"));
    }

    // ─── Guest Host Contract Caller Tests ─────────────────────────────────────────

    #[test]
    fn host_contract_name_to_lua_caller_basic() {
        assert_eq!(
            host_contract_name_to_lua_caller("host.logger"),
            "HostLoggerContract"
        );
    }

    #[test]
    fn host_contract_name_to_lua_caller_nested() {
        assert_eq!(
            host_contract_name_to_lua_caller("host.fs.reader"),
            "HostFsReaderContract"
        );
    }

    #[test]
    fn host_contract_name_to_lua_caller_already_has_host() {
        assert_eq!(
            host_contract_name_to_lua_caller("host.HostLogger"),
            "HostLoggerContract"
        );
    }

    #[test]
    fn generate_lua_guest_host_contract_caller_basic() {
        let contract: ResolvedHostContract = ResolvedHostContract {
            name: "host.logger".to_owned(),
            contract_id: 0x123456789ABCDEF0,
            version: Version {
                major: 1,
                minor: 0,
                patch: 0,
            },
            singleton: false,
            functions: vec![ResolvedFunction {
                name: "log".to_owned(),
                function_id: 0,
                params: vec![
                    ResolvedParam {
                        name: "level".to_owned(),
                        ty: ResolvedTypeRef::Primitive(PrimitiveType::U32),
                        docs: None,
                    },
                    ResolvedParam {
                        name: "message".to_owned(),
                        ty: ResolvedTypeRef::AbiType(AbiBuiltin::StringView),
                        docs: None,
                    },
                ],
                returns: None,
                docs: None,
                return_docs: None,
            }],
            docs: None,
        };
        let mut out: String = String::new();
        generate_lua_guest_host_contract_caller(&mut out, &contract, &[])
            .expect("caller generation must succeed");
        assert!(
            out.contains("HostLoggerContract = {}"),
            "missing class table: {out}"
        );
        assert!(
            out.contains("HostLoggerContract.__index = HostLoggerContract"),
            "missing __index: {out}"
        );
        assert!(
            out.contains("function HostLoggerContract:new(interface, instance)"),
            "missing new method: {out}"
        );
        assert!(
            out.contains("function HostLoggerContract.from_host(host_ptr, min_version)"),
            "missing from_host: {out}"
        );
        assert!(
            out.contains("function HostLoggerContract:is_valid()"),
            "missing is_valid: {out}"
        );
        assert!(
            out.contains("function HostLoggerContract:log(level, message)"),
            "missing log method (colon syntax binds self implicitly — no explicit self param): {out}"
        );
        // Defect (a): the caller must cast to the canonical flat HostContractInterface,
        // never the nonexistent HostContractVTable, and read dispatch metadata directly.
        assert!(
            out.contains("ffi.cast(\"HostContractInterface*\", self._interface)"),
            "must cast to HostContractInterface: {out}"
        );
        assert!(
            !out.contains("HostContractVTable"),
            "must not reference the nonexistent HostContractVTable: {out}"
        );
        assert!(
            !out.contains(".header."),
            "must not read through a nonexistent .header field: {out}"
        );
        // Host contracts carry their generated bridge in user_data, which is
        // forwarded as the VM callback's adapter context before loader data.
        assert!(
            out.contains(
                "interface.dispatch.vm.call(interface.user_data, interface.dispatch.vm.loader_data,"
            ),
            "must call vm.call with user_data adapter context before loader data: {out}"
        );
        // from_host resolves the interface via resolve_host_contract_interface and the
        // instance via get_host_contract, matching the canonical Rust caller.
        assert!(
            out.contains("host.resolve_host_contract_interface(host,"),
            "from_host must resolve the interface vtable: {out}"
        );
        assert!(
            out.contains("host.get_host_contract(host,"),
            "from_host must obtain the per-instance state: {out}"
        );
        // host_ptr (a plain Lua number) must be cast through uintptr_t before use,
        // matching the host-caller path; a direct ffi.cast("HostApi*", number) is
        // rejected by LuaJIT as the first FFI argument.
        assert!(
            out.contains("ffi.cast(\"HostApi*\", ffi.cast(\"uintptr_t\", host_ptr))"),
            "from_host must cast host_ptr through uintptr_t: {out}"
        );
    }

    #[test]
    fn generate_lua_guest_host_contract_caller_with_return() {
        let contract: ResolvedHostContract = ResolvedHostContract {
            name: "host.fs.reader".to_owned(),
            contract_id: 0xDEADBEEF,
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
                    docs: None,
                }],
                returns: Some(ResolvedTypeRef::AbiType(AbiBuiltin::Buffer)),
                docs: None,
                return_docs: None,
            }],
            docs: None,
        };
        let mut out: String = String::new();
        generate_lua_guest_host_contract_caller(&mut out, &contract, &[])
            .expect("caller generation must succeed");
        assert!(
            out.contains("HostFsReaderContract = {}"),
            "missing class table: {out}"
        );
        assert!(
            out.contains("function HostFsReaderContract:read(path)"),
            "missing read method (colon syntax binds self implicitly — no explicit self param): {out}"
        );
        assert!(
            out.contains("return out_val"),
            "missing return statement: {out}"
        );
    }

    #[test]
    fn generate_guest_host_contracts_file_empty() {
        let ir: ValidatedIr = ValidatedIr {
            types: vec![],
            enums: vec![],
            contracts: vec![],
            host_contracts: vec![],
            bundle: None,
        };
        let result: String = generate_guest_host_contracts_file(&ir)
            .expect("host contracts generation must succeed");
        assert!(result.contains("local ffi = require(\"ffi\")"));
        assert!(result.contains("local M = {}"));
        assert!(result.contains("return M"));
        assert!(!result.contains("HostLoggerContract"));
    }

    #[test]
    fn generate_guest_host_contracts_file_with_contract() {
        let contract: ResolvedHostContract = ResolvedHostContract {
            name: "host.logger".to_owned(),
            contract_id: 0x123456789ABCDEF0,
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
                docs: None,
                return_docs: None,
            }],
            docs: None,
        };
        let ir: ValidatedIr = ValidatedIr {
            types: vec![],
            enums: vec![],
            contracts: vec![],
            host_contracts: vec![contract],
            bundle: None,
        };
        let result: String = generate_guest_host_contracts_file(&ir)
            .expect("host contracts generation must succeed");
        assert!(result.contains("HostLoggerContract = {}"));
        assert!(result.contains("M.HOSTLOGGERCONTRACT_ID = 0x123456789ABCDEF0ULL"));
        assert!(result.contains("M.HostLoggerContract = HostLoggerContract"));
    }

    // ─── Guest Peer Caller Tests ───────────────────────────────────────────────

    #[test]
    fn peer_caller_emitted_for_declared_dependency() {
        let contract: ResolvedContract = ResolvedContract {
            name: "pipeline.Validator".to_owned(),
            contract_id: 0xAAAA_BBBB_CCCC_DDDD_u64,
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
                    docs: None,
                }],
                returns: Some(ResolvedTypeRef::AbiType(AbiBuiltin::StringView)),
                docs: None,
                return_docs: None,
            }],
            docs: None,
        };

        let ir: ValidatedIr = ValidatedIr {
            types: vec![],
            enums: vec![],
            contracts: vec![contract],
            host_contracts: vec![],
            bundle: Some(ResolvedBundle {
                name: "test.bundle".to_owned(),
                version: Version {
                    major: 1,
                    minor: 0,
                    patch: 0,
                },
                loader: "lua".to_owned(),
                file: ResolvedBundleFile::Single("test.lua".to_owned()),
                plugins: vec![ResolvedPlugin {
                    name: "test_plugin".to_owned(),
                    implements: vec!["data.Transformer@1.0".to_owned()],
                    optional: vec![],
                }],
                bundle_id: 0x1234_5678_9ABC_DEF0_u64,
                dependencies: vec![ResolvedDependency::ByContract {
                    contract: "pipeline.Validator".to_owned(),
                    contract_id: 0xAAAA_BBBB_CCCC_DDDD_u64,
                    min_version: 1,
                }],
                needs_reinit_on_dep_reload: false,
            }),
        };

        let peers: Vec<&ResolvedContract> = collect_peer_contracts(&ir);
        assert!(
            !peers.is_empty(),
            "should find peer contract for declared dependency"
        );

        let mut out: String = String::new();
        generate_lua_guest_peer_caller(&mut out, peers[0], 1, &ir.enums);

        assert!(
            out.contains("PipelineValidatorPeer = {}"),
            "missing peer class table: {out}"
        );
        assert!(
            out.contains("PipelineValidatorPeer.__index = PipelineValidatorPeer"),
            "missing __index: {out}"
        );
        assert!(
            out.contains("function PipelineValidatorPeer.resolve(host_ptr)"),
            "missing resolve factory: {out}"
        );
        assert!(
            !out.contains("polyplug_guest.get_host_interface()"),
            "resolve must thread host_ptr explicitly, not read a global: {out}"
        );
        assert!(
            out.contains("ffi.cast(\"HostApi*\", ffi.cast(\"uintptr_t\", host_ptr))"),
            "host_ptr must be cast through uintptr_t: {out}"
        );
        assert!(
            out.contains("host.find_guest_contract(host,"),
            "must call find_guest_contract: {out}"
        );
        assert!(
            out.contains("host.resolve_guest_contract(host, handle)"),
            "must call resolve_guest_contract: {out}"
        );
        assert!(
            out.contains("host.create_guest_instance(host, interface, nil, instance)"),
            "must call create_guest_instance with out-param: {out}"
        );
        assert!(
            out.contains("function PipelineValidatorPeer:validate(input)"),
            "missing validate method: {out}"
        );
        // Direct cached-interface dispatch.
        // Branch on the cached interface's dispatch_type.
        assert!(
            out.contains("if dispatch_type == 0 then"),
            "peer must branch on dispatch_type: {out}"
        );
        // Native arm: call the guest function pointer directly via NativeDispatchFnType.
        assert!(
            out.contains("local fn_ptr = interface.dispatch.native.functions["),
            "native arm must read the function pointer from the cached interface: {out}"
        );
        assert!(
            out.contains("ffi.cast(NativeDispatchFnType, fn_ptr)"),
            "native arm must cast through the shared NativeDispatchFnType ctype: {out}"
        );
        assert!(
            out.contains("fn(interface.adapter_context, self._instance, args_ptr, out_ptr, err)"),
            "native arm must forward adapter context before the instance: {out}"
        );
        // VM arm: call the loader trampoline with the immutable interface context
        // and a nil arena. Lua peer callers have no per-caller CallArena.
        assert!(
            out.contains(
                "interface.dispatch.vm.call(interface.adapter_context, interface.dispatch.vm.loader_data, self._instance,"
            ),
            "vm arm must pass adapter_context before loader data and the instance: {out}"
        );
        assert!(
            out.contains("return out_val"),
            "missing return statement: {out}"
        );
    }

    #[test]
    fn no_peer_callers_without_dependencies() {
        let contract: ResolvedContract = ResolvedContract {
            name: "pipeline.Validator".to_owned(),
            contract_id: 0xAAAA_BBBB_CCCC_DDDD_u64,
            version: Version {
                major: 1,
                minor: 0,
                patch: 0,
            },
            functions: vec![],
            docs: None,
        };

        // No bundle at all — no peer contracts.
        let ir_no_bundle: ValidatedIr = ValidatedIr {
            types: vec![],
            enums: vec![],
            contracts: vec![contract],
            host_contracts: vec![],
            bundle: None,
        };
        let peers: Vec<&ResolvedContract> = collect_peer_contracts(&ir_no_bundle);
        assert!(
            peers.is_empty(),
            "should produce no peers when there is no bundle"
        );

        // Bundle with no dependencies — no peer contracts even if contracts exist.

        let contract2: ResolvedContract = ResolvedContract {
            name: "pipeline.Validator".to_owned(),
            contract_id: 0xAAAA_BBBB_CCCC_DDDD_u64,
            version: Version {
                major: 1,
                minor: 0,
                patch: 0,
            },
            functions: vec![],
            docs: None,
        };
        let ir_no_deps: ValidatedIr = ValidatedIr {
            types: vec![],
            enums: vec![],
            contracts: vec![contract2],
            host_contracts: vec![],
            bundle: Some(ResolvedBundle {
                name: "test.bundle".to_owned(),
                version: Version {
                    major: 1,
                    minor: 0,
                    patch: 0,
                },
                loader: "lua".to_owned(),
                file: ResolvedBundleFile::Single("test.lua".to_owned()),
                plugins: vec![ResolvedPlugin {
                    name: "test_plugin".to_owned(),
                    implements: vec!["data.Transformer@1.0".to_owned()],
                    optional: vec![],
                }],
                bundle_id: 0x1234_5678_9ABC_DEF0_u64,
                dependencies: vec![],
                needs_reinit_on_dep_reload: false,
            }),
        };
        let peers2: Vec<&ResolvedContract> = collect_peer_contracts(&ir_no_deps);
        assert!(
            peers2.is_empty(),
            "should produce no peers when bundle has no declared dependencies"
        );
    }

    // ─── Scalar out-slot tests ─────────────────────────────────────────────────

    #[test]
    fn host_out_setup_scalar_u32_emits_array_slot() {
        // A u32 return is scalar: out slot must be ffi.new("uint32_t[1]") and
        // the caller must read back with out_val[0].

        let func: ResolvedFunction = ResolvedFunction {
            name: "get_count".to_owned(),
            function_id: 0,
            params: vec![],
            returns: Some(ResolvedTypeRef::Primitive(PrimitiveType::U32)),
            docs: None,
            return_docs: None,
        };
        let contract: ResolvedContract = ResolvedContract {
            name: "data.Counter".to_owned(),
            contract_id: 0x1111_2222_3333_4444_u64,
            version: Version {
                major: 1,
                minor: 0,
                patch: 0,
            },
            functions: vec![func],
            docs: None,
        };
        let ir: ValidatedIr = ValidatedIr {
            types: vec![],
            enums: vec![],
            contracts: vec![contract],
            host_contracts: vec![],
            bundle: None,
        };
        let out: String = generate_host_callers_file(&ir);
        assert!(
            out.contains("ffi.new(ctype(\"uint32_t[1]\"))"),
            "scalar u32 return must use a 1-element array slot: {out}"
        );
        assert!(
            out.contains("return out_val[0]"),
            "scalar u32 return must read result with out_val[0]: {out}"
        );
        assert!(
            !out.contains("ffi.new(ctype(\"uint32_t\"))"),
            "scalar u32 must NOT use a bare value slot (would yield NULL out_ptr): {out}"
        );
    }

    #[test]
    fn host_out_setup_string_view_keeps_struct_slot() {
        // A StringView return is a struct (reference cdata): out slot must stay
        // ffi.new("StringView") and the caller must return the raw handle.

        let func: ResolvedFunction = ResolvedFunction {
            name: "get_name".to_owned(),
            function_id: 0,
            params: vec![],
            returns: Some(ResolvedTypeRef::AbiType(AbiBuiltin::StringView)),
            docs: None,
            return_docs: None,
        };
        let contract: ResolvedContract = ResolvedContract {
            name: "data.Namer".to_owned(),
            contract_id: 0xAAAA_BBBB_1111_2222_u64,
            version: Version {
                major: 1,
                minor: 0,
                patch: 0,
            },
            functions: vec![func],
            docs: None,
        };
        let ir: ValidatedIr = ValidatedIr {
            types: vec![],
            enums: vec![],
            contracts: vec![contract],
            host_contracts: vec![],
            bundle: None,
        };
        let out: String = generate_host_callers_file(&ir);
        assert!(
            out.contains("ffi.new(ctype(\"StringView\"))"),
            "StringView return must use a bare struct slot: {out}"
        );
        assert!(
            !out.contains("ffi.new(ctype(\"StringView[1]\"))"),
            "StringView must NOT use an array slot: {out}"
        );
        // "return out_val" is the expected form; "return out_val[0]" must NOT appear.
        assert!(
            !out.contains("return out_val[0]"),
            "StringView return must NOT use out_val[0]: {out}"
        );
        assert!(
            out.contains("return out_val"),
            "StringView return must use return out_val: {out}"
        );
    }

    #[test]
    fn internal_profile_emits_consuming_namespaced_typed_lua_facade() {
        let contract = ResolvedContract {
            name: "shape.Plugin".to_owned(),
            contract_id: 0xD0A1,
            version: Version {
                major: 1,
                minor: 0,
                patch: 0,
            },
            functions: vec![
                ResolvedFunction {
                    name: "write".to_owned(),
                    function_id: 0,
                    params: vec![
                        ResolvedParam {
                            name: "label".to_owned(),
                            ty: ResolvedTypeRef::AbiType(AbiBuiltin::StringView),
                            docs: None,
                        },
                        ResolvedParam {
                            name: "mode".to_owned(),
                            ty: ResolvedTypeRef::UserDefined("Mode".to_owned()),
                            docs: None,
                        },
                    ],
                    returns: Some(ResolvedTypeRef::AbiType(AbiBuiltin::Buffer)),
                    docs: None,
                    return_docs: None,
                },
                ResolvedFunction {
                    name: "inspect".to_owned(),
                    function_id: 1,
                    params: vec![ResolvedParam {
                        name: "envelope".to_owned(),
                        ty: ResolvedTypeRef::UserDefined("Envelope".to_owned()),
                        docs: None,
                    }],
                    returns: Some(ResolvedTypeRef::UserDefined("Envelope".to_owned())),
                    docs: None,
                    return_docs: None,
                },
            ],
            docs: None,
        };
        let ir = ValidatedIr {
            types: vec![ResolvedType {
                name: "Envelope".to_owned(),
                fields: vec![ResolvedField {
                    name: "payload".to_owned(),
                    ty: ResolvedTypeRef::AbiType(AbiBuiltin::Buffer),
                    docs: None,
                }],
                docs: None,
            }],
            enums: vec![EnumDef {
                name: "Mode".to_owned(),
                repr: ReprType::U32,
                bitflag: false,
                variants: vec![EnumVariant {
                    name: "Ready".to_owned(),
                    value: "0".to_owned(),
                    docs: None,
                }],
                docs: None,
            }],
            contracts: vec![contract],
            host_contracts: vec![],
            bundle: Some(ResolvedBundle {
                name: "lua-internal-profile".to_owned(),
                version: Version {
                    major: 1,
                    minor: 0,
                    patch: 0,
                },
                loader: String::new(),
                file: ResolvedBundleFile::Single(String::new()),
                plugins: vec![ResolvedPlugin {
                    name: "shape_provider".to_owned(),
                    implements: vec!["shape.Plugin@1.0".to_owned()],
                    optional: vec![],
                }],
                bundle_id: 0xD0A1_D0A1_D0A1_D0A1_u64,
                dependencies: vec![],
                needs_reinit_on_dep_reload: false,
            }),
        };
        let mut files = GeneratedFiles::default();
        LuaGenerator
            .generate_internal_bundle(&ir, "lua-internal-profile", &mut files)
            .expect("generate Lua internal profile");
        let profile = files
            .files
            .iter()
            .find(|file| file.path == *"guest/internal.lua")
            .expect("internal facade")
            .content
            .as_str();
        assert!(
            profile.contains("providers were consumed by a previous registration attempt")
                && profile.contains("must be a factory function")
                && profile.contains("runtime:register_internal_plugin(resident)")
                && profile.contains("result[\"shape_provider_shape_Plugin\"]"),
            "profile must consume factory-only providers, use canonical registration, and return named callers: {profile}"
        );
        assert!(
            profile.contains(
                "local packed = ffi.cast(\"const ShapePluginContractWriteArgs*\", args)[0]"
            ) && profile.contains("ffi.cast(buffer_ptr_t, out_ptr)[0] = result")
                && profile.contains("local output = ffi.cast(\"Envelope*\", out_ptr)")
                && profile.contains("return_roots.buffers[#return_roots.buffers + 1]")
                && profile.contains("return_roots.strings = {}"),
            "profile must preserve multi-arg, buffer, nested-struct, and bounded variable-return shapes: {profile}"
        );
        let callers = files
            .files
            .iter()
            .find(|file| file.path == *"host/callers.lua")
            .expect("internal callers")
            .content
            .as_str();
        assert!(
            callers.contains(
                "local native_bridge = require(\"polyplug.loaders.lua\").internal_plugin_bridge()"
            ),
            "internal callers must load the native lifecycle gateway: {callers}"
        );
        assert!(
            callers.contains(
                "local handle = runtime:find_guest_contract(SHAPE_PLUGIN_CONTRACT_ID, 0)"
            ) && callers.contains("host.create_guest_instance(host, interface, nil, instance)")
                && callers.contains("native_bridge.caller_create_with_implementation")
                && callers.contains("native_bridge.caller_destroy"),
            "ordinary lookup callers must preserve generic ABI dispatch while exact internal callers use validated lifecycle gateways: {callers}"
        );
        assert!(
            !files.files.iter().any(|file| file.path == *"manifest.toml"),
            "internal profile must not synthesize an external artifact manifest"
        );
    }

    #[test]
    fn internal_profile_scalar_return_slots_match_the_abi() {
        for (c_type, pointer_type) in [
            ("uint64_t", "uint64_ptr_t"),
            ("uint32_t", "uint32_ptr_t"),
            ("uint16_t", "uint16_ptr_t"),
            ("uint8_t", "uint8_ptr_t"),
            ("int64_t", "int64_ptr_t"),
            ("int32_t", "int32_ptr_t"),
            ("int16_t", "int16_ptr_t"),
            ("int8_t", "int8_ptr_t"),
            ("float", "float_ptr_t"),
            ("double", "double_ptr_t"),
            ("bool", "bool_ptr_t"),
            ("void*", "void_ptr_ptr_t"),
        ] {
            assert_eq!(lua_internal_scalar_pointer_type(c_type), pointer_type);
        }
    }

    #[test]
    fn internal_profile_array_returns_store_integer_addresses() {
        let mut primitives = String::new();
        let mut primitive_context = LuaInternalMarshalContext {
            types: &[],
            enums: &[],
            uid: 0,
        };
        emit_lua_internal_profile_marshal_array(
            &mut primitives,
            "output",
            "result",
            "u32",
            "",
            &mut primitive_context,
        );
        assert!(
            primitives.contains(
                "output.items = ffi.cast(\"uint64_t\", ffi.cast(\"uintptr_t\", values_0))"
            ),
            "primitive arrays must store a uint64 ABI address: {primitives}"
        );

        let pair = ResolvedType {
            name: "Pair".to_owned(),
            fields: vec![ResolvedField {
                name: "value".to_owned(),
                ty: ResolvedTypeRef::Primitive(PrimitiveType::U32),
                docs: None,
            }],
            docs: None,
        };
        let mut structs = String::new();
        let mut struct_context = LuaInternalMarshalContext {
            types: &[pair],
            enums: &[],
            uid: 0,
        };
        emit_lua_internal_profile_marshal_array(
            &mut structs,
            "output",
            "result",
            "Pair",
            "",
            &mut struct_context,
        );
        assert!(
            structs.contains(
                "output.items = ffi.cast(\"uint64_t\", ffi.cast(\"uintptr_t\", values_0))"
            ),
            "struct arrays must store a uint64 ABI address: {structs}"
        );
    }

    // ─── Host Interface Factory Tests ──────────────────────────────────────────

    fn host_logger_ir() -> ValidatedIr {
        ValidatedIr {
            types: vec![],
            enums: vec![EnumDef {
                name: "LogLevel".to_owned(),
                repr: ReprType::U32,
                bitflag: false,
                variants: vec![EnumVariant {
                    name: "Info".to_owned(),
                    value: "1".to_owned(),
                    docs: None,
                }],
                docs: None,
            }],
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
                functions: vec![
                    ResolvedFunction {
                        name: "log".to_owned(),
                        function_id: 0,
                        params: vec![ResolvedParam {
                            name: "message".to_owned(),
                            ty: ResolvedTypeRef::AbiType(AbiBuiltin::StringView),
                            docs: None,
                        }],
                        returns: None,
                        docs: None,
                        return_docs: None,
                    },
                    ResolvedFunction {
                        name: "log_with_level".to_owned(),
                        function_id: 1,
                        params: vec![
                            ResolvedParam {
                                name: "level".to_owned(),
                                ty: ResolvedTypeRef::UserDefined("LogLevel".to_owned()),
                                docs: None,
                            },
                            ResolvedParam {
                                name: "message".to_owned(),
                                ty: ResolvedTypeRef::AbiType(AbiBuiltin::StringView),
                                docs: None,
                            },
                        ],
                        returns: None,
                        docs: None,
                        return_docs: None,
                    },
                ],
                docs: None,
            }],
            bundle: None,
        }
    }

    /// The factory must populate the REAL ABI `HostContractInterface` struct —
    /// the old output wrote `interface.header.*` fields on a fictional
    /// `HostContractVTable` that no cdef ever defined.
    #[test]
    fn lua_host_interface_factory_uses_real_abi_struct() {
        let out: String = generate_lua_host_interface_factories_file(&host_logger_ir());
        assert!(
            out.contains("ffi.new(\"HostContractInterface\")"),
            "factory must build the real ABI struct: {out}"
        );
        assert!(
            !out.contains("HostContractVTable"),
            "fictional HostContractVTable must be gone: {out}"
        );
        assert!(
            !out.contains("interface.header"),
            "HostContractInterface has no header wrapper: {out}"
        );
        assert!(
            out.contains("interface.contract_version.major = 1"),
            "version must be set on the real field: {out}"
        );
        assert!(
            out.contains("interface.singleton = 0  -- multi-instance"),
            "singleton must be a numeric uint8_t value: {out}"
        );
        assert!(
            out.contains(
                "interface.dispatch.vm.call = lua_bridge_lib.polyplug_lua_host_vm_dispatch"
            ),
            "dispatch must route through the lua loader trampoline: {out}"
        );
        assert!(
            out.contains("interface.create_instance = create_cb"),
            "create_instance must use the Lua per-instance callback: {out}"
        );
    }

    /// Toolchain-free conformance floor for the out-param ABI bug class.
    ///
    /// LuaJIT's `ffi.cdef` is the ONLY place in the whole codebase where a
    /// generator hand-types an ABI function-pointer signature as literal text:
    /// every other language is checked by its own compiler against the
    /// auto-regenerated mirror (cpp/csharp/rust) or derives the signature from
    /// the mirror field at runtime (python `type(field)`, js typed-Rust install).
    /// LuaJIT structurally cannot do either — it needs literal C text — so a
    /// drift between these cdefs and the real trampolines in
    /// `crates/polyplug_lua/src/ffi.rs` is invisible to `cargo build`/`clippy`
    /// and only manifests at runtime. This test is the floor that catches that
    /// drift with nothing but `cargo test` (no luajit, no version matching):
    /// it pins the exact out-param signatures and forbids the by-value forms.
    #[test]
    fn lua_host_trampoline_cdefs_are_out_param_abi() {
        let out: String = generate_lua_host_interface_factories_file(&host_logger_ir());
        // Exact out-param signatures — must match the `extern "C"` trampolines in
        // crates/polyplug_lua/src/ffi.rs (void return + trailing out-pointer).
        assert!(
            out.contains(
                "void polyplug_lua_host_vm_dispatch(void*, VmLoaderData, GuestContractInstance, uint32_t, const void*, void*, CallArena*, AbiError*);"
            ),
            "vm_dispatch cdef must carry adapter_context and use the out-param ABI: {out}"
        );
        assert!(
            out.contains(
                "void polyplug_lua_host_destroy_instance(const HostContractInterface*, HostContractInstance);"
            ),
            "destroy_instance cdef must be void with no out-param: {out}"
        );
        // The dispatch callback gains an instance_data first arg (routes to the
        // per-instance impl) and the bridge carries a destroy_callback that the
        // native destroy_instance trampoline invokes — both must match
        // PolyplugLuaHostDispatchBridge in crates/polyplug_lua/src/ffi.rs.
        assert!(
            out.contains(
                "typedef uint32_t (*PolyplugLuaHostDispatchCallback)(void* /*instance_data*/, uint32_t, const void*, void*);"
            ),
            "dispatch callback cdef must take instance_data as the first arg: {out}"
        );
        assert!(
            out.contains(
                "typedef void (*PolyplugLuaHostDestroyCallback)(void* /*instance_data*/);"
            ),
            "destroy callback typedef must be cdef'd: {out}"
        );
        assert!(
            out.contains("PolyplugLuaHostDestroyCallback destroy_callback;"),
            "bridge struct must carry the destroy_callback field: {out}"
        );
        // create_instance is now a Lua callback (typedef'd for the cast), NOT a
        // native trampoline — the old native export cdef must be gone.
        assert!(
            out.contains(
                "typedef void (*PolyplugLuaHostCreateInstanceFn)(const HostContractInterface*, const void*, HostContractInstance*);"
            ),
            "create_instance Lua-callback typedef must be cdef'd: {out}"
        );
        assert!(
            !out.contains("void polyplug_lua_host_create_instance("),
            "the native create_instance trampoline cdef is superseded by a Lua callback — must be gone: {out}"
        );
        // The stale by-value returns that this floor exists to prevent.
        assert!(
            !out.contains("AbiError polyplug_lua_host_vm_dispatch("),
            "by-value AbiError return is the regressed form — must never reappear: {out}"
        );
        assert!(
            !out.contains("HostContractInstance polyplug_lua_host_create_instance("),
            "by-value instance return is the regressed form — must never reappear: {out}"
        );
    }

    /// Multi-parameter functions must cast to an arg-pack struct that the SAME
    /// file cdefs (guarded), using the canonical pack-struct naming — the old
    /// output cast to `LOG_WITH_LEVELArgs*` which was never cdef'd anywhere.
    #[test]
    fn lua_host_interface_factory_cdefs_arg_pack_structs() {
        let out: String = generate_lua_host_interface_factories_file(&host_logger_ir());
        assert!(
            out.contains("} HostLoggerLogWithLevelArgs;"),
            "arg-pack struct must be cdef'd in the factories file: {out}"
        );
        assert!(
            out.contains("ffi.cast(\"const HostLoggerLogWithLevelArgs*\", args)"),
            "dispatcher must cast to the cdef'd pack struct: {out}"
        );
        assert!(
            !out.contains("LOG_WITH_LEVELArgs"),
            "uppercased never-cdef'd pack name must be gone: {out}"
        );
    }

    /// Contract enums are Lua TABLES — there is no cdef'd `LogLevel` C type.
    /// Pack fields and single-param casts must use the repr's C integer type;
    /// naming the enum only ever worked by colliding with the ABI's own
    /// `LogLevel` cdef in abi.lua (any other enum name fails the cdef).
    #[test]
    fn lua_host_interface_factory_enum_fields_use_repr_ctype() {
        let out: String = generate_lua_host_interface_factories_file(&host_logger_ir());
        assert!(
            out.contains("uint32_t level;"),
            "enum pack fields must use the repr C type: {out}"
        );
        assert!(
            !out.contains("LogLevel level;"),
            "enum pack fields must not name the (never-cdef'd) enum: {out}"
        );
    }

    /// Single enum params hit the bare-value cast path — it must also use the
    /// repr C type, not the enum name.
    #[test]
    fn lua_host_dispatch_single_enum_param_uses_repr_ctype() {
        let enums: Vec<EnumDef> = vec![EnumDef {
            name: "LogLevel".to_owned(),
            repr: ReprType::U32,
            bitflag: false,
            variants: vec![EnumVariant {
                name: "Info".to_owned(),
                value: "1".to_owned(),
                docs: None,
            }],
            docs: None,
        }];
        let func: ResolvedFunction = ResolvedFunction {
            name: "set_level".to_owned(),
            function_id: 0,
            params: vec![ResolvedParam {
                name: "level".to_owned(),
                ty: ResolvedTypeRef::UserDefined("LogLevel".to_owned()),
                docs: None,
            }],
            returns: None,
            docs: None,
            return_docs: None,
        };
        let mut out: String = String::new();
        generate_lua_host_dispatch_args(&mut out, "HostLogger", &func, &enums);
        assert!(
            out.contains("ffi.cast(\"const uint32_t*\", args)[0]"),
            "single enum param must cast to the repr C type: {out}"
        );
        assert!(
            !out.contains("const LogLevel*"),
            "the enum name has no cdef and must not be cast to: {out}"
        );
    }

    /// The dispatcher must be plain Lua — the old output emitted
    /// `local level: userdata = ...`, which is not Lua syntax at all.
    #[test]
    fn lua_host_interface_factory_emits_valid_lua_syntax() {
        let out: String = generate_lua_host_interface_factories_file(&host_logger_ir());
        assert!(
            !out.contains(": userdata"),
            "type-annotation syntax is not Lua: {out}"
        );
        assert!(
            out.contains("local level = packed.level"),
            "pack fields must be extracted with plain assignments: {out}"
        );
        // Every generated factory line must survive a Lua parse: no `local x: T`.
        for line in out.lines() {
            let trimmed: &str = line.trim_start();
            if let Some(rest) = trimmed.strip_prefix("local ") {
                assert!(
                    !rest
                        .split('=')
                        .next()
                        .is_some_and(|lhs: &str| lhs.contains(':')),
                    "invalid Lua type annotation in generated line: {line}"
                );
            }
        }
    }

    // ─── Caller-side enum marshalling (repr-integer slots) ──────────────────────
    //
    // Enums are emitted as plain Lua tables (numbers at the call site), so a
    // caller must NEVER cast the bare value to void* (value-as-address). Params
    // go through a repr-integer 1-element array slot whose ADDRESS is passed;
    // returns use a repr-integer slot read back with tonumber().

    fn pixel_format_enums() -> Vec<EnumDef> {
        vec![EnumDef {
            name: "PixelFormat".to_owned(),
            repr: ReprType::U32,
            bitflag: false,
            variants: vec![
                EnumVariant {
                    name: "Unknown".to_owned(),
                    value: "0".to_owned(),
                    docs: None,
                },
                EnumVariant {
                    name: "Rgba8".to_owned(),
                    value: "1".to_owned(),
                    docs: None,
                },
            ],
            docs: None,
        }]
    }

    fn enum_codec_contract() -> ResolvedContract {
        ResolvedContract {
            name: "image.Codec".to_owned(),
            contract_id: 0x1111_2222_3333_4444_u64,
            version: Version {
                major: 1,
                minor: 0,
                patch: 0,
            },
            functions: vec![
                ResolvedFunction {
                    name: "set_format".to_owned(),
                    function_id: 0,
                    params: vec![ResolvedParam {
                        name: "fmt".to_owned(),
                        ty: ResolvedTypeRef::UserDefined("PixelFormat".to_owned()),
                        docs: None,
                    }],
                    returns: None,
                    docs: None,
                    return_docs: None,
                },
                ResolvedFunction {
                    name: "get_format".to_owned(),
                    function_id: 1,
                    params: vec![],
                    returns: Some(ResolvedTypeRef::UserDefined("PixelFormat".to_owned())),
                    docs: None,
                    return_docs: None,
                },
            ],
            docs: None,
        }
    }

    fn assert_enum_caller_marshalling(out: &str, out_slot: &str) {
        // (i) single-enum param: repr-integer slot + address pass.
        assert!(
            out.contains("local fmt_val = ffi.new(\"uint32_t[1]\", fmt)"),
            "enum param must be written into a repr-integer slot: {out}"
        );
        assert!(
            out.contains("local args_ptr = ffi.cast(\"const void*\", fmt_val)"),
            "enum param must pass the slot's address: {out}"
        );
        assert!(
            !out.contains("ffi.cast(\"const void*\", fmt )")
                && !out.contains("ffi.cast(\"const void*\", fmt)"),
            "bare enum value must never be cast to void* (value-as-address): {out}"
        );
        // (ii) enum return: repr-integer out slot + tonumber() read-back.
        assert!(
            out.contains(&format!("local out_val = {out_slot}")),
            "enum return must allocate a repr-integer out slot: {out}"
        );
        assert!(
            out.contains("return tonumber(out_val[0])"),
            "enum return must be read back with tonumber(): {out}"
        );
        assert!(
            !out.contains("ffi.new(\"PixelFormat\""),
            "enum has no cdef'd C type — must use the repr integer: {out}"
        );
    }

    #[test]
    fn lua_host_caller_enum_param_and_return_use_repr_slots() {
        let mut out: String = String::new();
        generate_host_contract_caller(&mut out, &enum_codec_contract(), &pixel_format_enums());
        assert_enum_caller_marshalling(&out, "ffi.new(ctype(\"uint32_t[1]\"))");
    }

    #[test]
    fn lua_peer_caller_enum_param_and_return_use_repr_slots() {
        let mut out: String = String::new();
        generate_lua_guest_peer_caller(&mut out, &enum_codec_contract(), 1, &pixel_format_enums());
        assert_enum_caller_marshalling(&out, "ffi.new(\"uint32_t[1]\")");
    }

    #[test]
    fn lua_guest_host_contract_caller_enum_param_and_return_use_repr_slots() {
        let contract: ResolvedHostContract = ResolvedHostContract {
            name: "host.theme".to_owned(),
            contract_id: 0xDEAD_BEEF_u64,
            version: Version {
                major: 1,
                minor: 0,
                patch: 0,
            },
            singleton: false,
            functions: vec![
                ResolvedFunction {
                    name: "set_mode".to_owned(),
                    function_id: 0,
                    params: vec![ResolvedParam {
                        name: "fmt".to_owned(),
                        ty: ResolvedTypeRef::UserDefined("PixelFormat".to_owned()),
                        docs: None,
                    }],
                    returns: None,
                    docs: None,
                    return_docs: None,
                },
                ResolvedFunction {
                    name: "get_mode".to_owned(),
                    function_id: 1,
                    params: vec![],
                    returns: Some(ResolvedTypeRef::UserDefined("PixelFormat".to_owned())),
                    docs: None,
                    return_docs: None,
                },
            ],
            docs: None,
        };
        let mut out: String = String::new();
        generate_lua_guest_host_contract_caller(&mut out, &contract, &pixel_format_enums())
            .expect("caller generation must succeed");
        assert_enum_caller_marshalling(&out, "ffi.new(\"uint32_t[1]\")");
    }

    /// Scalar single params share the same LuaJIT pitfall: a scalar value cdata
    /// cast to void* converts the VALUE, not its address — so the caller must
    /// use the 1-element array form just like scalar out slots.
    #[test]
    fn lua_host_caller_single_scalar_param_uses_array_slot() {
        let contract: ResolvedContract = ResolvedContract {
            name: "counter.Inc".to_owned(),
            contract_id: 0x5555_6666_u64,
            version: Version {
                major: 1,
                minor: 0,
                patch: 0,
            },
            functions: vec![ResolvedFunction {
                name: "inc".to_owned(),
                function_id: 0,
                params: vec![ResolvedParam {
                    name: "amount".to_owned(),
                    ty: ResolvedTypeRef::Primitive(PrimitiveType::U32),
                    docs: None,
                }],
                returns: None,
                docs: None,
                return_docs: None,
            }],
            docs: None,
        };
        let mut out: String = String::new();
        generate_host_contract_caller(&mut out, &contract, &[]);
        assert!(
            out.contains("local amount_val = ffi.new(\"uint32_t[1]\", amount)"),
            "scalar param must use a 1-element array slot: {out}"
        );
        assert!(
            !out.contains("ffi.new(\"uint32_t\", amount)"),
            "scalar value cdata cast to void* is value-as-address: {out}"
        );
    }

    // ─── Guest handler: arg unpacking + return marshalling (W3 3a) ──────────────

    fn guest_handler(func: &ResolvedFunction, enums: &[EnumDef]) -> String {
        let mut out: String = String::new();
        emit_lua_guest_handler_body(&mut out, func, enums, "test.add", &[]);
        out
    }

    fn scalar_param(name: &str, prim: PrimitiveType) -> ResolvedParam {
        ResolvedParam {
            name: name.to_owned(),
            ty: ResolvedTypeRef::Primitive(prim),
            docs: None,
        }
    }

    #[test]
    fn lua_guest_handler_scalar_arg_unpacks_typed_slot() {
        let func: ResolvedFunction = ResolvedFunction {
            name: "scale".to_owned(),
            function_id: 0,
            params: vec![scalar_param("amount", PrimitiveType::U32)],
            returns: Some(ResolvedTypeRef::Primitive(PrimitiveType::U32)),
            docs: None,
            return_docs: None,
        };
        let out: String = guest_handler(&func, &[]);
        assert!(
            out.contains(
                "local args_val = ffi.cast(\"const uint32_t*\", ffi.cast(\"uintptr_t\", args_ptr))"
            ),
            "scalar arg must be cast back to its typed slot: {out}"
        );
        assert!(
            out.contains("local result = instance:scale(args_val[0])"),
            "impl must receive the unpacked scalar value, not raw pointers: {out}"
        );
        assert!(
            !out.contains("instance:scale(args_ptr, out_ptr)"),
            "scalar arg must NOT fall back to raw pointer pass-through: {out}"
        );
    }

    #[test]
    fn lua_guest_handler_buffer_arg_unpacks_cdata() {
        let func: ResolvedFunction = ResolvedFunction {
            name: "store".to_owned(),
            function_id: 0,
            params: vec![ResolvedParam {
                name: "data".to_owned(),
                ty: ResolvedTypeRef::AbiType(AbiBuiltin::Buffer),
                docs: None,
            }],
            returns: None,
            docs: None,
            return_docs: None,
        };
        let out: String = guest_handler(&func, &[]);
        assert!(
            out.contains(
                "local args_buf = ffi.cast(\"const Buffer*\", ffi.cast(\"uintptr_t\", args_ptr))"
            ) && out.contains("local result = instance:store(args_buf[0])"),
            "Buffer arg must be unpacked as a Buffer cdata: {out}"
        );
    }

    #[test]
    fn lua_guest_handler_struct_arg_unpacks_struct_cdata() {
        let func: ResolvedFunction = ResolvedFunction {
            name: "compute".to_owned(),
            function_id: 0,
            params: vec![ResolvedParam {
                name: "pair".to_owned(),
                ty: ResolvedTypeRef::UserDefined("Pair".to_owned()),
                docs: None,
            }],
            returns: Some(ResolvedTypeRef::Primitive(PrimitiveType::U32)),
            docs: None,
            return_docs: None,
        };
        let out: String = guest_handler(&func, &[]);
        assert!(
            out.contains(
                "local args_struct = ffi.cast(\"const Pair*\", ffi.cast(\"uintptr_t\", args_ptr))"
            ) && out.contains("local result = instance:compute(args_struct[0])"),
            "struct arg must be unpacked as the cdef'd struct cdata: {out}"
        );
    }

    #[test]
    fn lua_guest_handler_enum_arg_passes_number() {
        let enums: Vec<EnumDef> = vec![EnumDef {
            name: "Level".to_owned(),
            repr: ReprType::U32,
            bitflag: false,
            variants: vec![EnumVariant {
                name: "Info".to_owned(),
                value: "1".to_owned(),
                docs: None,
            }],
            docs: None,
        }];
        let func: ResolvedFunction = ResolvedFunction {
            name: "set_level".to_owned(),
            function_id: 0,
            params: vec![ResolvedParam {
                name: "level".to_owned(),
                ty: ResolvedTypeRef::UserDefined("Level".to_owned()),
                docs: None,
            }],
            returns: None,
            docs: None,
            return_docs: None,
        };
        let out: String = guest_handler(&func, &enums);
        assert!(
            out.contains(
                "local args_enum = ffi.cast(\"const uint32_t*\", ffi.cast(\"uintptr_t\", args_ptr))"
            ) && out.contains("local result = instance:set_level(tonumber(args_enum[0]))"),
            "enum arg must be read through its repr slot and handed over as a number: {out}"
        );
    }

    #[test]
    fn lua_guest_handler_multi_param_unpacks_pack_fields() {
        let func: ResolvedFunction = ResolvedFunction {
            name: "combine".to_owned(),
            function_id: 0,
            params: vec![
                scalar_param("a", PrimitiveType::U32),
                scalar_param("b", PrimitiveType::U32),
            ],
            returns: Some(ResolvedTypeRef::Primitive(PrimitiveType::U32)),
            docs: None,
            return_docs: None,
        };
        let out: String = guest_handler(&func, &[]);
        assert!(
            out.contains(
                "local args_pack = ffi.cast(\"const TestAddContractCombineArgs*\", ffi.cast(\"uintptr_t\", args_ptr))"
            ),
            "multi-param args must cast to the cdef'd arg-pack struct: {out}"
        );
        assert!(
            out.contains("local result = instance:combine(args_pack[0].a, args_pack[0].b)"),
            "each pack field must be unpacked and passed positionally: {out}"
        );
    }

    #[test]
    fn lua_guest_handler_buffer_return_marshalled_not_dropped() {
        let func: ResolvedFunction = ResolvedFunction {
            name: "make".to_owned(),
            function_id: 0,
            params: vec![],
            returns: Some(ResolvedTypeRef::AbiType(AbiBuiltin::Buffer)),
            docs: None,
            return_docs: None,
        };
        let out: String = guest_handler(&func, &[]);
        assert!(
            out.contains("local out_ref = ffi.cast(\"Buffer*\", ffi.cast(\"uintptr_t\", out_ptr))")
                && out.contains("out_ref[0] = result"),
            "Buffer return must be written into out_ptr, not silently dropped: {out}"
        );
        assert!(
            out.contains(
                "error(\"polyplug: implementation returned nil for a Buffer-returning function\")"
            ),
            "a nil Buffer return must raise rather than leave a zeroed out-slot: {out}"
        );
    }

    #[test]
    fn lua_guest_handler_struct_return_marshalled_not_dropped() {
        let func: ResolvedFunction = ResolvedFunction {
            name: "build".to_owned(),
            function_id: 0,
            params: vec![],
            returns: Some(ResolvedTypeRef::UserDefined("Pair".to_owned())),
            docs: None,
            return_docs: None,
        };
        let out: String = guest_handler(&func, &[]);
        assert!(
            out.contains("local out_ref = ffi.cast(\"Pair*\", ffi.cast(\"uintptr_t\", out_ptr))")
                && out.contains("out_ref[0] = result"),
            "struct return must be written into out_ptr, not silently dropped: {out}"
        );
    }
}
