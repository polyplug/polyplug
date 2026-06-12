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

        // ── guest/peer_callers.lua ─────────────────────────────────────────────
        let peer_contracts: Vec<&ResolvedContract> = collect_lua_peer_contracts(ir);
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
    // Native guest dispatch functions take the instance by value and return an
    // AbiError (24-byte struct), matching GuestContractInterface native dispatch.
    out.push_str("-- Cached FFI types for hot path performance\n");
    out.push_str(
        "local NativeDispatchFnType = ffi.typeof(\"AbiError (*)(GuestContractInstance, const void*, void*)\")\n\n",
    );

    for contract in &ir.contracts {
        generate_host_contract_caller(&mut out, contract, &ir.enums);
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
    enums: &[EnumDef],
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
    emit_lua_host_args_setup(out, func, contract_prefix, enums);
    emit_lua_host_out_setup(out, &func.returns, enums);

    // Dispatch on the interface's dispatch_type. Native guests (C++/Rust/native
    // Python) call the function pointer directly; VM guests (Lua, JS) route
    // through the loader's vm.call trampoline. Both return an AbiError by value.
    // DispatchType: 0 == Native, 1 == VirtualMachine.
    out.push_str("        local err\n");
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
        out.push_str(&format!(
            "        return {}\n",
            lua_return_expr(&func.returns, enums)
        ));
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
    // A missing impl must NOT fall through to success (the loader treats a
    // normal return as Ok, leaving a zeroed out-slot). Raising makes the
    // loader return AbiErrorCode.Generic to the caller.
    out.push_str(&format!(
        "        if impl == nil then error(\"polyplug: no implementation registered for function {idx}\") end\n"
    ));

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
        // A nil result for a StringView-returning function must NOT fall
        // through to success with a zeroed out-slot; raising makes the loader
        // return AbiErrorCode.Generic to the caller.
        out.push_str("        if out_ptr ~= 0 and result == nil then\n");
        out.push_str(
            "            error(\"polyplug: implementation returned nil for a StringView-returning function\")\n",
        );
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
        out.push_str(&format!("    local out_val = ffi.new(\"{repr}[1]\")\n"));
        out.push_str("    local out_ptr = ffi.cast(\"void*\", out_val)\n");
        return;
    }
    let ret_ty: String = match returns {
        Some(ret) => lua_type_name(ret),
        None => "void".to_owned(),
    };
    let is_scalar: bool = matches!(returns, Some(ret) if lua_return_is_scalar(ret));
    if is_scalar {
        out.push_str(&format!("    local out_val = ffi.new(\"{ret_ty}[1]\")\n"));
    } else {
        out.push_str(&format!("    local out_val = ffi.new(\"{ret_ty}\")\n"));
    }
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
fn generate_lua_guest_host_contract_caller(
    out: &mut String,
    contract: &ResolvedHostContract,
    enums: &[EnumDef],
) {
    let class_name: String = host_contract_name_to_lua_caller(&contract.name);

    out.push_str(&format!(
        "-- Guest caller for host contract `{}` (id=0x{:016X})\n",
        contract.name, contract.contract_id
    ));
    out.push_str(&format!("{} = {{}}\n", class_name));
    out.push_str(&format!("{}.__index = {}\n\n", class_name, class_name));

    out.push_str(&format!(
        "function {}:new(interface, instance)\n",
        class_name
    ));
    out.push_str("    local obj = { _interface = interface, _instance = instance }\n");
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
    // `host_ptr` arrives from polyplug_guest.get_host_interface() as a plain Lua
    // number (the host pointer the loader stored at init). Cast through uintptr_t
    // first, exactly like the host-side caller path: a direct ffi.cast("HostApi*",
    // number) yields a pointer LuaJIT then rejects as the first FFI argument
    // ("bad argument #1"). Pass the typed `host` cdata to every HostApi call.
    out.push_str("    local host = ffi.cast(\"HostApi*\", ffi.cast(\"uintptr_t\", host_ptr))\n");
    // Resolve the contract vtable. This is the source of dispatch metadata
    // (dispatch_type, function_count, functions) — NOT the instance. Mirrors the
    // canonical Rust host-contract caller (resolve_host_contract_interface +
    // get_host_contract).
    out.push_str(&format!(
        "    local interface_ptr = host.resolve_host_contract_interface(host, 0x{:016X}ULL, min_version)\n",
        contract.contract_id
    ));
    out.push_str("    if interface_ptr == nil then\n");
    out.push_str("        return nil\n");
    out.push_str("    end\n");
    // The per-instance state: native dispatch thunks receive this as their `this`
    // (first) argument.
    out.push_str(&format!(
        "    local instance = host.get_host_contract(host, 0x{:016X}ULL, min_version)\n",
        contract.contract_id
    ));
    out.push_str(&format!(
        "    return {}:new(interface_ptr, instance)\n",
        class_name
    ));
    out.push_str("end\n\n");

    out.push_str(&format!("function {}:is_valid()\n", class_name));
    out.push_str("    return self._interface ~= nil\n");
    out.push_str("end\n\n");

    for func in &contract.functions {
        generate_lua_guest_host_contract_method(out, func, &class_name, enums);
    }

    out.push('\n');
}

