#![allow(clippy::expect_used)]

use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use polyplug_codegen::{
    GenerateConfig, InternalLuaGenerateConfig, Lang, OutputDestination, OutputLayout,
    OutputPartition, Side, ValidatedImport, generate, generate_internal_lua, write_output,
};
use tempfile::TempDir;

fn write_api(path: &Path, contract: &str) {
    fs::write(
        path,
        format!(
            r#"
[[enum]]
name = "Mode"
repr = "u32"
[[enum.variants]]
name = "Ready"
value = "0"

[[types]]
name = "Inner"
fields = [
  {{ name = "label", type = "StringView" }},
  {{ name = "bytes", type = "Buffer" }},
]

[[types]]
name = "ArrayOf_ArrayOf_Inner"
fields = [
  {{ name = "items", type = "u64" }},
  {{ name = "len", type = "u64" }},
]

[[types]]
name = "Envelope"
fields = [
  {{ name = "inner", type = "Inner" }},
  {{ name = "entries", type = "ArrayOf_ArrayOf_Inner" }},
  {{ name = "mode", type = "Mode" }},
]

[[guest_contract]]
name = "{contract}"
version = "1.0"

[[guest_contract.functions]]
name = "metadata"
return = "Envelope"

[[guest_contract.functions]]
name = "read"
params = [{{ name = "address", type = "u64" }}, {{ name = "size", type = "u32" }}]
return = "Buffer"

[[guest_contract.functions]]
name = "write"
params = [{{ name = "label", type = "StringView" }}, {{ name = "mode", type = "Mode" }}, {{ name = "entries", type = "Array<Inner>" }}]
return = "StringView"

[[guest_contract.functions]]
name = "take_inner"
params = [{{ name = "value", type = "Inner" }}]
return = "u32"
"#
        ),
    )
    .expect("write Lua API fixture");
}

fn write_primitive_api(path: &Path, contract: &str) {
    fs::write(
        path,
        format!(
            r#"
[[guest_contract]]
name = "{contract}"
version = "1.0"

[[guest_contract.functions]]
name = "increment"
params = [{{ name = "value", type = "u32" }}]
return = "u32"

[[guest_contract.functions]]
name = "flush"
"#
        ),
    )
    .expect("write primitive Lua API fixture");
}

fn write_bundle(path: &Path, api: &str, bundle_name: &str, contract: &str) {
    fs::write(
        path,
        format!(
            "[bundle]\nname = \"{bundle_name}\"\nversion = \"1.0\"\napi = \"{api}\"\n\n[[plugin]]\nname = \"{bundle_name}.provider\"\nimplements = [\"{contract}@1.0\"]\n"
        ),
    )
    .expect("write Lua internal bundle fixture");
}

fn internal_output(temp: &TempDir, name: &str, contract: &str) -> polyplug_codegen::GenerateOutput {
    let api = temp.path().join(format!("{name}.toml"));
    let bundle = temp.path().join(format!("{name}.bundle.toml"));
    write_api(&api, contract);
    write_bundle(&bundle, &format!("{name}.toml"), name, contract);
    generate_internal_lua(InternalLuaGenerateConfig {
        bundle_toml: bundle,
        out_dir: temp.path().join("out"),
        layout: Default::default(),
    })
    .expect("generate Lua internal profile")
}

fn lua_path_literal(path: &Path) -> String {
    let path = path.display().to_string();
    for delimiter_len in 0.. {
        let delimiter = "=".repeat(delimiter_len);
        let closing = format!("]{delimiter}]");
        if !path.contains(&closing) {
            return format!("[{delimiter}[{path}]{delimiter}]");
        }
    }
    unreachable!("a finite path has an available Lua long-string delimiter")
}

#[test]
fn lua_path_literal_avoids_embedded_long_string_delimiters() {
    let path = Path::new("a]]b]=]c]==]d]===]e");
    assert_eq!(lua_path_literal(path), "[====[a]]b]=]c]==]d]===]e]====]");
}

#[test]
fn internal_lua_profile_is_opt_in_artifactless_and_typed() {
    let temp = TempDir::new().expect("create Lua profile fixture");
    let output = internal_output(&temp, "lua_internal_profile", "platform.Plugin");
    let paths = output
        .files
        .iter()
        .map(|file| file.path.clone())
        .collect::<HashSet<PathBuf>>();
    assert_eq!(
        paths.len(),
        5,
        "internal Lua profile has a fixed five-file surface"
    );
    let namespace = Path::new("internal").join(format!(
        "lua_internal_profile-{:016x}",
        polyplug_utils::bundle_id("lua_internal_profile")
    ));
    assert!(
        paths.iter().all(|path| path.starts_with(&namespace)),
        "internal Lua files must remain bundle-namespaced"
    );
    for required_path in [
        Path::new("guest").join("internal.lua"),
        Path::new("guest").join("types.lua"),
        Path::new("host").join("callers.lua"),
        Path::new("host").join("types.lua"),
        PathBuf::from("init.lua"),
    ] {
        assert!(
            paths.iter().any(|path| path.ends_with(&required_path)),
            "internal Lua profile missing `{}`",
            required_path.display()
        );
    }

    let internal = output
        .files
        .iter()
        .find(|file| file.path.ends_with(Path::new("guest").join("internal.lua")))
        .expect("internal Lua registrar")
        .content
        .as_str();
    for required in [
        "function M.providers(values)",
        "function M.register(runtime, providers)",
        "providers were consumed by a previous registration attempt",
        "must be a factory function",
        "native_bridge.create_resident(INTERNAL_MANIFEST)",
        "native_bridge.add_provider",
        "runtime:register_internal_plugin(resident)",
        "return_roots.values",
        "return_roots.strings = {}",
        "metadata",
        "read",
        "write",
        "take_inner",
    ] {
        assert!(
            internal.contains(required),
            "missing `{required}` in generated internal Lua profile"
        );
    }
    assert!(
        internal.contains("handles[1]") && internal.contains("create_from_handle"),
        "internal Lua registration must bind callers from committed handles"
    );
    assert!(
        internal.find("factories[")
            < internal.find("native_bridge.create_resident(INTERNAL_MANIFEST)"),
        "provider factories must be type-validated before resident allocation"
    );
    for required in [
        "native_bridge.release_resident(resident)",
        "constructed_callers",
        "previous:destroy()",
        "pcall(function() runtime:unload_bundle(bundle_id) end)",
    ] {
        assert!(
            internal.contains(required),
            "generated internal Lua rollback must include `{required}`"
        );
    }
    for forbidden in [
        "ffi.new(\"GuestContractInterface",
        "ffi.new(\"PluginDescriptor",
        "ffi.cast(uintptr_t, resident)",
        "runtime:register_internal_plugin(bundle)",
    ] {
        assert!(
            !internal.contains(forbidden),
            "generated internal Lua profile must not emit `{forbidden}`"
        );
    }
    assert!(
        internal.contains("ffi.cast(\"uint64_t\", ffi.cast(\"uintptr_t\", values_"),
        "internal array marshalling must assign the u64 ABI address, not pointer cdata"
    );
    let callers = output
        .files
        .iter()
        .find(|file| file.path.ends_with(Path::new("host").join("callers.lua")))
        .expect("internal host callers")
        .content
        .as_str();
    assert!(
        callers.contains("function M.PlatformPluginContract_create_from_handle"),
        "internal Lua callers must expose exact-handle construction"
    );
    for required in [
        "local retained_handle = ffi.new(\"GuestContractHandle\")",
        "_handle = retained_handle",
    ] {
        assert!(
            callers.contains(required),
            "exact-handle Lua callers must retain the ABI handle for revalidation: `{required}`"
        );
    }
    assert!(
        callers.contains("if interface == self._interface then"),
        "Lua callers must retain their instance when an unrelated registry revision resolves the same interface"
    );

    let api = temp.path().join("lua_internal_profile.toml");
    let external = generate(GenerateConfig {
        api_toml: api,
        lang: Lang::Lua,
        side: Side::Guest,
        layout: OutputLayout::unified(),
    })
    .expect("generate external Lua bindings");
    let external_paths = external
        .files
        .iter()
        .map(|file| file.path.clone())
        .collect::<HashSet<PathBuf>>();
    let expected_external_paths = [
        Path::new("guest").join("contracts.lua"),
        Path::new("guest").join("types.lua"),
    ]
    .into_iter()
    .collect::<HashSet<_>>();
    assert_eq!(
        external_paths, expected_external_paths,
        "external Lua guest generation must retain its canonical file set"
    );

    let external_host = generate(GenerateConfig {
        api_toml: temp.path().join("lua_internal_profile.toml"),
        lang: Lang::Lua,
        side: Side::Host,
        layout: OutputLayout::unified(),
    })
    .expect("generate external Lua host callers");
    let default_host_paths = external_host
        .files
        .iter()
        .map(|file| file.path.clone())
        .collect::<HashSet<PathBuf>>();
    let expected_default_host_paths = [
        Path::new("host").join("callers.lua"),
        Path::new("host").join("types.lua"),
    ]
    .into_iter()
    .collect::<HashSet<_>>();
    assert_eq!(
        default_host_paths, expected_default_host_paths,
        "default Lua host generation must retain its canonical file set"
    );
    let default_callers = external_host
        .files
        .iter()
        .find(|file| file.path == Path::new("host").join("callers.lua"))
        .expect("default Lua host callers")
        .content
        .as_str();
    assert!(
        default_callers.contains("function M.PlatformPluginContract_create("),
        "default Lua host callers must expose ordinary caller construction"
    );
}

