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
use crate::ir::ResolvedFunction;
use crate::ir::ResolvedHostContract;
use crate::ir::ResolvedParam;
use crate::ir::ResolvedPlugin;
use crate::ir::ResolvedType;
use crate::ir::ResolvedTypeRef;
use crate::ir::ValidatedIr;
use polyplug_codegen::PolyplugcError;

pub(crate) struct LuaGenerator;

impl CodeGenerator for LuaGenerator {
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

        if ir.bundle.is_some() {
            files.files.push(GeneratedFile {
                path: PathBuf::from("manifest.toml"),
                content: generate_bundle_manifest_lua(ir),
                force_regenerate: true,
            });
        }

        if !ir.host_contracts.is_empty() {
            let host_contracts_lua: String = generate_guest_host_contracts_file(ir);
            files.files.push(GeneratedFile {
                path: PathBuf::from("guest/host_contracts.lua"),
                content: host_contracts_lua,
                force_regenerate: false,
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

    let dep_toml: String = super::emit_manifest_dependencies(&bundle.dependencies);

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
    out.push_str("-- ABI error codes (match polyplug_abi.AbiErrorCode)\n");
    out.push_str("local AbiErrorCode = {\n");
    out.push_str("    Ok = 0,\n");
    out.push_str("    Generic = 1,\n");
    out.push_str("    InvalidPointer = 8,\n");
    out.push_str("}\n\n");

    // Null/not-found sentinel for find_guest_contract. GuestContractHandle is
    // `#[repr(C)] { index: u32 }`; the null handle is index == u32::MAX. This
    // matches polyplug.runtime.NULL_HANDLE; emitted locally so callers.lua has no
    // dependency on the runtime module beyond the `runtime` arg passed to factories.
    out.push_str("local NULL_HANDLE = 0xFFFFFFFF\n\n");

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
    // Native guest dispatch functions take the instance by value and return an
    // AbiError (24-byte struct), matching GuestContractInterface native dispatch.
    out.push_str("-- Cached FFI types for hot path performance\n");
    out.push_str(
        "local NativeDispatchFnType = ffi.typeof(\"AbiError (*)(GuestContractInstance, const void*, void*)\")\n\n",
    );

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

    // The LuaLoader (Rust side) drives registration: after it execs the bundle
    // script and calls polyplug_init, it reads _G._polyplug_handlers and builds
    // the GuestContractInterface itself, wrapping each Lua handler in an
    // extern "C" trampoline. Guest code therefore NEVER constructs a
    // GuestContractInterface cdata or ffi.cast()s a Lua function into a
    // struct-returning C function pointer — LuaJIT cannot create callbacks for
    // function types that return a struct by value (e.g. GuestContractInstance,
    // StringView), so any such cast fails at load. We instead register pure Lua
    // handlers, mirroring tests/fixtures/test_plugin_lua/test_plugin.lua.
    //
    // Each handler has the low-level dispatch signature (args_ptr, out_ptr),
    // where both are i64 integers (see polyplug_lua::loader::lua_dispatch). The
    // generated wrapper marshals args/out around the user's high-level impl.

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
                    generate_guest_plugin_interface(&mut out, &plugin.name, contract)?;
                    registrations.push((plugin.name.as_str(), contract));
                }
            }
        }
    }

    // Define the global polyplug_init the LuaLoader calls. It populates
    // _G._polyplug_handlers with the per-contract dispatch tables. The example
    // guests require this module and call set_<plugin>_impl at module top level,
    // so the impls are already stored by the time polyplug_init runs.
    out.push_str("\n-- Registration entry point called by the LuaLoader.\n");
    out.push_str("function polyplug_init(host_ptr, ctx_ptr)\n");
    out.push_str("    if host_ptr == nil or ctx_ptr == nil then\n");
    out.push_str("        return polyplug_guest.AbiErrorCode.Generic\n");
    out.push_str("    end\n");
    out.push_str("    polyplug_guest.store_host_interface(host_ptr)\n");
    for (plugin_name, _contract) in &registrations {
        let plugin_var: String = plugin_name.to_uppercase().replace(['.', '-'], "_");
        out.push_str(&format!("    M._register_{plugin_var}()\n"));
    }
    out.push_str("    return polyplug_guest.AbiErrorCode.Ok\n");
    out.push_str("end\n\n");

    out.push_str("--- Get a host extension by name. Returns nil if not registered.\n");
    out.push_str("-- @param name string Extension name (as a Lua string).\n");
    out.push_str("-- @return cdata|nil Opaque extension pointer, or nil if not registered.\n");
    out.push_str("function M.polyplug_get_extension(name)\n");
    out.push_str("    local host_ptr = polyplug_guest.get_host_interface()\n");
    out.push_str("    if host_ptr == nil then return nil end\n");
    out.push_str("    local hash = 2166136261\n");
    out.push_str("    for i = 1, #name do\n");
    out.push_str("        hash = bit.bxor(hash, name:byte(i))\n");
    out.push_str("        hash = bit.band(hash * 16777619, 0xFFFFFFFF)\n");
    out.push_str("    end\n");
    out.push_str("    local host = ffi.cast('HostApi*', ffi.cast('uintptr_t', host_ptr))\n");
    out.push_str("    local ptr = host.get_extension(host_ptr, hash)\n");
    out.push_str("    if ptr == nil then return nil end\n");
    out.push_str("    return ptr\n");
    out.push_str("end\n\n");

    out.push_str("return M\n");
    Ok(out)
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