/// Generate one method for a guest-side host contract caller.
fn generate_lua_guest_host_contract_method(
    out: &mut String,
    func: &ResolvedFunction,
    class_name: &str,
    enums: &[EnumDef],
) {
    let fn_id: u32 = func.function_id;
    let has_return: bool = func.returns.is_some();

    // Colon-method syntax (`Class:method`) already binds an implicit `self`, so the
    // parameter list must NOT re-declare it. Emitting `:method(self, ...)` shifts
    // every real argument by one (the caller's first arg lands in the redundant
    // `self` slot and the last real parameter becomes nil) — the bug that silently
    // dropped the message a guest passed to host.logger:log().
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

    // The resolved interface is a flat `HostContractInterface` (80 bytes): there is
    // no `HostContractVTable`/`header` wrapper in the ABI. Read dispatch metadata
    // directly from the struct, mirroring the canonical Rust host-contract caller.
    out.push_str("    local interface = ffi.cast(\"HostContractInterface*\", self._interface)\n");
    out.push_str("    local dispatch_type = interface.dispatch_type\n");

    emit_lua_guest_host_contract_args_setup(out, func, class_name, enums);
    emit_lua_guest_host_contract_out_setup(out, &func.returns, enums);

    out.push_str("    local err\n");
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
    out.push_str("        err = fn(impl_ptr, args_ptr, out_ptr)\n");
    out.push_str("    elseif dispatch_type == 1 then\n");
    // The VM dispatch ABI signature is fn(loader_data, instance, fn_id, args, out,
    // arena). Host contracts carry no guest instance, so a null GuestContractInstance
    // is passed in the instance slot — matching the canonical rust host-contract
    // caller (which passes GuestContractInstance::null()). The arena is null: this
    // caller has no per-caller arena, so the bridge falls back to host->alloc.
    out.push_str("        local _null_instance = ffi.new(\"GuestContractInstance\")\n");
    out.push_str(&format!(
        "        err = interface.dispatch.vm.call(interface.dispatch.vm.loader_data, _null_instance, {fn_id}, args_ptr, out_ptr, nil)\n"
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
    out.push_str("end\n\n");
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

/// Collect every contract in `ir.contracts` whose `contract_id` appears in the
/// bundle's declared dependencies.  Returns an empty vec when there is no bundle
/// or when no dependency matches any known contract.
fn collect_lua_peer_contracts(ir: &ValidatedIr) -> Vec<&ResolvedContract> {
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

/// Return the `min_version` (major) for the dependency whose `contract_id` matches `target`.
/// Returns 0 when no matching dependency is found (callers guard the empty-peer-set case first).
fn peer_min_version_lua(ir: &ValidatedIr, target_contract_id: u64) -> u32 {
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
    out.push_str("local ffi = require(\"ffi\")\n");
    // polyplug_abi declares GuestContractInterface, GuestContractInstance,
    // GuestContractHandle, AbiError, StringView, Buffer, HostApi — all needed below.
    out.push_str("local polyplug_abi = require(\"polyplug_abi\")\n");
    out.push_str("local polyplug_guest = require(\"polyplug_guest\")\n\n");

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

    out.push_str("local M = {}\n\n");

    for contract in peers {
        let min_ver: u32 = peer_min_version_lua(ir, contract.contract_id);
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

    // :new(interface, instance, host) — low-level constructor used by resolve().
    out.push_str(&format!(
        "function {}:new(interface, instance, host)\n",
        class_name
    ));
    out.push_str(
        "    local obj = { _interface = interface, _instance = instance, _host = host }\n",
    );
    out.push_str("    setmetatable(obj, self)\n");
    out.push_str("    return obj\n");
    out.push_str("end\n\n");

    // .resolve() — factory: get host → find → resolve → create_instance.
    // `host_ptr` comes from `polyplug_guest.get_host_interface()`, which returns
    // a plain Lua number. Cast through uintptr_t first (matching the host-contract
    // caller's from_host path) — a direct ffi.cast("HostApi*", number) is rejected
    // by LuaJIT as the first FFI argument.
    out.push_str(&format!("function {}.resolve()\n", class_name));
    out.push_str("    local host_ptr = polyplug_guest.get_host_interface()\n");
    out.push_str("    if host_ptr == nil then\n");
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
    out.push_str("    local instance = interface.create_instance(host, nil)\n");
    out.push_str(
        "    -- Stamp the peer contract id so call_guest_method routes by it even when a\n",
    );
    out.push_str("    -- stateless peer's create_instance returns a null (null-id) handle.\n");
    out.push_str(&format!(
        "    instance.contract_id = 0x{:016X}ULL\n",
        contract.contract_id
    ));
    out.push_str(&format!(
        "    return {}:new(interface, instance, host)\n",
        class_name
    ));
    out.push_str("end\n\n");

    out.push_str(&format!("function {}:is_valid()\n", class_name));
    out.push_str("    return self._interface ~= nil\n");
    out.push_str("end\n\n");

    for func in &contract.functions {
        generate_lua_guest_peer_method(out, func, &class_name, enums);
    }

    out.push('\n');
}

/// Generate one method on a guest peer caller class.
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

    // Cast the stored interface pointer to GuestContractInterface so we can read
    // dispatch_type and the native/VM union — mirrors the host-caller path.
    out.push_str("    local interface = ffi.cast(\"GuestContractInterface*\", self._interface)\n");
    out.push_str("    local dispatch_type = interface.dispatch_type\n");

    // Args and out setup — reuse the same helpers as the host-contract caller so
    // marshalling is identical (no extra tostring() layer = avoids the a3-parity
    // double-conversion Lua footgun).
    emit_lua_guest_host_contract_args_setup(out, func, class_name, enums);
    emit_lua_guest_host_contract_out_setup(out, &func.returns, enums);

    out.push_str("    local err\n");
    out.push_str("    if dispatch_type == 0 then\n");
    // Function-id bounds check inside the Native arm only: on a VM interface
    // dispatch.native.function_count aliases bits of dispatch.vm.call through
    // the union (garbage). The host-mediated call_guest_method path enforces
    // its own bounds (FunctionNotAvailable).
    out.push_str(&format!(
        "        if {fn_id} >= interface.dispatch.native.function_count then\n"
    ));
    if has_return {
        out.push_str("            return nil\n");
    } else {
        out.push_str("            return\n");
    }
    out.push_str("        end\n");
    // Native dispatch path: call_guest_method routes through the host-mediated ABI.
    // Pass nil for the arena — a Lua peer caller has no per-caller CallArena; the
    // bridge falls back to host->alloc (same convention as the host caller's nil
    // arena comment).
    out.push_str(&format!(
        "        err = self._host.call_guest_method(self._host, self._instance, {fn_id}, args_ptr, out_ptr, nil)\n"
    ));
    out.push_str("    elseif dispatch_type == 1 then\n");
    // VM dispatch path: call_guest_method still applies; the host routes it through
    // the loader vm.call trampoline internally. Arena is nil for the same reason.
    out.push_str(&format!(
        "        err = self._host.call_guest_method(self._host, self._instance, {fn_id}, args_ptr, out_ptr, nil)\n"
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
fn generate_guest_host_contracts_file(ir: &ValidatedIr) -> String {
    let mut out: String = String::new();
    out.push_str(file_header());
    out.push_str("local ffi = require(\"ffi\")\n");
    // Require the polyplug_abi Lua SDK so the HostContractInterface / AbiError /
    // GuestContractInstance cdefs this module casts to are declared. Without this
    // require the ffi.cast(\"HostContractInterface*\", ...) below would fail at load.
    out.push_str("local polyplug_abi = require(\"polyplug_abi\")\n\n");

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
        "local DispatchFnType = ffi.typeof(\"AbiError (*)(const void*, const void*, void*)\")\n\n",
    );

    for contract in &ir.host_contracts {
        generate_lua_guest_host_contract_caller(&mut out, contract, &ir.enums);
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
    out.push_str("local ffi = require(\"ffi\")\n\n");

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
        "    typedef uint32_t (*PolyplugLuaHostDispatchCallback)(uint32_t, const void*, void*);\n",
    );
    out.push_str("    typedef struct PolyplugLuaHostDispatchBridge {\n");
    out.push_str("        PolyplugLuaHostDispatchCallback callback;\n");
    out.push_str("    } PolyplugLuaHostDispatchBridge;\n");
    out.push_str(
        "    AbiError polyplug_lua_host_vm_dispatch(VmLoaderData, GuestContractInstance, uint32_t, const void*, void*, CallArena*);\n",
    );
    out.push_str(
        "    HostContractInstance polyplug_lua_host_create_instance(const HostContractInterface*, const void*);\n",
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
    out.push_str("--     impl: implementation table with methods matching the contract\n");
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
        "function M.{factory_name}(impl, lua_bridge_lib)\n"
    ));
    out.push_str(&format!(
        "    if impl == nil then\n        error(\"{factory_name}: impl is nil\")\n    end\n"
    ));
    out.push_str(&format!(
        "    if lua_bridge_lib == nil then\n        error(\"{factory_name}: lua_bridge_lib is nil (pass the lua loader cdylib handle)\")\n    end\n\n"
    ));

    // Scalar-only dispatcher: (fn_id, args, out) -> AbiErrorCode number.
    out.push_str("    local function dispatch(fn_id, args, out)\n");
    out.push_str("        local ok, code = pcall(function()\n");
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

    // Bridge + interface construction. The callback cast anchors the LuaJIT
    // callback object; bridge and interface are plain cdata.
    out.push_str("    local callback = ffi.cast(\"PolyplugLuaHostDispatchCallback\", dispatch)\n");
    out.push_str("    local bridge = ffi.new(\"PolyplugLuaHostDispatchBridge\")\n");
    out.push_str("    bridge.callback = callback\n\n");

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
    out.push_str(
        "    interface.create_instance = lua_bridge_lib.polyplug_lua_host_create_instance\n",
    );
    out.push_str(
        "    interface.destroy_instance = lua_bridge_lib.polyplug_lua_host_destroy_instance\n",
    );
    out.push_str("    interface.dispatch.vm.call = lua_bridge_lib.polyplug_lua_host_vm_dispatch\n");
    out.push_str("    interface.dispatch.vm.loader_data.data = ffi.cast(\"void*\", bridge)\n\n");

    out.push_str(
        "    _anchors[#_anchors + 1] = { interface = interface, bridge = bridge, callback = callback, impl = impl }\n",
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
        generate_lua_guest_host_contract_caller(&mut out, &contract, &[]);
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
        // VM dispatch uses the canonical 6-arg call(loader_data, instance, fn_id, args,
        // out, nil) — loader_data, not the old bridge_data field.
        assert!(
            out.contains("interface.dispatch.vm.call(interface.dispatch.vm.loader_data,"),
            "must call vm.call with loader_data: {out}"
        );
        assert!(
            !out.contains("bridge_data"),
            "must use loader_data, not bridge_data: {out}"
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
        generate_lua_guest_host_contract_caller(&mut out, &contract, &[]);
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

    // ─── Guest Peer Caller Tests ───────────────────────────────────────────────

    #[test]
    fn peer_caller_emitted_for_declared_dependency() {
        use crate::ir::ResolvedBundle;
        use crate::ir::ResolvedDependency;
        use crate::ir::ResolvedPlugin;
        use crate::ir::Version;
        use polyplug_codegen::ResolvedBundleFile;

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
                }],
                returns: Some(ResolvedTypeRef::AbiType(AbiBuiltin::StringView)),
            }],
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
                    version: Version {
                        major: 1,
                        minor: 0,
                        patch: 0,
                    },
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

        let peers: Vec<&ResolvedContract> = collect_lua_peer_contracts(&ir);
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
            out.contains("function PipelineValidatorPeer.resolve()"),
            "missing resolve factory: {out}"
        );
        assert!(
            out.contains("polyplug_guest.get_host_interface()"),
            "resolve must use polyplug_guest.get_host_interface(): {out}"
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
            out.contains("interface.create_instance(host, nil)"),
            "must call create_instance: {out}"
        );
        assert!(
            out.contains("function PipelineValidatorPeer:validate(input)"),
            "missing validate method: {out}"
        );
        assert!(
            out.contains("call_guest_method(self._host, self._instance,"),
            "method must dispatch via call_guest_method: {out}"
        );
        // Arena must be nil — Lua peer callers have no per-caller CallArena.
        assert!(
            out.contains(", nil)"),
            "call_guest_method must pass nil arena: {out}"
        );
        assert!(
            out.contains("return out_val"),
            "missing return statement: {out}"
        );
    }

    #[test]
    fn no_peer_callers_without_dependencies() {
        use crate::ir::Version;

        let contract: ResolvedContract = ResolvedContract {
            name: "pipeline.Validator".to_owned(),
            contract_id: 0xAAAA_BBBB_CCCC_DDDD_u64,
            version: Version {
                major: 1,
                minor: 0,
                patch: 0,
            },
            functions: vec![],
        };

        // No bundle at all — no peer contracts.
        let ir_no_bundle: ValidatedIr = ValidatedIr {
            types: vec![],
            enums: vec![],
            contracts: vec![contract],
            host_contracts: vec![],
            bundle: None,
        };
        let peers: Vec<&ResolvedContract> = collect_lua_peer_contracts(&ir_no_bundle);
        assert!(
            peers.is_empty(),
            "should produce no peers when there is no bundle"
        );

        // Bundle with no dependencies — no peer contracts even if contracts exist.
        use crate::ir::ResolvedBundle;
        use crate::ir::ResolvedPlugin;
        use polyplug_codegen::ResolvedBundleFile;

        let contract2: ResolvedContract = ResolvedContract {
            name: "pipeline.Validator".to_owned(),
            contract_id: 0xAAAA_BBBB_CCCC_DDDD_u64,
            version: Version {
                major: 1,
                minor: 0,
                patch: 0,
            },
            functions: vec![],
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
                    version: Version {
                        major: 1,
                        minor: 0,
                        patch: 0,
                    },
                    implements: vec!["data.Transformer@1.0".to_owned()],
                    optional: vec![],
                }],
                bundle_id: 0x1234_5678_9ABC_DEF0_u64,
                dependencies: vec![],
                needs_reinit_on_dep_reload: false,
            }),
        };
        let peers2: Vec<&ResolvedContract> = collect_lua_peer_contracts(&ir_no_deps);
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
        use crate::ir::PrimitiveType;
        use crate::ir::Version;

        let func: ResolvedFunction = ResolvedFunction {
            name: "get_count".to_owned(),
            function_id: 0,
            params: vec![],
            returns: Some(ResolvedTypeRef::Primitive(PrimitiveType::U32)),
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
            out.contains("ffi.new(\"uint32_t[1]\")"),
            "scalar u32 return must use a 1-element array slot: {out}"
        );
        assert!(
            out.contains("return out_val[0]"),
            "scalar u32 return must read result with out_val[0]: {out}"
        );
        assert!(
            !out.contains("ffi.new(\"uint32_t\")"),
            "scalar u32 must NOT use a bare value slot (would yield NULL out_ptr): {out}"
        );
    }

    #[test]
    fn host_out_setup_string_view_keeps_struct_slot() {
        // A StringView return is a struct (reference cdata): out slot must stay
        // ffi.new("StringView") and the caller must return the raw handle.
        use crate::ir::Version;

        let func: ResolvedFunction = ResolvedFunction {
            name: "get_name".to_owned(),
            function_id: 0,
            params: vec![],
            returns: Some(ResolvedTypeRef::AbiType(AbiBuiltin::StringView)),
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
            out.contains("ffi.new(\"StringView\")"),
            "StringView return must use a bare struct slot: {out}"
        );
        assert!(
            !out.contains("ffi.new(\"StringView[1]\")"),
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
                }],
            }],
            contracts: vec![],
            host_contracts: vec![ResolvedHostContract {
                name: "host.logger".to_owned(),
                contract_id: 0x1234_5678_9ABC_DEF0_u64,
                version: crate::ir::Version {
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
                    },
                ],
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
            out.contains(
                "interface.create_instance = lua_bridge_lib.polyplug_lua_host_create_instance"
            ),
            "create_instance must use the native stub: {out}"
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
            }],
        }];
        let func: ResolvedFunction = ResolvedFunction {
            name: "set_level".to_owned(),
            function_id: 0,
            params: vec![ResolvedParam {
                name: "level".to_owned(),
                ty: ResolvedTypeRef::UserDefined("LogLevel".to_owned()),
            }],
            returns: None,
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
                },
                EnumVariant {
                    name: "Rgba8".to_owned(),
                    value: "1".to_owned(),
                },
            ],
        }]
    }

    fn enum_codec_contract() -> ResolvedContract {
        ResolvedContract {
            name: "image.Codec".to_owned(),
            contract_id: 0x1111_2222_3333_4444_u64,
            version: crate::ir::Version {
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
                    }],
                    returns: None,
                },
                ResolvedFunction {
                    name: "get_format".to_owned(),
                    function_id: 1,
                    params: vec![],
                    returns: Some(ResolvedTypeRef::UserDefined("PixelFormat".to_owned())),
                },
            ],
        }
    }

    fn assert_enum_caller_marshalling(out: &str) {
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
            out.contains("local out_val = ffi.new(\"uint32_t[1]\")"),
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
        assert_enum_caller_marshalling(&out);
    }

    #[test]
    fn lua_peer_caller_enum_param_and_return_use_repr_slots() {
        let mut out: String = String::new();
        generate_lua_guest_peer_caller(&mut out, &enum_codec_contract(), 1, &pixel_format_enums());
        assert_enum_caller_marshalling(&out);
    }

    #[test]
    fn lua_guest_host_contract_caller_enum_param_and_return_use_repr_slots() {
        let contract: ResolvedHostContract = ResolvedHostContract {
            name: "host.theme".to_owned(),
            contract_id: 0xDEAD_BEEF_u64,
            version: crate::ir::Version {
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
                    }],
                    returns: None,
                },
                ResolvedFunction {
                    name: "get_mode".to_owned(),
                    function_id: 1,
                    params: vec![],
                    returns: Some(ResolvedTypeRef::UserDefined("PixelFormat".to_owned())),
                },
            ],
        };
        let mut out: String = String::new();
        generate_lua_guest_host_contract_caller(&mut out, &contract, &pixel_format_enums());
        assert_enum_caller_marshalling(&out);
    }

    /// Scalar single params share the same LuaJIT pitfall: a scalar value cdata
    /// cast to void* converts the VALUE, not its address — so the caller must
    /// use the 1-element array form just like scalar out slots.
    #[test]
    fn lua_host_caller_single_scalar_param_uses_array_slot() {
        let contract: ResolvedContract = ResolvedContract {
            name: "counter.Inc".to_owned(),
            contract_id: 0x5555_6666_u64,
            version: crate::ir::Version {
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
                }],
                returns: None,
            }],
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
}