#[test]
fn split_internal_lua_uses_canonical_modules_and_unloads_after_stateful_provider_setup() {
    let temp = TempDir::new().expect("create split Lua profile fixture");
    let api = temp.path().join("split.lua.toml");
    let bundle = temp.path().join("split.lua.bundle.toml");
    write_api(&api, "platform.Plugin");
    write_bundle(&bundle, "split.lua.toml", "split_lua", "platform.Plugin");
    let domain_root = temp.path().join("canonical-domain");
    let contracts_root = temp.path().join("canonical-contracts");
    let layout = OutputLayout {
        bindings: OutputDestination::Inline,
        domain_types: OutputDestination::Emit {
            root: domain_root.clone(),
            import: ValidatedImport::parse(Lang::Lua, "canonical.domain")
                .expect("valid Lua domain module"),
        },
        guest_contracts: OutputDestination::Emit {
            root: contracts_root.clone(),
            import: ValidatedImport::parse(Lang::Lua, "canonical.contracts")
                .expect("valid Lua contracts module"),
        },
    };
    let output = generate_internal_lua(InternalLuaGenerateConfig {
        bundle_toml: bundle,
        out_dir: temp.path().join("unused"),
        layout,
    })
    .expect("generate split Lua internal profile");
    let profile = output
        .files
        .iter()
        .find(|file| file.path.ends_with(Path::new("guest").join("internal.lua")))
        .expect("split internal Lua profile");
    let domain = output
        .files
        .iter()
        .find(|file| file.partition == OutputPartition::DomainTypes)
        .expect("canonical Lua domain module");
    let contracts = output
        .files
        .iter()
        .find(|file| file.partition == OutputPartition::GuestContracts)
        .expect("canonical Lua contracts module");
    assert!(profile.content.contains("require(\"canonical.domain\")"));
    assert!(profile.content.contains("require(\"canonical.contracts\")"));
    assert!(!profile.content.contains("guest/types.lua"));
    assert_eq!(domain.path.file_name(), Some("types.lua".as_ref()));
    assert_eq!(contracts.path.file_name(), Some("contracts.lua".as_ref()));
    assert!(contracts.content.contains(
        "---@field write fun(self: PlatformPluginContractProvider, label: string, mode: number, entries: userdata): string"
    ) && contracts
        .content
        .contains("---@field metadata fun(self: PlatformPluginContractProvider): table")
        && contracts.content.contains(
            "---@field split_lua_provider_platform_Plugin fun(host_ptr: lightuserdata): PlatformPluginContractProvider"
        )
        && contracts
            .content
            .contains("---@param values InternalProviderFactories"),
        "internal provider declarations must carry exact LuaLS function and factory annotations: {}",
        contracts.content
    );

    let bindings_root = temp.path().join("bindings");
    write_output(&output, &bindings_root).expect("write split Lua output");
    let script = temp.path().join("split-layout-e2e.lua");
    fs::write(
        &script,
        format!(
            r#"
local ffi = require("ffi")
ffi.cdef[[
typedef struct {{ const char *ptr; size_t len; }} StringView;
typedef struct {{ void *ptr; size_t len; size_t capacity; }} Buffer;
typedef struct {{ void *data; uint64_t contract_id; }} GuestContractInstance;
typedef struct {{ uint32_t index; uint32_t generation; }} GuestContractHandle;
typedef struct {{ uint64_t contract_id; }} GuestContractInterface;
typedef struct {{ uint32_t code; void *message; }} AbiError;
]]
local fake_interface = ffi.new("GuestContractInterface")
fake_interface.contract_id = 1
local factory_calls, unloaded, captured_dispatch, stateful_provider = 0, 0, nil, nil
package.preload["canonical.domain"] = function() return dofile({}) end
package.preload["canonical.contracts"] = function() return dofile({}) end
package.preload["polyplug.loaders.lua"] = function()
    return {{
        internal_plugin_bridge = function()
            return {{
                create_resident = function() return 1 end,
                add_provider = function(_, factory, dispatch)
                    stateful_provider = factory()
                    captured_dispatch = dispatch
                    local value = stateful_provider:metadata()
                    assert(value.mode == 0)
                    assert(#value.entries == 2)
                    assert(#value.entries[1] == 2)
                    return 1
                end,
                caller_resolve_from_handle = function() return 0 end,
                release_resident = function() end,
            }}
        end,
    }}
end
local bindings = dofile({})
local runtime = {{
    host = function() return 1 end,
    register_internal_plugin = function()
        return {{ bundle_id = 77, handles = {{ {{ index = 1, generation = 1 }} }} }}
    end,
    unload_bundle = function() unloaded = unloaded + 1 end,
}}
local ok = pcall(function()
    bindings.register(runtime, bindings.providers({{
        split_lua_provider_platform_Plugin = function()
            factory_calls = factory_calls + 1
            return {{
                metadata = function()
                    return {{
                        inner = {{ label = "state", bytes = "" }},
                        entries = {{
                            {{ {{ label = "a", bytes = "" }}, {{ label = "b", bytes = "" }} }},
                            {{ {{ label = "c", bytes = "" }} }},
                        }},
                        mode = 0,
                    }}
                end,
                read = function() return "" end,
                write = function() return "" end,
                take_inner = function() return 0 end,
            }}
        end,
    }}))
end)
assert(not ok)
assert(factory_calls == 1)
assert(unloaded == 1)
local output = ffi.new("Envelope[1]")
local roots = {{ strings = {{}}, buffers = {{}}, values = {{}} }}
assert(captured_dispatch(stateful_provider, 0, nil, output, roots) == 0)
assert(output[0].mode == 0)
assert(output[0].entries.len == 2)
local outer = ffi.cast("ArrayOf_Inner*", ffi.cast("uintptr_t", output[0].entries.items))
assert(outer[0].len == 2)
"#,
            lua_path_literal(&domain_root.join(&domain.path)),
            lua_path_literal(&contracts_root.join(&contracts.path)),
            lua_path_literal(&bindings_root.join(&profile.path)),
        ),
    )
    .expect("write split Lua E2E script");
    let result = Command::new("luajit")
        .arg(&script)
        .output()
        .expect("run split Lua E2E");
    assert!(
        result.status.success(),
        "split Lua internal profile must execute through canonical modules\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );
}

#[test]
fn split_guest_lua_loads_external_domain_and_contract_modules() {
    let temp = TempDir::new().expect("create split guest Lua fixture");
    let api = temp.path().join("guest.lua.toml");
    let bundle = temp.path().join("guest.lua.bundle.toml");
    write_api(&api, "platform.Plugin");
    write_bundle(&bundle, "guest.lua.toml", "guest_lua", "platform.Plugin");
    let bundle_contents = fs::read_to_string(&bundle).expect("read Lua bundle fixture");
    fs::write(
        &bundle,
        bundle_contents.replacen(
            "[bundle]\n",
            "[bundle]\nloader = \"lua\"\nfile = \"guest.lua\"\n",
            1,
        ),
    )
    .expect("add Lua loader fields");
    let domain_root = temp.path().join("canonical-domain");
    let contracts_root = temp.path().join("canonical-contracts");
    let output = generate(GenerateConfig {
        api_toml: bundle,
        lang: Lang::Lua,
        side: Side::Guest,
        layout: OutputLayout {
            bindings: OutputDestination::Inline,
            domain_types: OutputDestination::Emit {
                root: domain_root.clone(),
                import: ValidatedImport::parse(Lang::Lua, "canonical.domain")
                    .expect("valid Lua domain module"),
            },
            guest_contracts: OutputDestination::Emit {
                root: contracts_root.clone(),
                import: ValidatedImport::parse(Lang::Lua, "canonical.contracts")
                    .expect("valid Lua contracts module"),
            },
        },
    })
    .expect("generate split Lua guest bindings");
    let domain = output
        .files
        .iter()
        .find(|file| file.partition == OutputPartition::DomainTypes)
        .expect("Lua domain declaration");
    let contracts = output
        .files
        .iter()
        .find(|file| file.partition == OutputPartition::GuestContracts)
        .expect("Lua contract declaration");
    let bindings = output
        .files
        .iter()
        .find(|file| file.path == Path::new("guest").join("bindings.lua"))
        .expect("Lua native registration bindings");
    assert!(bindings.content.contains("require(\"canonical.domain\")"));
    assert!(
        bindings
            .content
            .contains("require(\"canonical.contracts\")")
    );
    assert!(
        contracts
            .content
            .contains("---@param factory fun(host_ptr: integer): PlatformPluginContractProvider"),
        "ordinary Lua provider factory annotation must match the loader's integer host pointer: {}",
        contracts.content
    );
    assert!(
        output
            .files
            .iter()
            .filter(|file| file.partition == OutputPartition::DomainTypes)
            .all(|file| file.path != Path::new("guest").join("bindings.lua")),
        "Lua native bindings must not duplicate domain declarations"
    );

    let bindings_root = temp.path().join("bindings");
    write_output(&output, &bindings_root).expect("write split Lua guest output");
    let script = temp.path().join("split-guest-e2e.lua");
    fs::write(
        &script,
        format!(
            r#"
local ffi = require("ffi")
ffi.cdef[[
typedef struct {{ const char *ptr; size_t len; }} StringView;
typedef struct {{ void *ptr; size_t len; size_t capacity; }} Buffer;
]]
package.preload["canonical.domain"] = function() return dofile({}) end
package.preload["canonical.contracts"] = function() return dofile({}) end
package.preload["polyplug_guest"] = function()
    return {{ AbiErrorCode = {{ Ok = 0, Generic = 1 }} }}
end
local contracts = dofile({})
local calls = 0
contracts.set_guest_lua_provider_factory(function()
    return {{
        metadata = function()
            calls = calls + 1
            return {{
                inner = {{ label = "state", bytes = "" }},
                entries = {{
                    {{ {{ label = "a", bytes = "" }}, {{ label = "b", bytes = "" }} }},
                    {{ {{ label = "c", bytes = "" }} }},
                }},
                mode = 0,
            }}
        end,
        read = function() return "" end,
        write = function() return "" end,
        take_inner = function() return 0 end,
    }}
end)
local registrations, err = polyplug_init(1, 1)
assert(err.code == 0)
local implementation = registrations["platform.Plugin"].factory()
assert(implementation:metadata().entries[1][2].label == "b")
assert(implementation:metadata().mode == 0)
assert(calls == 2)
"#,
            lua_path_literal(&domain_root.join(&domain.path)),
            lua_path_literal(&contracts_root.join(&contracts.path)),
            lua_path_literal(&bindings_root.join(&bindings.path)),
        ),
    )
    .expect("write split Lua guest E2E script");
    let result = Command::new("luajit")
        .arg(&script)
        .output()
        .expect("run split Lua guest E2E");
    assert!(
        result.status.success(),
        "split Lua guest bindings must execute through external declarations\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );
}

#[test]
fn primitive_lua_omits_domain_types_and_dispatches_external_and_internal_profiles() {
    let temp = TempDir::new().expect("create primitive Lua fixture");
    let api = temp.path().join("primitive.lua.toml");
    let bundle = temp.path().join("primitive.lua.bundle.toml");
    write_primitive_api(&api, "primitive.Plugin");
    write_bundle(
        &bundle,
        "primitive.lua.toml",
        "primitive_lua",
        "primitive.Plugin",
    );
    let bundle_contents = fs::read_to_string(&bundle).expect("read primitive Lua bundle");
    fs::write(
        &bundle,
        bundle_contents.replacen(
            "[bundle]\n",
            "[bundle]\nloader = \"lua\"\nfile = \"guest.lua\"\n",
            1,
        ),
    )
    .expect("add primitive Lua loader fields");
    let layout = OutputLayout {
        bindings: OutputDestination::Inline,
        domain_types: OutputDestination::Omit,
        guest_contracts: OutputDestination::Omit,
    };
    let external = generate(GenerateConfig {
        api_toml: bundle.clone(),
        lang: Lang::Lua,
        side: Side::Guest,
        layout: layout.clone(),
    })
    .expect("generate primitive external Lua bindings");
    layout
        .validate(Lang::Lua, &external.files)
        .expect("primitive external Omit layout");
    assert!(
        !external
            .files
            .iter()
            .any(|file| file.partition == OutputPartition::DomainTypes),
        "primitive external bindings must not emit a DomainTypes file"
    );
    let external_bindings = external
        .files
        .iter()
        .find(|file| file.path == Path::new("guest").join("bindings.lua"))
        .expect("primitive external bindings");
    assert!(
        !external_bindings.content.contains("domain_types")
            && !external_bindings.content.contains("types.lua")
            && external_bindings
                .content
                .contains("---@field flush fun(self: PrimitivePluginContractProvider): nil"),
        "primitive external bindings must self-contain scalar and no-arg dispatch: {}",
        external_bindings.content
    );
    let external_root = temp.path().join("external");
    write_output(&external, &external_root).expect("write primitive external bindings");
    let external_script = temp.path().join("primitive-external.lua");
    fs::write(
        &external_script,
        format!(
            r#"
local ffi = require("ffi")
package.preload["polyplug_guest"] = function()
    return {{ AbiErrorCode = {{ Ok = 0, Generic = 1 }} }}
end
local bindings = dofile({})
local flushed = false
bindings.set_primitive_lua_provider_factory(function()
    return {{
        increment = function(_, value) return value + 1 end,
        flush = function() flushed = true end,
    }}
end)
local registrations, err = polyplug_init(1, 1)
assert(err.code == 0)
local implementation = registrations["primitive.Plugin"].factory()
local input = ffi.new("uint32_t[1]", 41)
local output = ffi.new("uint32_t[1]")
registrations["primitive.Plugin"].functions[0](implementation, input, output, nil, nil)
registrations["primitive.Plugin"].functions[1](implementation, nil, nil, nil, nil)
assert(output[0] == 42)
assert(flushed)
"#,
            lua_path_literal(&external_root.join(&external_bindings.path)),
        ),
    )
    .expect("write primitive external dispatch script");
    let external_result = Command::new("luajit")
        .arg(&external_script)
        .output()
        .expect("run primitive external dispatch");
    assert!(
        external_result.status.success(),
        "primitive external bindings must load and dispatch\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&external_result.stdout),
        String::from_utf8_lossy(&external_result.stderr),
    );

    let internal = generate_internal_lua(InternalLuaGenerateConfig {
        bundle_toml: bundle,
        out_dir: temp.path().join("unused"),
        layout: layout.clone(),
    })
    .expect("generate primitive internal Lua bindings");
    layout
        .validate(Lang::Lua, &internal.files)
        .expect("primitive internal Omit layout");
    assert!(
        !internal
            .files
            .iter()
            .any(|file| file.partition == OutputPartition::DomainTypes),
        "primitive internal bindings must not emit a DomainTypes file"
    );
    let profile = internal
        .files
        .iter()
        .find(|file| file.path.ends_with(Path::new("guest").join("internal.lua")))
        .expect("primitive internal profile");
    assert!(
        !profile.content.contains("types.lua")
            && !profile.content.contains("local types")
            && profile.content.contains("function M.providers(values)"),
        "primitive internal profile must self-contain its provider surface: {}",
        profile.content
    );
    let internal_root = temp.path().join("internal");
    write_output(&internal, &internal_root).expect("write primitive internal profile");
    let internal_script = temp.path().join("primitive-internal.lua");
    fs::write(
        &internal_script,
        format!(
            r#"
local ffi = require("ffi")
ffi.cdef[[
typedef struct {{ const char *ptr; size_t len; }} StringView;
typedef struct {{ void *ptr; size_t len; size_t capacity; }} Buffer;
typedef struct {{ void *data; uint64_t contract_id; }} GuestContractInstance;
typedef struct {{ uint32_t index; uint32_t generation; }} GuestContractHandle;
typedef struct {{ uint64_t contract_id; }} GuestContractInterface;
typedef struct {{ uint32_t code; void *message; }} AbiError;
]]
local captured_dispatch, implementation, flushed = nil, nil, false
package.preload["polyplug.loaders.lua"] = function()
    return {{
        internal_plugin_bridge = function()
            return {{
                create_resident = function() return 1 end,
                add_provider = function(_, factory, dispatch)
                    implementation = factory(1)
                    captured_dispatch = dispatch
                    return 1
                end,
                caller_resolve_from_handle = function() return 0 end,
                release_resident = function() end,
            }}
        end,
    }}
end
local profile = dofile({})
local runtime = {{
    host = function() return 1 end,
    register_internal_plugin = function()
        return {{ bundle_id = 1, handles = {{ {{ index = 1, generation = 1 }} }} }}
    end,
    unload_bundle = function() end,
}}
assert(not pcall(function()
    profile.register(runtime, profile.providers({{
        primitive_lua_provider_primitive_Plugin = function()
            return {{
                increment = function(_, value) return value + 1 end,
                flush = function() flushed = true end,
            }}
        end,
    }}))
end))
local input = ffi.new("uint32_t[1]", 41)
local output = ffi.new("uint32_t[1]")
assert(captured_dispatch(implementation, 0, input, output, {{}}) == 0)
assert(captured_dispatch(implementation, 1, nil, nil, {{}}) == 0)
assert(output[0] == 42)
assert(flushed)
"#,
            lua_path_literal(&internal_root.join(&profile.path)),
        ),
    )
    .expect("write primitive internal dispatch script");
    let internal_result = Command::new("luajit")
        .arg(&internal_script)
        .output()
        .expect("run primitive internal dispatch");
    assert!(
        internal_result.status.success(),
        "primitive internal profile must load and dispatch\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&internal_result.stdout),
        String::from_utf8_lossy(&internal_result.stderr),
    );
}

#[test]
fn omitted_lua_contracts_keep_domain_reusable_and_dispatch_from_bindings() {
    let temp = TempDir::new().expect("create omitted Lua contracts fixture");
    let api = temp.path().join("guest.lua.toml");
    let bundle = temp.path().join("guest.lua.bundle.toml");
    write_api(&api, "platform.Plugin");
    write_bundle(&bundle, "guest.lua.toml", "guest_lua", "platform.Plugin");
    let bundle_contents = fs::read_to_string(&bundle).expect("read Lua bundle fixture");
    fs::write(
        &bundle,
        bundle_contents.replacen(
            "[bundle]\n",
            "[bundle]\nloader = \"lua\"\nfile = \"guest.lua\"\n",
            1,
        ),
    )
    .expect("add Lua loader fields");
    let domain_root = temp.path().join("canonical-domain");
    let layout = OutputLayout {
        bindings: OutputDestination::Inline,
        domain_types: OutputDestination::Emit {
            root: domain_root.clone(),
            import: ValidatedImport::parse(Lang::Lua, "canonical.domain")
                .expect("valid Lua domain module"),
        },
        guest_contracts: OutputDestination::Omit,
    };
    let output = generate(GenerateConfig {
        api_toml: bundle.clone(),
        lang: Lang::Lua,
        side: Side::Guest,
        layout: layout.clone(),
    })
    .expect("generate Lua bindings with omitted contracts");
    layout
        .validate(Lang::Lua, &output.files)
        .expect("omitted Lua contracts must have no declaration dependency");
    let domain = output
        .files
        .iter()
        .find(|file| file.partition == OutputPartition::DomainTypes)
        .expect("Lua domain declaration");
    let bindings = output
        .files
        .iter()
        .find(|file| file.path == Path::new("guest").join("bindings.lua"))
        .expect("Lua registration bindings");
    assert!(
        !domain.content.contains("PlatformPluginContractReadArgs")
            && !domain.content.contains("PlatformPluginContractWriteArgs"),
        "domain types must not carry per-contract ABI packs: {}",
        domain.content
    );
    assert!(
        !bindings.content.contains("contracts.lua")
            && !bindings.content.contains("guest_contracts")
            && bindings.content.contains("local factories = {}")
            && bindings
                .content
                .contains("function M.set_guest_lua_provider_factory(factory)"),
        "omitted contracts must keep the local provider factory surface in bindings: {}",
        bindings.content
    );
    assert!(
        bindings
            .content
            .contains("} PlatformPluginContractReadArgs;")
            && bindings
                .content
                .contains("} PlatformPluginContractWriteArgs;"),
        "multi-argument ABI packs must remain in bindings: {}",
        bindings.content
    );
    assert!(
        bindings
            .content
            .contains(
                "---@field write fun(self: PlatformPluginContractProvider, label: userdata, mode: number, entries: userdata): string"
            )
            && bindings
                .content
                .contains("---@field metadata fun(self: PlatformPluginContractProvider): table")
            && bindings.content.contains(
                "---@param factory fun(host_ptr: integer): PlatformPluginContractProvider"
            ),
        "provider annotations must describe exact function, return, and factory shapes: {}",
        bindings.content
    );

    let changed_api = temp.path().join("changed.lua.toml");
    let changed_bundle = temp.path().join("changed.lua.bundle.toml");
    let changed_api_contents = fs::read_to_string(&api).expect("read Lua API fixture");
    fs::write(
        &changed_api,
        changed_api_contents.replacen(
            "name = \"read\"\nparams = [{ name = \"address\", type = \"u64\" }, { name = \"size\", type = \"u32\" }]",
            "name = \"read\"\nparams = [{ name = \"address\", type = \"u64\" }, { name = \"size\", type = \"u64\" }]",
            1,
        ),
    )
    .expect("change only a contract signature");
    write_bundle(
        &changed_bundle,
        "changed.lua.toml",
        "changed_lua",
        "platform.Plugin",
    );
    let changed_bundle_contents =
        fs::read_to_string(&changed_bundle).expect("read changed Lua bundle fixture");
    fs::write(
        &changed_bundle,
        changed_bundle_contents.replacen(
            "[bundle]\n",
            "[bundle]\nloader = \"lua\"\nfile = \"guest.lua\"\n",
            1,
        ),
    )
    .expect("add changed Lua loader fields");
    let changed = generate(GenerateConfig {
        api_toml: changed_bundle,
        lang: Lang::Lua,
        side: Side::Guest,
        layout,
    })
    .expect("regenerate changed Lua contract against the same domain");
    let changed_domain = changed
        .files
        .iter()
        .find(|file| file.partition == OutputPartition::DomainTypes)
        .expect("changed Lua domain declaration");
    assert_eq!(
        domain.content, changed_domain.content,
        "a contract signature change must reuse byte-identical application domain declarations"
    );
    let changed_bindings = changed
        .files
        .iter()
        .find(|file| file.path == Path::new("guest").join("bindings.lua"))
        .expect("changed Lua registration bindings");
    assert!(
        changed_bindings.content.contains("uint64_t size;")
            && changed_bindings.content != bindings.content,
        "the changed contract signature must move only the binding ABI pack"
    );

    let bindings_root = temp.path().join("bindings");
    write_output(&output, &bindings_root).expect("write omitted Lua output");
    let script = temp.path().join("omitted-contracts-e2e.lua");
    fs::write(
        &script,
        format!(
            r#"
local ffi = require("ffi")
ffi.cdef[[
typedef struct {{ const char *ptr; size_t len; }} StringView;
typedef struct {{ void *ptr; size_t len; size_t capacity; }} Buffer;
]]
package.preload["canonical.domain"] = function() return dofile({}) end
package.preload["polyplug_guest"] = function()
    return {{ AbiErrorCode = {{ Ok = 0, Generic = 1 }} }}
end
local bindings = dofile({})
bindings.set_guest_lua_provider_factory(function()
    return {{
        metadata = function()
            return {{
                inner = {{ label = "state", bytes = "" }},
                entries = {{{{ {{ label = "a", bytes = "" }} }}}},
                mode = 0,
            }}
        end,
        read = function() return "" end,
        write = function() return "" end,
        take_inner = function() return 0 end,
    }}
end)
local registrations, err = polyplug_init(1, 1)
assert(err.code == 0)
local registration = registrations["platform.Plugin"]
assert(registration.factory ~= nil)
local input = ffi.new("Inner[1]")
local output = ffi.new("uint32_t[1]")
local roots = {{ strings = {{}}, buffers = {{}}, values = {{}} }}
registration.functions[3](registration.factory(), input, output, roots)
assert(output[0] == 0)
"#,
            lua_path_literal(&domain_root.join(&domain.path)),
            lua_path_literal(&bindings_root.join(&bindings.path)),
        ),
    )
    .expect("write omitted Lua E2E script");
    let result = Command::new("luajit")
        .arg(&script)
        .output()
        .expect("run omitted Lua guest E2E");
    assert!(
        result.status.success(),
        "omitted Lua contracts must register and dispatch\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );
}

#[test]
fn omitted_internal_lua_contracts_keep_provider_surface_in_bindings() {
    let temp = TempDir::new().expect("create omitted internal Lua fixture");
    let api = temp.path().join("internal.lua.toml");
    let bundle = temp.path().join("internal.lua.bundle.toml");
    write_api(&api, "platform.Plugin");
    write_bundle(
        &bundle,
        "internal.lua.toml",
        "internal_lua",
        "platform.Plugin",
    );
    let domain_root = temp.path().join("canonical-domain");
    let layout = OutputLayout {
        bindings: OutputDestination::Inline,
        domain_types: OutputDestination::Emit {
            root: domain_root.clone(),
            import: ValidatedImport::parse(Lang::Lua, "canonical.domain")
                .expect("valid Lua domain module"),
        },
        guest_contracts: OutputDestination::Omit,
    };
    let output = generate_internal_lua(InternalLuaGenerateConfig {
        bundle_toml: bundle,
        out_dir: temp.path().join("unused"),
        layout: layout.clone(),
    })
    .expect("generate omitted internal Lua contracts");
    layout
        .validate(Lang::Lua, &output.files)
        .expect("omitted internal contracts must not remain a dependency");
    let profile = output
        .files
        .iter()
        .find(|file| file.path.ends_with(Path::new("guest").join("internal.lua")))
        .expect("internal Lua profile");
    let domain = output
        .files
        .iter()
        .find(|file| file.partition == OutputPartition::DomainTypes)
        .expect("canonical Lua domain");
    assert!(
        !output
            .files
            .iter()
            .any(|file| file.partition == OutputPartition::GuestContracts)
            && !profile.content.contains("guest_contracts")
            && !profile.content.contains("contracts.lua")
            && profile.content.contains("function M.providers(values)"),
        "internal Omit must keep providers in bindings without a contract loader: {}",
        profile.content
    );

    let bindings_root = temp.path().join("bindings");
    write_output(&output, &bindings_root).expect("write omitted internal Lua output");
    let script = temp.path().join("omitted-internal-load.lua");
    fs::write(
        &script,
        format!(
            r#"
local ffi = require("ffi")
ffi.cdef[[
typedef struct {{ const char *ptr; size_t len; }} StringView;
typedef struct {{ void *ptr; size_t len; size_t capacity; }} Buffer;
typedef struct {{ void *data; uint64_t contract_id; }} GuestContractInstance;
typedef struct {{ uint32_t index; uint32_t generation; }} GuestContractHandle;
typedef struct {{ uint64_t contract_id; }} GuestContractInterface;
typedef struct {{ uint32_t code; void *message; }} AbiError;
]]
package.preload["canonical.domain"] = function() return dofile({}) end
package.preload["polyplug.loaders.lua"] = function()
    return {{ internal_plugin_bridge = function() return {{}} end }}
end
local profile = dofile({})
assert(type(profile.providers) == "function")
assert(profile.providers({{}})._consumed == false)
"#,
            lua_path_literal(&domain_root.join(&domain.path)),
            lua_path_literal(&bindings_root.join(&profile.path)),
        ),
    )
    .expect("write omitted internal Lua load script");
    let result = Command::new("luajit")
        .arg(&script)
        .output()
        .expect("load omitted internal Lua profile");
    assert!(
        result.status.success(),
        "omitted internal Lua profile must load without contract declarations\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );
}

#[test]
fn two_internal_lua_bundles_write_to_distinct_namespaces() {
    let temp = TempDir::new().expect("create Lua coexistence fixture");
    let first = internal_output(&temp, "lua_first", "first.Plugin");
    let second = internal_output(&temp, "lua_second", "second.Plugin");
    let first_paths = first
        .files
        .iter()
        .map(|file| file.path.clone())
        .collect::<HashSet<PathBuf>>();
    let second_paths = second
        .files
        .iter()
        .map(|file| file.path.clone())
        .collect::<HashSet<PathBuf>>();
    assert!(
        first_paths.is_disjoint(&second_paths),
        "internal Lua bundles must have distinct output namespaces"
    );

    let out_dir = temp.path().join("coexisting");
    write_output(&first, &out_dir).expect("write first namespaced Lua profile");
    write_output(&second, &out_dir).expect("write second namespaced Lua profile");
    assert_eq!(
        out_dir
            .join("internal")
            .read_dir()
            .expect("internal output directory")
            .count(),
        2,
        "both generated Lua namespaces must coexist on disk"
    );
}

#[test]
fn generated_internal_lua_profile_marshals_nonempty_nested_arrays() {
    let temp = TempDir::new().expect("create nested Lua profile fixture");
    let output = internal_output(&temp, "nested_arrays", "platform.Plugin");
    let out_dir = temp.path().join("generated");
    write_output(&output, &out_dir).expect("write generated Lua profile");
    let profile = output
        .files
        .iter()
        .find(|file| file.path.ends_with(Path::new("guest").join("internal.lua")))
        .expect("generated internal Lua profile");
    let profile_path = out_dir.join(&profile.path);
    let script = temp.path().join("nested-arrays-e2e.lua");
    fs::write(
        &script,
        format!(
            r#"
local ffi = require("ffi")
ffi.cdef[[
typedef struct {{ const char *ptr; size_t len; }} StringView;
typedef struct {{ void *ptr; size_t len; size_t capacity; }} Buffer;
typedef struct {{ void *data; uint64_t contract_id; }} GuestContractInstance;
typedef struct {{ uint32_t index; uint32_t generation; }} GuestContractHandle;
typedef struct {{ uint64_t contract_id; }} GuestContractInterface;
typedef struct {{ uint32_t code; void *message; }} AbiError;
]]
local captured_factory
local captured_dispatch
local factory_calls = 0
local fake_interface = ffi.new("GuestContractInterface")
fake_interface.contract_id = 1
package.preload["polyplug.loaders.lua"] = function()
    return {{
        internal_plugin_bridge = function()
            return {{
                create_resident = function() return 1 end,
                add_provider = function(_, factory, dispatch)
                    captured_factory = factory
                    captured_dispatch = dispatch
                    return true
                end,
                caller_resolve_from_handle = function()
                    return 1, 1, 1, tonumber(ffi.cast("uintptr_t", ffi.cast("void*", fake_interface))), 1, captured_factory
                end,
                caller_create_with_implementation = function() return 1 end,
                caller_destroy = function() end,
                caller_reset = function() return 0, 0, 0, 0 end,
            }}
        end,
    }}
end
local bindings = dofile({})
local runtime = {{
    host = function() return 1 end,
    register_internal_plugin = function()
        return {{ bundle_id = 1, handles = {{ {{ index = 1, generation = 1 }} }} }}
    end,
}}
bindings.register(runtime, bindings.providers({{
    nested_arrays_provider_platform_Plugin = function()
        factory_calls = factory_calls + 1
        return {{
            metadata = function()
                return {{
                    inner = {{ label = "z", bytes = "" }},
                    entries = {{
                        {{ {{ label = "a", bytes = "" }}, {{ label = "b", bytes = "" }} }},
                        {{ {{ label = "c", bytes = "" }} }},
                    }},
                    mode = 0,
                }}
            end,
            read = function() return "" end,
            write = function() return "" end,
            take_inner = function() return 0 end,
        }}
    end,
}}))
assert(factory_calls == 1, "successful factory must run once for its created caller")
assert(captured_dispatch ~= nil)
local provider = captured_factory()
local output = ffi.new("Envelope[1]")
local roots = {{ strings = {{}}, buffers = {{}}, values = {{}} }}
assert(captured_dispatch(provider, 0, nil, output, roots) == 0)
assert(output[0].entries.len == 2)
local outer = ffi.cast("ArrayOf_Inner*", ffi.cast("uintptr_t", output[0].entries.items))
assert(outer[0].len == 2)
assert(outer[1].len == 1)
local first = ffi.cast("Inner*", ffi.cast("uintptr_t", outer[0].items))
assert(first[0].label.len == 1)
assert(first[1].label.len == 1)
assert(#roots.values == 3)
"#,
            lua_path_literal(&profile_path)
        ),
    )
    .expect("write nested Lua E2E script");
    let result = Command::new("luajit")
        .arg(&script)
        .output()
        .expect("run generated nested Lua profile");
    assert!(
        result.status.success(),
        "generated nested Lua array profile must execute\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );
}

#[test]
fn generated_internal_lua_profile_rolls_back_earlier_callers_before_unload() {
    let temp = TempDir::new().expect("create rollback Lua profile fixture");
    let api = temp.path().join("rollback.toml");
    let bundle = temp.path().join("rollback.bundle.toml");
    write_api(&api, "platform.Plugin");
    fs::write(
        &bundle,
        "[bundle]\nname = \"rollback\"\nversion = \"1.0\"\napi = \"rollback.toml\"\n\n[[plugin]]\nname = \"rollback.one\"\nimplements = [\"platform.Plugin@1.0\"]\n\n[[plugin]]\nname = \"rollback.two\"\nimplements = [\"platform.Plugin@1.0\"]\n",
    )
    .expect("write multi-provider Lua bundle fixture");
    let output = generate_internal_lua(InternalLuaGenerateConfig {
        bundle_toml: bundle,
        out_dir: temp.path().join("out"),
        layout: Default::default(),
    })
    .expect("generate rollback Lua profile");
    let out_dir = temp.path().join("generated");
    write_output(&output, &out_dir).expect("write rollback Lua profile");
    let profile = output
        .files
        .iter()
        .find(|file| file.path.ends_with(Path::new("guest").join("internal.lua")))
        .expect("generated internal Lua profile");
    let profile_path = out_dir.join(&profile.path);
    let script = temp.path().join("caller-rollback-e2e.lua");
    fs::write(
        &script,
        format!(
            r#"
local ffi = require("ffi")
ffi.cdef[[
typedef struct {{ const char *ptr; size_t len; }} StringView;
typedef struct {{ void *ptr; size_t len; size_t capacity; }} Buffer;
typedef struct {{ void *data; uint64_t contract_id; }} GuestContractInstance;
typedef struct {{ uint32_t index; uint32_t generation; }} GuestContractHandle;
typedef struct {{ uint64_t contract_id; }} GuestContractInterface;
typedef struct {{ uint32_t code; void *message; }} AbiError;
]]
local created, added, destroyed, unloaded = 0, 0, 0, 0
local fake_interface = ffi.new("GuestContractInterface")
fake_interface.contract_id = 1
local factory = function() return {{}} end
package.preload["polyplug.loaders.lua"] = function()
    return {{
        internal_plugin_bridge = function()
            return {{
                create_resident = function() created = created + 1; return 1 end,
                add_provider = function() added = added + 1; return 1 end,
                release_resident = function() error("resident must transfer before caller rollback") end,
                caller_resolve_from_handle = function(_, index)
                    if index == 2 then return 0 end
                    return 1, 1, 1, tonumber(ffi.cast("uintptr_t", ffi.cast("void*", fake_interface))), 1, factory
                end,
                caller_create_with_implementation = function() return 1 end,
                caller_destroy = function() destroyed = destroyed + 1 end,
                caller_reset = function() return 0, 0, 0, 0 end,
            }}
        end,
    }}
end
local bindings = dofile({})
local runtime = {{
    host = function() return 1 end,
    register_internal_plugin = function()
        return {{ bundle_id = 71, handles = {{ {{ index = 1, generation = 1 }}, {{ index = 2, generation = 1 }} }} }}
    end,
    unload_bundle = function() unloaded = unloaded + 1 end,
}}
local ok, err = pcall(function()
    bindings.register(runtime, bindings.providers({{
        rollback_one_platform_Plugin = factory,
        rollback_two_platform_Plugin = factory,
    }}))
end)
assert(not ok)
assert(tostring(err):find("generated caller construction failed for rollback_two_platform_Plugin", 1, true))
assert(created == 1)
assert(added == 2)
assert(destroyed == 1)
assert(unloaded == 1)
"#,
            lua_path_literal(&profile_path)
        ),
    )
    .expect("write caller rollback Lua E2E script");
    let result = Command::new("luajit")
        .arg(&script)
        .output()
        .expect("run generated caller rollback profile");
    assert!(
        result.status.success(),
        "generated caller rollback must destroy earlier callers before unloading\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );
}

#[test]
fn generated_internal_lua_profile_rejects_non_factory_before_resident_allocation() {
    let temp = TempDir::new().expect("create factory-validation Lua profile fixture");
    let output = internal_output(&temp, "factory_validation", "platform.Plugin");
    let out_dir = temp.path().join("generated");
    write_output(&output, &out_dir).expect("write factory-validation Lua profile");
    let profile = output
        .files
        .iter()
        .find(|file| file.path.ends_with(Path::new("guest").join("internal.lua")))
        .expect("generated internal Lua profile");
    let profile_path = out_dir.join(&profile.path);
    let script = temp.path().join("factory-validation-e2e.lua");
    fs::write(
        &script,
        format!(
            r#"
local ffi = require("ffi")
ffi.cdef[[
typedef struct {{ const char *ptr; size_t len; }} StringView;
typedef struct {{ void *ptr; size_t len; size_t capacity; }} Buffer;
typedef struct {{ void *data; uint64_t contract_id; }} GuestContractInstance;
typedef struct {{ uint32_t index; uint32_t generation; }} GuestContractHandle;
typedef struct {{ uint64_t contract_id; }} GuestContractInterface;
typedef struct {{ uint32_t code; void *message; }} AbiError;
]]
local allocations = 0
package.preload["polyplug.loaders.lua"] = function()
    return {{
        internal_plugin_bridge = function()
            return {{
                create_resident = function() allocations = allocations + 1; return 1 end,
            }}
        end,
    }}
end
local bindings = dofile({})
local ok, err = pcall(function()
    bindings.register({{ host = function() return 1 end }}, bindings.providers({{
        factory_validation_provider_platform_Plugin = "not a factory",
    }}))
end)
assert(not ok)
assert(tostring(err):find("must be a factory function", 1, true))
assert(allocations == 0)
"#,
            lua_path_literal(&profile_path)
        ),
    )
    .expect("write factory-validation Lua E2E script");
    let result = Command::new("luajit")
        .arg(&script)
        .output()
        .expect("run generated factory-validation profile");
    assert!(
        result.status.success(),
        "bad factory must fail before native resident allocation\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );
}

#[test]
fn mixed_lua_layout_executes_inline_contracts_without_require_cache() {
    let temp = TempDir::new().expect("create mixed Lua layout fixture");
    let api = temp.path().join("mixed.lua.toml");
    let bundle = temp.path().join("mixed.lua.bundle.toml");
    write_api(&api, "platform.Plugin");
    write_bundle(&bundle, "mixed.lua.toml", "mixed_lua", "platform.Plugin");
    let bundle_contents = fs::read_to_string(&bundle).expect("read mixed Lua bundle");
    fs::write(
        &bundle,
        bundle_contents.replacen(
            "[bundle]\n",
            "[bundle]\nloader = \"lua\"\nfile = \"guest.lua\"\n",
            1,
        ),
    )
    .expect("add Lua loader fields");
    let domain_root = temp.path().join("canonical-domain");
    let output = generate(GenerateConfig {
        api_toml: bundle,
        lang: Lang::Lua,
        side: Side::Guest,
        layout: OutputLayout {
            bindings: OutputDestination::Inline,
            domain_types: OutputDestination::Emit {
                root: domain_root.clone(),
                import: ValidatedImport::parse(Lang::Lua, "canonical.domain")
                    .expect("valid Lua domain module"),
            },
            guest_contracts: OutputDestination::Inline,
        },
    })
    .expect("generate mixed Lua guest bindings");
    let domain = output
        .files
        .iter()
        .find(|file| file.partition == OutputPartition::DomainTypes)
        .expect("external Lua domain declaration");
    let contracts = output
        .files
        .iter()
        .find(|file| file.path == Path::new("guest").join("contracts.lua"))
        .expect("inline Lua contract declarations");
    let bindings = output
        .files
        .iter()
        .find(|file| file.path == Path::new("guest").join("bindings.lua"))
        .expect("Lua native registration bindings");
    assert!(contracts.content.contains("require(\"canonical.domain\")"));
    assert!(bindings.content.contains("require(\"canonical.domain\")"));
    assert!(
        bindings
            .content
            .contains("dofile(directory .. \"/contracts.lua\")")
    );
    assert!(!bindings.content.contains("require(\"guest.contracts\")"));

    let bindings_root = temp.path().join("bindings");
    write_output(&output, &bindings_root).expect("write mixed Lua output");
    let script = temp.path().join("mixed-layout-e2e.lua");
    fs::write(
        &script,
        format!(
            r#"
local ffi = require("ffi")
ffi.cdef[[
typedef struct {{ const char *ptr; size_t len; }} StringView;
typedef struct {{ void *ptr; size_t len; size_t capacity; }} Buffer;
]]
package.preload["canonical.domain"] = function() return dofile({}) end
package.preload["polyplug_guest"] = function()
    return {{ AbiErrorCode = {{ Ok = 0, Generic = 1 }} }}
end
local contracts = dofile({})
contracts.set_mixed_lua_provider_factory(function()
    return {{
        metadata = function() return {{}} end,
        read = function() return "" end,
        write = function() return "" end,
        take_inner = function() return 0 end,
    }}
end)
local registrations, err = polyplug_init(1, 1)
assert(err.code == 0)
assert(registrations["platform.Plugin"].factory ~= nil)
assert(package.loaded["guest.contracts"] == nil)
"#,
            lua_path_literal(&domain_root.join(&domain.path)),
            lua_path_literal(&bindings_root.join(&bindings.path)),
        ),
    )
    .expect("write mixed Lua E2E script");
    let result = Command::new("luajit")
        .arg(&script)
        .output()
        .expect("run mixed Lua guest E2E");
    assert!(
        result.status.success(),
        "mixed Lua layout must execute\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );
}

#[test]
fn inline_lua_domains_are_isolated_between_bundles() {
    let temp = TempDir::new().expect("create Lua bundle isolation fixture");
    let first_api = temp.path().join("first.lua.toml");
    let first_bundle = temp.path().join("first.lua.bundle.toml");
    let second_api = temp.path().join("second.lua.toml");
    let second_bundle = temp.path().join("second.lua.bundle.toml");
    write_api(&first_api, "first.Plugin");
    write_bundle(&first_bundle, "first.lua.toml", "first_lua", "first.Plugin");
    write_api(&second_api, "second.Plugin");
    write_bundle(
        &second_bundle,
        "second.lua.toml",
        "second_lua",
        "second.Plugin",
    );
    let first_root = temp.path().join("first");
    let second_root = temp.path().join("second");
    let first = generate_internal_lua(InternalLuaGenerateConfig {
        bundle_toml: first_bundle,
        out_dir: first_root.clone(),
        layout: OutputLayout {
            bindings: OutputDestination::Inline,
            domain_types: OutputDestination::Inline,
            guest_contracts: OutputDestination::ImportOnly {
                import: ValidatedImport::parse(Lang::Lua, "first.contracts")
                    .expect("valid first contracts module"),
            },
        },
    })
    .expect("generate first Lua bundle");
    let second = generate_internal_lua(InternalLuaGenerateConfig {
        bundle_toml: second_bundle,
        out_dir: second_root.clone(),
        layout: OutputLayout {
            bindings: OutputDestination::Inline,
            domain_types: OutputDestination::Inline,
            guest_contracts: OutputDestination::ImportOnly {
                import: ValidatedImport::parse(Lang::Lua, "second.contracts")
                    .expect("valid second contracts module"),
            },
        },
    })
    .expect("generate second Lua bundle");
    write_output(&first, &first_root).expect("write first Lua bundle");
    write_output(&second, &second_root).expect("write second Lua bundle");
    let first_domain = first_root.join(
        first
            .files
            .iter()
            .find(|file| file.partition == OutputPartition::DomainTypes)
            .expect("first inline domain")
            .path
            .clone(),
    );
    let second_domain = second_root.join(
        second
            .files
            .iter()
            .find(|file| file.partition == OutputPartition::DomainTypes)
            .expect("second inline domain")
            .path
            .clone(),
    );
    fs::write(
        &first_domain,
        "local ffi = require(\"ffi\")\npcall(ffi.cdef, [[typedef struct { uint64_t items; uint64_t len; } ArrayOf_Inner;]])\nreturn { bundle = 'first' }\n",
    )
    .expect("replace first domain fixture");
    fs::write(
        &second_domain,
        "local ffi = require(\"ffi\")\npcall(ffi.cdef, [[typedef struct { uint64_t items; uint64_t len; } ArrayOf_Inner;]])\nreturn { bundle = 'second' }\n",
    )
    .expect("replace second domain fixture");
    let first_profile = first_root.join(
        first
            .files
            .iter()
            .find(|file| file.path.ends_with(Path::new("guest").join("internal.lua")))
            .expect("first Lua profile")
            .path
            .clone(),
    );
    let second_profile = second_root.join(
        second
            .files
            .iter()
            .find(|file| file.path.ends_with(Path::new("guest").join("internal.lua")))
            .expect("second Lua profile")
            .path
            .clone(),
    );
    let script = temp.path().join("bundle-isolation-e2e.lua");
    fs::write(
        &script,
        format!(
            r#"
local ffi = require("ffi")
ffi.cdef[[
typedef struct {{ const char *ptr; size_t len; }} StringView;
typedef struct {{ void *ptr; size_t len; size_t capacity; }} Buffer;
typedef struct {{ void *data; uint64_t contract_id; }} GuestContractInstance;
typedef struct {{ uint32_t index; uint32_t generation; }} GuestContractHandle;
typedef struct {{ uint64_t contract_id; }} GuestContractInterface;
typedef struct {{ uint32_t code; void *message; }} AbiError;
]]
package.preload["polyplug.loaders.lua"] = function()
    return {{ internal_plugin_bridge = function() return {{}} end }}
end
package.preload["first.contracts"] = function()
    return {{ providers = function(values) return values end }}
end
package.preload["second.contracts"] = function()
    return {{ providers = function(values) return values end }}
end
local first = dofile({})
local second = dofile({})
assert(first ~= second)
assert(package.loaded["domain.types"] == nil)
"#,
            lua_path_literal(&first_profile),
            lua_path_literal(&second_profile),
        ),
    )
    .expect("write Lua bundle-isolation E2E script");
    let result = Command::new("luajit")
        .arg(&script)
        .output()
        .expect("run Lua bundle-isolation E2E");
    assert!(
        result.status.success(),
        "inline Lua domains must remain bundle-local\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );
}