/// Generate the full host caller for a contract with instance-based RAII pattern.
/// Creates methods table, metatable with __gc, and factory function.
fn generate_host_contract_caller(out: &mut String, contract: &ResolvedContract) {
    let contract_prefix: String = contract_name_to_prefix(&contract.name);
    let contract_struct: String = contract_name_to_struct(&contract.name);
    let contract_upper: String = contract.name.to_uppercase().replace(['.', '-'], "_");
    let contract_id_const: String = format!("{}_CONTRACT_ID", contract_upper);

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

    // destroy method - calls destroy_instance and marks the wrapper destroyed.
    out.push_str("    destroy = function(self)\n");
    out.push_str("        if self._interface ~= nil and not self._destroyed then\n");
    out.push_str("            self._interface.destroy_instance(self._host, self._instance)\n");
    out.push_str("            self._destroyed = true\n");
    out.push_str("        end\n");
    out.push_str("    end,\n\n");

    // reset method - destroy current instance, create a fresh one from the
    // still-resolved interface. A null instance.data is valid for stateless
    // contracts and is preserved as the opaque dispatch token.
    out.push_str("    reset = function(self)\n");
    out.push_str("        self:destroy()\n");
    out.push_str("        if self._interface ~= nil then\n");
    out.push_str("            self._instance = self._interface.create_instance(self._host, nil)\n");
    out.push_str("            self._destroyed = false\n");
    out.push_str("        end\n");
    out.push_str("    end,\n\n");

    // Contract function methods - pass instance as first argument
    for func in &contract.functions {
        generate_host_caller_method(out, func, &contract_prefix, &contract_struct);
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
    out.push_str("    -- find_guest_contract returns a u32 handle; the null/not-found sentinel\n");
    out.push_str("    -- is index == u32::MAX (NULL_HANDLE), never nil.\n");
    out.push_str("    if handle == NULL_HANDLE then\n");
    out.push_str("        return nil\n");
    out.push_str("    end\n");
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
    out.push_str("    local instance = interface.create_instance(host, nil)\n");
    out.push_str("    local wrapper = {\n");
    out.push_str("        _interface = interface,\n");
    out.push_str("        _instance = instance,\n");
    out.push_str("        _host = host,\n");
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
) {
    let fn_id: u32 = func.function_id;
    let sig_params: String = build_lua_sig_params(func);
    out.push_str(&format!("    {} = function(self{sig_params})\n", func.name));

    // Validity keys off the resolved interface pointer, NOT instance.data:
    // stateless and VM-dispatch guests carry a null instance handle.
    out.push_str("        if self._interface == nil or self._destroyed then\n");
    out.push_str("            error(\"invalid caller: interface is nil\", 2)\n");
    out.push_str("        end\n");

    // Setup args and out
    emit_lua_host_args_setup(out, func, contract_prefix);
    emit_lua_host_out_setup(out, &func.returns);

    out.push_str(&format!(
        "        if {fn_id} >= self._interface.dispatch.native.function_count then\n"
    ));
    out.push_str("            error(\"function not available in interface\", 2)\n");
    out.push_str("        end\n");

    // Dispatch on the interface's dispatch_type. Native guests (C++/Rust/native
    // Python) call the function pointer directly; VM guests (Lua, JS) route
    // through the loader's vm.call trampoline. Both return an AbiError by value.
    // DispatchType: 0 == Native, 1 == VirtualMachine.
    out.push_str("        local err\n");
    out.push_str("        if self._interface.dispatch_type == 0 then\n");
    out.push_str(&format!(
        "            local fn_ptr = self._interface.dispatch.native.functions[{fn_id}]\n"
    ));
    out.push_str("            local fn = ffi.cast(NativeDispatchFnType, fn_ptr)\n");
    out.push_str("            err = fn(self._instance, args_ptr, out_ptr)\n");
    out.push_str("        else\n");
    // The arena is nil: a Lua host caller cannot soundly hold a per-caller
    // CallArena (the 40-byte arena owns a borrowed primary buffer plus a host
    // overflow chain that must be reset between calls, which has no safe owner in
    // the LuaJIT FFI caller object). A null arena makes the guest bridge fall back
    // to per-value host->alloc — correct, just not zero-allocation. Native Rust/C++
    // hosts (rust.rs fn_needs_arena) carry real per-caller arenas.
    out.push_str(&format!(
        "            err = self._interface.dispatch.vm.call(self._interface.dispatch.vm.loader_data, self._instance, {fn_id}, args_ptr, out_ptr, nil)\n"
    ));
    out.push_str("        end\n");
    out.push_str("        if err.code ~= AbiErrorCode.Ok then\n");
    out.push_str(
        "            error(\"polyplug call failed (code \" .. tonumber(err.code) .. \")\", 2)\n",
    );
    out.push_str("        end\n");

    if has_return_value(&func.returns) {
        out.push_str("        return out_val\n");
    } else {
        out.push_str("        return nil\n");
    }
    out.push_str("    end");
}

fn generate_guest_plugin_interface(
    out: &mut String,
    plugin_name: &str,
    contract: &ResolvedContract,
) -> Result<(), PolyplugcError> {
    let plugin_var: String = plugin_name.to_uppercase().replace(['.', '-'], "_");
    let contract_name_full: String = format!("{}@{}", contract.name, contract.version.major);
    let plugin_lower: String = plugin_name.to_lowercase().replace(['.', '-'], "_");

    out.push_str(&format!(
        "-- Guest contract: {plugin_name} ({contract_name_full})\n"
    ));
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
        out.push_str(&format!(
            "--   {fn_name}({}) -> {ret_ty}\n",
            params.join(", "),
            fn_name = func.name.replace('.', "_")
        ));
    }

    // Per-plugin storage for the user-supplied high-level implementations.
    out.push_str(&format!("local {plugin_var}_IMPLS = {{}}\n"));

    // set_<plugin>_impl(fn0, fn1, ...) stores the high-level handlers in
    // declaration order (matching contract function_id order).
    let set_impl_name: String = format!("set_{plugin_lower}_impl");
    let impl_params: Vec<String> = contract
        .functions
        .iter()
        .map(|f: &ResolvedFunction| format!("{}_fn", f.name.replace('.', "_")))
        .collect();
    out.push_str(&format!(
        "function M.{set_impl_name}({})\n",
        impl_params.join(", ")
    ));
    for (idx, func) in contract.functions.iter().enumerate() {
        out.push_str(&format!(
            "    {plugin_var}_IMPLS[{idx}] = {fn}_fn\n",
            fn = func.name.replace('.', "_")
        ));
    }
    out.push_str("end\n");

    // _register_<plugin>() builds the low-level dispatch handlers and stores them
    // under a per-contract entry in _G._polyplug_handlers, keyed by contract name.
    // The loader iterates every entry and registers one GuestContractInterface per
    // contract, so multi-contract bundles register ALL their contracts. Each handler
    // has the signature (args_ptr, out_ptr) with i64 pointer integers, marshals the
    // inputs, invokes the stored high-level impl, and writes the result to out_ptr.
    out.push_str(&format!("function M._register_{plugin_var}()\n"));
    out.push_str("    local functions = {}\n");
    for (idx, func) in contract.functions.iter().enumerate() {
        out.push_str(&format!(
            "    functions[{idx}] = function(args_ptr, out_ptr)\n"
        ));
        emit_lua_guest_handler_body(out, func, &plugin_var, idx);
        out.push_str("    end\n");
    }
    out.push_str("    _G._polyplug_handlers = _G._polyplug_handlers or {}\n");
    out.push_str(&format!(
        "    _G._polyplug_handlers[\"{}\"] = {{\n",
        contract.name
    ));
    out.push_str(&format!(
        "        contract_version = {},\n",
        contract.version.major
    ));
    out.push_str(&format!("        plugin_name = \"{plugin_name}\",\n"));
    out.push_str("        functions = functions,\n");
    out.push_str("    }\n");
    out.push_str("end\n\n");

    Ok(())
}

