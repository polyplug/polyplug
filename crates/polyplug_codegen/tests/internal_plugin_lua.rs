#![allow(clippy::expect_used)]

use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use polyplug_codegen::{
    GenerateConfig, InternalLuaGenerateConfig, Lang, Side, generate, generate_internal_lua,
    write_output,
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

[[plugin_contract]]
name = "{contract}"
version = "1.0"

[[plugin_contract.functions]]
name = "metadata"
return = "Envelope"

[[plugin_contract.functions]]
name = "read"
params = [{{ name = "address", type = "u64" }}, {{ name = "size", type = "u32" }}]
return = "Buffer"

[[plugin_contract.functions]]
name = "write"
params = [{{ name = "label", type = "StringView" }}, {{ name = "mode", type = "Mode" }}, {{ name = "entries", type = "Array<Inner>" }}]
return = "StringView"

[[plugin_contract.functions]]
name = "take_inner"
params = [{{ name = "value", type = "Inner" }}]
return = "u32"
"#
        ),
    )
    .expect("write Lua API fixture");
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
        out_dir: temp.path().join("external"),
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
        out_dir: temp.path().join("external-host"),
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