/// Emit the body of one low-level dispatch handler: marshal args from
/// `args_ptr`, call the stored high-level impl, marshal the result to
/// `out_ptr`. Pointers arrive as i64 integers (see lua_dispatch).
fn emit_lua_guest_handler_body(
    out: &mut String,
    func: &ResolvedFunction,
    plugin_var: &str,
    idx: usize,
) {
    out.push_str(&format!("        local impl = {plugin_var}_IMPLS[{idx}]\n"));
    out.push_str("        if impl == nil then return end\n");

    // Marshal a single StringView input (the common pipeline shape). Other
    // input shapes pass the raw args pointer through for the impl to read.
    let single_string_view: bool = func.params.len() == 1
        && matches!(
            func.params[0].ty,
            ResolvedTypeRef::AbiType(AbiBuiltin::StringView)
        );

    if single_string_view {
        out.push_str(
            "        local args_sv = ffi.cast(\"const StringView*\", ffi.cast(\"uintptr_t\", args_ptr))\n",
        );
        out.push_str("        local result = impl(args_sv[0])\n");
    } else if func.params.is_empty() {
        out.push_str("        local result = impl()\n");
    } else {
        // Fall back to handing the raw pointer integers to the impl.
        out.push_str("        local result = impl(args_ptr, out_ptr)\n");
    }

    // Marshal a StringView return value into out_ptr. alloc_string returns a
    // host-allocated StringView cdata; copy it into the caller's out slot.
    if matches!(
        func.returns,
        Some(ResolvedTypeRef::AbiType(AbiBuiltin::StringView))
    ) {
        out.push_str("        if out_ptr ~= 0 and result ~= nil then\n");
        out.push_str(
            "            local out_sv = ffi.cast(\"StringView*\", ffi.cast(\"uintptr_t\", out_ptr))\n",
        );
        out.push_str("            out_sv[0] = result\n");
        out.push_str("        end\n");
    }
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
/// For host interfaces, we use ergonomic Lua types:
/// - StringView -> string
/// - Buffer -> string (Lua uses strings for byte buffers)
/// - UserDefined -> userdata
/// - Primitives -> number (Lua's numeric type)
fn lua_host_param_type_annotation(ty: &ResolvedTypeRef) -> String {
    match ty {
        ResolvedTypeRef::Primitive(_) => "number".to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => "string".to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => "string".to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::Ptr) => "userdata".to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::Void) => "nil".to_owned(),
        ResolvedTypeRef::UserDefined(_) => "userdata".to_owned(),
    }
}

/// Generate Lua return type annotation for host contract methods.
fn lua_host_return_type_annotation(ty: &ResolvedTypeRef) -> String {
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
            Some(ty) => lua_host_return_type_annotation(ty),
            None => "nil".to_owned(),
        };

        out.push_str("--- @param self table\n");
        for param in &func.params {
            out.push_str(&format!(
                "--- @param {} {}\n",
                param.name,
                lua_host_param_type_annotation(&param.ty)
            ));
        }
        out.push_str(&format!("--- @return {}\n", return_type));
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
fn generate_lua_guest_host_contract_caller(out: &mut String, contract: &ResolvedHostContract) {
    let class_name: String = host_contract_name_to_lua_caller(&contract.name);

    out.push_str(&format!(
        "-- Guest caller for host contract `{}` (id=0x{:016X})\n",
        contract.name, contract.contract_id
    ));
    out.push_str(&format!("{} = {{}}\n", class_name));
    out.push_str(&format!("{}.__index = {}\n\n", class_name, class_name));

    out.push_str(&format!("function {}:new(interface)\n", class_name));
    out.push_str("    local obj = { _interface = interface }\n");
    out.push_str("    setmetatable(obj, self)\n");
    out.push_str("    return obj\n");
    out.push_str("end\n\n");

    out.push_str(&format!(
        "function {}.from_host(host_ptr, min_version)\n",
        class_name
    ));
    out.push_str("    if min_version == nil then min_version = 0 end\n");
    out.push_str("    if host_ptr == nil then\n");
    out.push_str("        return nil\n");
    out.push_str("    end\n");
    out.push_str("    local host = ffi.cast(\"HostApi*\", host_ptr)\n");
    out.push_str(&format!(
        "    local interface_ptr = host.get_host_contract(host_ptr, 0x{:016X}ULL, min_version)\n",
        contract.contract_id
    ));
    out.push_str("    if interface_ptr == nil then\n");
    out.push_str("        return nil\n");
    out.push_str("    end\n");
    out.push_str(&format!("    return {}:new(interface_ptr)\n", class_name));
    out.push_str("end\n\n");

    out.push_str(&format!("function {}:is_valid()\n", class_name));
    out.push_str("    return self._interface ~= nil\n");
    out.push_str("end\n\n");

    for func in &contract.functions {
        generate_lua_guest_host_contract_method(out, func, &class_name);
    }

    out.push('\n');
}

/// Generate one method for a guest-side host contract caller.
fn generate_lua_guest_host_contract_method(
    out: &mut String,
    func: &ResolvedFunction,
    class_name: &str,
) {
    let fn_id: u32 = func.function_id;
    let has_return: bool = func.returns.is_some();

    let params_str: String = if func.params.is_empty() {
        "self".to_owned()
    } else {
        let params: Vec<String> = func
            .params
            .iter()
            .map(|p: &ResolvedParam| p.name.clone())
            .collect();
        format!("self, {}", params.join(", "))
    };

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

    out.push_str("    local header = ffi.cast(\"HostContractVTable*\", self._interface).header\n");
    out.push_str(&format!("    if {fn_id} >= header.function_count then\n"));
    if has_return {
        out.push_str("        return nil\n");
    } else {
        out.push_str("        return\n");
    }
    out.push_str("    end\n");

    out.push_str("    local dispatch_type = header.dispatch_type\n");

    emit_lua_guest_host_contract_args_setup(out, func);
    emit_lua_guest_host_contract_out_setup(out, &func.returns);

    out.push_str("    local err\n");
    out.push_str("    if dispatch_type == 0 then\n");
    out.push_str(&format!(
        "        local fn_ptr = header.dispatch.native.functions[{fn_id}]\n"
    ));
    out.push_str("        local impl_ptr = header.dispatch.native.impl_ptr\n");
    out.push_str("        local fn = ffi.cast(DispatchFnType, fn_ptr)\n");
    out.push_str("        err = fn(impl_ptr, args_ptr, out_ptr)\n");
    out.push_str("    elseif dispatch_type == 1 then\n");
    // The VM dispatch ABI signature is fn(loader_data, instance, fn_id, args, out,
    // arena). Host contracts carry no guest instance, so a null GuestContractInstance
    // is passed in the instance slot — matching the canonical rust host-contract
    // caller (which passes GuestContractInstance::null()). The arena is null: this
    // caller has no per-caller arena, so the bridge falls back to host->alloc.
    out.push_str("        local _null_instance = ffi.new(\"GuestContractInstance\")\n");
    out.push_str(&format!(
        "        err = header.dispatch.vm.call(header.dispatch.vm.bridge_data, _null_instance, {fn_id}, args_ptr, out_ptr, nil)\n"
    ));
    out.push_str("    else\n");
    if has_return {
        out.push_str("        return nil\n");
    } else {
        out.push_str("        return\n");
    }
    out.push_str("    end\n");

    out.push_str("    if err ~= 0 then\n");
    if has_return {
        out.push_str("        return nil\n");
    } else {
        out.push_str("        return\n");
    }
    out.push_str("    end\n");

    if has_return {
        out.push_str("    return out_val\n");
    }
    out.push_str("end\n\n");
}

/// Emit the args_ptr setup for a Lua guest host contract method.
fn emit_lua_guest_host_contract_args_setup(out: &mut String, func: &ResolvedFunction) {
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
                out.push_str(&format!(
                    "    local args_ptr = ffi.cast(\"const void*\", {})\n",
                    param.name
                ));
            }
            ResolvedTypeRef::Primitive(_) | ResolvedTypeRef::AbiType(_) => {
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

    // Multiple params: pack into inline struct
    out.push_str("    local args_val = {}\n");
    for param in &func.params {
        match &param.ty {
            ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => {
                out.push_str(&format!(
                    "    local {name}_bytes = tostring({name})\n",
                    name = param.name
                ));
                out.push_str(&format!(
                    "    args_val.{name} = ffi.new(\"StringView\")\n",
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
                    "    args_val.{name} = ffi.new(\"Buffer\")\n",
                    name = param.name
                ));
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
fn emit_lua_guest_host_contract_out_setup(out: &mut String, returns: &Option<ResolvedTypeRef>) {
    if let Some(ret_ty) = returns {
        if matches!(ret_ty, ResolvedTypeRef::AbiType(AbiBuiltin::StringView)) {
            out.push_str("    local out_val = ffi.new(\"StringView\")\n");
            out.push_str("    local out_ptr = ffi.cast(\"void*\", out_val)\n");
        } else if matches!(ret_ty, ResolvedTypeRef::AbiType(AbiBuiltin::Buffer)) {
            out.push_str("    local out_val = ffi.new(\"Buffer\")\n");
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

/// Generate `guest/host_contracts.lua` — caller classes for guest-side host contract callers.
fn generate_guest_host_contracts_file(ir: &ValidatedIr) -> String {
    let mut out: String = String::new();
    out.push_str(file_header());
    out.push_str("local ffi = require(\"ffi\")\n\n");

    out.push_str("local M = {}\n\n");

    out.push_str("-- Cached FFI types for hot path performance\n");
    out.push_str(
        "local DispatchFnType = ffi.typeof(\"uint32_t (*)(const void*, const void*, void*)\")\n\n",
    );

    for contract in &ir.host_contracts {
        generate_lua_guest_host_contract_caller(&mut out, contract);
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
    out
}

// ─── Host Interface Factories Generation ─────────────────────────────────────────

/// Generate all host-side interface factories into a single file.
fn generate_lua_host_interface_factories_file(ir: &ValidatedIr) -> String {
    let mut out: String = String::new();
    out.push_str(file_header());
    out.push_str("local ffi = require(\"ffi\")\n\n");

    out.push_str("-- ABI error codes (match polyplug_abi.AbiErrorCode)\n");
    out.push_str("local AbiErrorCode = {\n");
    out.push_str("    Ok = 0,\n");
    out.push_str("    Panic = 5,\n");
    out.push_str("}\n\n");

    out.push_str("local M = {}\n\n");

    for contract in &ir.host_contracts {
        generate_lua_host_interface_factory(&mut out, contract);
    }

    out.push_str("return M\n");
    out
}

/// Generate interface factories for one host contract.
fn generate_lua_host_interface_factory(out: &mut String, contract: &ResolvedHostContract) {
    let class_name: String = host_contract_name_to_lua_class(&contract.name);
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
        "-- Create a host contract interface for `{}` with NATIVE dispatch.\n",
        contract.name
    ));
    out.push_str("--\n");
    out.push_str("-- Takes an implementation table and creates an interface.\n");
    out.push_str("-- The implementation must have methods matching the contract.\n");
    out.push_str("--\n");
    out.push_str("-- Memory:\n");
    out.push_str(
        "-- The returned interface is cached and lives for the lifetime of the program.\n",
    );
    out.push_str(&format!("function M.{factory_name}(impl)\n"));
    out.push_str(&format!("    _{class_name}_impl = impl\n\n"));

    // Generate thunks for each function
    for func in &contract.functions {
        generate_lua_host_thunk(out, func, &contract.name, &class_name);
    }

    // Static function pointer array
    out.push_str(&format!(
        "    local functions = ffi.new(\"void*[{fn_count}]\")\n"
    ));
    for (idx, func) in contract.functions.iter().enumerate() {
        let thunk_name: String = format!(
            "_{}_{}_thunk",
            contract.name.replace('.', "_").to_lowercase(),
            func.name
        );
        out.push_str(&format!(
            "    functions[{idx}] = ffi.cast(\"void*\", ffi.cast(\"uint32_t (*)(const void*, const void*, void*)\", {thunk_name}))\n"
        ));
    }
    out.push('\n');

    // Create the interface
    out.push_str("    local interface = ffi.new(\"HostContractVTable\")\n");
    out.push_str("    interface.header.vtable_version = 1\n");
    out.push_str(&format!(
        "    interface.header.contract_id = 0x{contract_id:016X}ULL\n"
    ));
    out.push_str(&format!("    interface.header.contract_major = {major}\n"));
    out.push_str(&format!("    interface.header.contract_minor = {minor}\n"));
    out.push_str(&format!(
        "    interface.header.function_count = {fn_count}\n"
    ));
    out.push_str(&format!(
        "    interface.header.singleton = {singleton}  -- {}\n",
        if singleton {
            "singleton"
        } else {
            "multi-instance"
        }
    ));
    out.push_str("    interface.header.dispatch_type = 0  -- DispatchType.Native\n");
    out.push_str("    interface.dispatch.native.impl_ptr = nil  -- We use global _impl instead\n");
    out.push_str("    interface.dispatch.native.functions = functions\n\n");
    out.push_str("    return interface\n");
    out.push_str("end\n\n");

    // Global implementation storage
    out.push_str(&format!("_{class_name}_impl = nil\n\n"));

    // VM dispatch factory
    out.push_str(&format!(
        "-- Create a host contract interface for `{}` with VM dispatch.\n",
        contract.name
    ));
    out.push_str("--\n");
    out.push_str("-- Used when the host implementation is in a VM language (Python, Lua, JS).\n");
    out.push_str("--\n");
    out.push_str("-- Arguments:\n");
    out.push_str("--     bridge_data: Opaque pointer to VM-specific data\n");
    out.push_str("--     dispatch_fn: Function to call for each contract function\n");
    out.push_str("--\n");
    out.push_str("-- Memory:\n");
    out.push_str(
        "-- The returned interface is cached and lives for the lifetime of the program.\n",
    );
    out.push_str(&format!(
        "function M.{factory_vm_name}(bridge_data, dispatch_fn)\n"
    ));
    out.push_str("    local interface = ffi.new(\"HostContractVTable\")\n");
    out.push_str("    interface.header.vtable_version = 1\n");
    out.push_str(&format!(
        "    interface.header.contract_id = 0x{contract_id:016X}ULL\n"
    ));
    out.push_str(&format!("    interface.header.contract_major = {major}\n"));
    out.push_str(&format!("    interface.header.contract_minor = {minor}\n"));
    out.push_str(&format!(
        "    interface.header.function_count = {fn_count}\n"
    ));
    out.push_str(&format!(
        "    interface.header.singleton = {singleton}  -- {}\n",
        if singleton {
            "singleton"
        } else {
            "multi-instance"
        }
    ));
    out.push_str("    interface.header.dispatch_type = 1  -- DispatchType.VirtualMachine\n");
    out.push_str("    interface.dispatch.vm.call = dispatch_fn\n");
    out.push_str("    interface.dispatch.vm.bridge_data = bridge_data\n\n");
    out.push_str("    return interface\n");
    out.push_str("end\n\n");
}

/// Generate a thunk function for a host contract function.
fn generate_lua_host_thunk(
    out: &mut String,
    func: &ResolvedFunction,
    contract_name: &str,
    class_name: &str,
) {
    let thunk_name: String = format!(
        "_{}_{}_thunk",
        contract_name.replace('.', "_").to_lowercase(),
        func.name
    );
    let has_return: bool = func.returns.is_some();

    out.push_str(&format!(
        "    local function {thunk_name}(impl_ptr, args, out)\n"
    ));
    out.push_str("        local ok, err = pcall(function()\n");
    out.push_str(&format!("            local impl = _{class_name}_impl\n"));
    out.push_str("            if impl == nil then\n");
    out.push_str("                return AbiErrorCode.Panic\n");
    out.push_str("            end\n");

    // Generate argument extraction
    if !func.params.is_empty() {
        generate_lua_host_thunk_args(out, func);
    } else {
        out.push_str("            local _ = args\n");
    }

    // Generate the method call
    generate_lua_host_thunk_call(out, func, has_return);

    // Handle return value
    if has_return {
        out.push_str("            -- SAFETY: out is a valid pointer per ABI contract.\n");
        out.push_str("            ffi.copy(out, result, ffi.sizeof(result))\n");
    } else {
        out.push_str("            local _ = out\n");
    }

    out.push_str("            return AbiErrorCode.Ok\n");
    out.push_str("        end)\n");
    out.push_str("        if not ok then\n");
    out.push_str("            return AbiErrorCode.Panic\n");
    out.push_str("        end\n");
    out.push_str("        return err\n");
    out.push_str("    end\n\n");
}

/// Generate argument extraction for a host thunk.
fn generate_lua_host_thunk_args(out: &mut String, func: &ResolvedFunction) {
    if func.params.len() == 1 {
        let param: &ResolvedParam = &func.params[0];
        let ty_name: String = lua_host_abi_type_name(&param.ty);
        match &param.ty {
            ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => {
                out.push_str(&format!(
                    "            local {name}_sv = ffi.cast(\"StringView*\", args)[0]\n",
                    name = param.name
                ));
                out.push_str(&format!(
                    "            local {name} = ffi.string({name}_sv.ptr, {name}_sv.len)\n",
                    name = param.name
                ));
            }
            ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => {
                out.push_str(&format!(
                    "            local {name}_buf = ffi.cast(\"Buffer*\", args)[0]\n",
                    name = param.name
                ));
                out.push_str(&format!(
                    "            local {name} = ffi.string({name}_buf.ptr, {name}_buf.len)\n",
                    name = param.name
                ));
            }
            ResolvedTypeRef::UserDefined(_) => {
                out.push_str(&format!(
                    "            local {name} = ffi.cast(\"{ty}*\", args)[0]\n",
                    name = param.name,
                    ty = ty_name
                ));
            }
            _ => {
                out.push_str(&format!(
                    "            local {name} = ffi.cast(\"{ty}*\", args)[0]\n",
                    name = param.name,
                    ty = ty_name
                ));
            }
        }
    } else {
        // Multiple params - use arg-pack struct
        let pack_struct: String = format!("{}Args", func.name.to_uppercase());
        out.push_str(&format!(
            "            local packed = ffi.cast(\"{pack_struct}*\", args)[0]\n"
        ));
        // Extract each param from the packed struct
        for param in &func.params {
            match &param.ty {
                ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => {
                    out.push_str(&format!(
                        "            local {name} = ffi.string(packed.{name}.ptr, packed.{name}.len)\n",
                        name = param.name
                    ));
                }
                ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => {
                    out.push_str(&format!(
                        "            local {name} = ffi.string(packed.{name}.ptr, packed.{name}.len)\n",
                        name = param.name
                    ));
                }
                _ => {
                    let ty_name: String = lua_host_param_type_annotation(&param.ty);
                    out.push_str(&format!(
                        "            local {name}: {ty} = packed.{name}\n",
                        name = param.name,
                        ty = ty_name
                    ));
                }
            }
        }
    }
}

/// Generate the method call inside a host thunk.
fn generate_lua_host_thunk_call(out: &mut String, func: &ResolvedFunction, has_return: bool) {
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

    if has_return {
        let _ret_ty: String = match func.returns.as_ref() {
            Some(ret) => lua_host_abi_type_name(ret),
            None => String::from("void"),
        };
        out.push_str(&format!(
            "            local result = impl:{func_name}({call_args})\n",
            func_name = func.name
        ));
    } else {
        out.push_str(&format!(
            "            impl:{func_name}({call_args})\n",
            func_name = func.name
        ));
    }
}

/// Generate ABI type name for host thunk arguments.
fn lua_host_abi_type_name(ty: &ResolvedTypeRef) -> String {
    match ty {
        ResolvedTypeRef::Primitive(p) => p.cpp_name().to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::StringView) => "StringView".to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::Buffer) => "Buffer".to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::Ptr) => "void*".to_owned(),
        ResolvedTypeRef::AbiType(AbiBuiltin::Void) => "void".to_owned(),
        ResolvedTypeRef::UserDefined(name) => name.clone(),
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
        assert_eq!(lua_host_param_type_annotation(&ty), "string");
    }

    #[test]
    fn lua_host_param_type_buffer() {
        let ty: ResolvedTypeRef = ResolvedTypeRef::AbiType(AbiBuiltin::Buffer);
        assert_eq!(lua_host_param_type_annotation(&ty), "string");
    }

    #[test]
    fn lua_host_param_type_primitives() {
        assert_eq!(
            lua_host_param_type_annotation(&ResolvedTypeRef::Primitive(PrimitiveType::U32)),
            "number"
        );
        assert_eq!(
            lua_host_param_type_annotation(&ResolvedTypeRef::Primitive(PrimitiveType::I64)),
            "number"
        );
        assert_eq!(
            lua_host_param_type_annotation(&ResolvedTypeRef::Primitive(PrimitiveType::F64)),
            "number"
        );
        assert_eq!(
            lua_host_param_type_annotation(&ResolvedTypeRef::Primitive(PrimitiveType::Bool)),
            "number"
        );
    }

    #[test]
    fn lua_host_return_type_stringview() {
        let ty: ResolvedTypeRef = ResolvedTypeRef::AbiType(AbiBuiltin::StringView);
        assert_eq!(lua_host_return_type_annotation(&ty), "string");
    }

    #[test]
    fn lua_host_return_type_buffer() {
        let ty: ResolvedTypeRef = ResolvedTypeRef::AbiType(AbiBuiltin::Buffer);
        assert_eq!(lua_host_return_type_annotation(&ty), "string");
    }

    #[test]
    fn generate_lua_host_contract_metatable_basic() {
        let contract: ResolvedHostContract = ResolvedHostContract {
            name: "host.logger".to_owned(),
            contract_id: 0x123456789ABCDEF0,
            version: crate::ir::Version {
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
                    },
                    ResolvedParam {
                        name: "message".to_owned(),
                        ty: ResolvedTypeRef::AbiType(AbiBuiltin::StringView),
                    },
                ],
                returns: None,
            }],
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
            version: crate::ir::Version {
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
            version: crate::ir::Version {
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
            version: crate::ir::Version {
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
                    },
                    ResolvedParam {
                        name: "message".to_owned(),
                        ty: ResolvedTypeRef::AbiType(AbiBuiltin::StringView),
                    },
                ],
                returns: None,
            }],
        };
        let mut out: String = String::new();
        generate_lua_guest_host_contract_caller(&mut out, &contract);
        assert!(
            out.contains("HostLoggerContract = {}"),
            "missing class table: {out}"
        );
        assert!(
            out.contains("HostLoggerContract.__index = HostLoggerContract"),
            "missing __index: {out}"
        );
        assert!(
            out.contains("function HostLoggerContract:new(interface)"),
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
            out.contains("function HostLoggerContract:log(self, level, message)"),
            "missing log method: {out}"
        );
    }

    #[test]
    fn generate_lua_guest_host_contract_caller_with_return() {
        let contract: ResolvedHostContract = ResolvedHostContract {
            name: "host.fs.reader".to_owned(),
            contract_id: 0xDEADBEEF,
            version: crate::ir::Version {
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
        generate_lua_guest_host_contract_caller(&mut out, &contract);
        assert!(
            out.contains("HostFsReaderContract = {}"),
            "missing class table: {out}"
        );
        assert!(
            out.contains("function HostFsReaderContract:read(self, path)"),
            "missing read method: {out}"
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
        let result: String = generate_guest_host_contracts_file(&ir);
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
            version: crate::ir::Version {
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
        };
        let ir: ValidatedIr = ValidatedIr {
            types: vec![],
            enums: vec![],
            contracts: vec![],
            host_contracts: vec![contract],
            bundle: None,
        };
        let result: String = generate_guest_host_contracts_file(&ir);
        assert!(result.contains("HostLoggerContract = {}"));
        assert!(result.contains("M.HOSTLOGGERCONTRACT_ID = 0x123456789ABCDEF0ULL"));
        assert!(result.contains("M.HostLoggerContract = HostLoggerContract"));
    }
}
