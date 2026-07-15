//! Focused output contracts for internal Python binding generation.

#![allow(clippy::expect_used)]

use std::collections::BTreeMap;
use std::collections::HashSet;
use std::env::consts::OS;
use std::env::join_paths;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use polyplug_utils::bundle_id;

use crate::GenerateConfig;
use crate::GenerateOutput;
use crate::InternalPythonGenerateConfig;
use crate::Lang;
use crate::OutputDestination;
use crate::OutputLayout;
use crate::Side;
use crate::ValidatedImport;
use crate::generate;
use crate::generate_internal_python;
use crate::write_output;

fn output_map(output: GenerateOutput) -> BTreeMap<PathBuf, String> {
    output
        .files
        .into_iter()
        .map(|file| (file.path, file.content))
        .collect()
}

fn native_library_name(stem: &str) -> String {
    match OS {
        "windows" => format!("{stem}.dll"),
        "macos" => format!("lib{stem}.dylib"),
        _ => format!("lib{stem}.so"),
    }
}

fn write_api(path: &Path) {
    fs::write(
        path,
        "[[enum]]\nname = \"Mode\"\nrepr = \"u32\"\n\n[[enum.variants]]\nname = \"Fast\"\nvalue = \"1\"\n\n[[types]]\nname = \"Inner\"\nfields = [{ name = \"name\", type = \"StringView\" }]\n\n[[types]]\nname = \"Outer\"\nfields = [{ name = \"inner\", type = \"Inner\" }, { name = \"payload\", type = \"Buffer\" }]\n\n[[guest_contract]]\nname = \"python.profile\"\nversion = \"1.0\"\n\n[[guest_contract.functions]]\nname = \"scalar\"\nparams = [{ name = \"value\", type = \"u32\" }]\nreturn = \"u32\"\n\n[[guest_contract.functions]]\nname = \"text\"\nparams = [{ name = \"value\", type = \"StringView\" }]\nreturn = \"StringView\"\n\n[[guest_contract.functions]]\nname = \"many\"\nparams = [{ name = \"mode\", type = \"Mode\" }, { name = \"item\", type = \"Outer\" }]\nreturn = \"Outer\"\n\n[[guest_contract.functions]]\nname = \"buffer\"\nparams = [{ name = \"value\", type = \"Buffer\" }]\nreturn = \"Buffer\"\n",
    )
    .expect("write API TOML");
    let mut api = fs::OpenOptions::new()
        .append(true)
        .open(path)
        .expect("open API TOML for array contract");
    api.write_all(
        b"\n[[guest_contract.functions]]\nname = \"array_roundtrip\"\nparams = [{ name = \"items\", type = \"Array<Inner>\" }]\nreturn = \"Array<Inner>\"\n",
    )
    .expect("append array contract");
}

fn write_internal_bundle(path: &Path, name: &str, api: &str, plugin: &str) {
    fs::write(
        path,
        format!(
            "[bundle]\nname = \"{name}\"\nversion = \"1.0\"\napi = \"{api}\"\n\n[[plugin]]\nname = \"{plugin}\"\nimplements = [\"python.profile@1.0\"]\n"
        ),
    )
    .expect("write internal bundle TOML");
}

#[test]
fn internal_plugin_python_profile_is_namespaced_typed_and_consumed_on_attempt() {
    let temp = tempfile::tempdir().expect("create temporary directory");
    let api = temp.path().join("api.toml");
    let bundle = temp.path().join("bundle.toml");
    write_api(&api);
    write_internal_bundle(&bundle, "python_internal", "api.toml", "python_provider");

    let output = output_map(
        generate_internal_python(InternalPythonGenerateConfig {
            bundle_toml: bundle,
            out_dir: temp.path().join("out"),
            layout: Default::default(),
        })
        .expect("generate internal Python profile"),
    );
    let namespace = Path::new("internal").join(format!(
        "python_internal-{:016x}",
        bundle_id("python_internal")
    ));
    assert!(
        output.keys().all(|path| path.starts_with(&namespace)),
        "every internal Python file must be namespaced: {output:#?}"
    );

    let facade = output
        .get(&namespace.join("internal.py"))
        .expect("internal façade");
    let contracts = output
        .get(&namespace.join("guest").join("contracts.py"))
        .expect("guest provider bindings");
    let callers = output
        .get(&namespace.join("host").join("callers.py"))
        .expect("host caller bindings");

    assert!(facade.contains("class InternalPluginProviders:"));
    assert!(facade.contains("class InternalPluginRegistration:"));
    assert!(facade.contains("def register(runtime: Any, providers: InternalPluginProviders)"));
    assert!(facade.contains("internal-plugin provider input has already been consumed"));
    assert!(facade.contains("runtime.register_generated_internal_plugin"));
    assert!(facade.contains("python_provider_python_profile"));
    assert!(facade.contains("bundle = InternalPlugin"));
    assert!(facade.contains("runtime.create_generated_internal_plugin_caller"));
    assert!(!facade.contains(".create(runtime"));
    assert!(!facade.contains("GuestContractInterface"));
    assert!(!facade.contains("PluginDescriptor"));
    assert!(!facade.contains("adapter_context"));
    assert!(!facade.contains("manifest.file"));

    assert!(contracts.contains("def scalar("));
    assert!(contracts.contains("to_str(StringView.from_address(args_ptr))"));
    assert!(contracts.contains("item = args_val.item"));
    assert!(contracts.contains("Buffer.from_address(args_ptr)"));
    assert!(contracts.contains("args_val: PythonProfileContractManyArgs"));
    assert!(callers.contains("class PythonProfileContractCaller:"));
    assert!(callers.contains("if interface == self._interface:"));
    assert!(callers.contains("self._cached_revision = self._live_revision()"));
    assert!(output.contains_key(&namespace.join("__init__.py")));
    assert!(output.contains_key(&namespace.join("host").join("__init__.py")));
    assert!(output.contains_key(&namespace.join("guest").join("__init__.py")));
}

#[test]
fn default_and_external_python_generation_do_not_emit_internal_artifacts() {
    let temp = tempfile::tempdir().expect("create temporary directory");
    let api = temp.path().join("api.toml");
    let bundle = temp.path().join("external.toml");
    write_api(&api);
    fs::write(
        &bundle,
        "[bundle]\nname = \"python_external\"\nversion = \"1.0\"\nloader = \"python\"\nfile = \"plugin.py\"\napi = \"api.toml\"\n\n[[plugin]]\nname = \"python_provider\"\nimplements = [\"python.profile@1.0\"]\n",
    )
    .expect("write external bundle TOML");

    let default = output_map(
        generate(GenerateConfig {
            api_toml: api,
            lang: Lang::Python,
            side: Side::Host,
            layout: OutputLayout::unified(),
        })
        .expect("generate default Python host bindings"),
    );
    let external = output_map(
        generate(GenerateConfig {
            api_toml: bundle,
            lang: Lang::Python,
            side: Side::Guest,
            layout: OutputLayout::unified(),
        })
        .expect("generate external Python guest bindings"),
    );
    let default_paths: Vec<PathBuf> = default.keys().cloned().collect();
    assert_eq!(
        default_paths,
        vec![
            Path::new("host").join("callers.py"),
            Path::new("host").join("callers.pyi"),
            Path::new("host").join("types.py"),
            Path::new("host").join("types.pyi"),
        ],
        "default generation must retain exactly the canonical host caller bindings"
    );
    let external_paths: Vec<PathBuf> = external.keys().cloned().collect();
    assert_eq!(
        external_paths,
        vec![
            Path::new("guest").join("contracts.py"),
            Path::new("guest").join("contracts.pyi"),
            Path::new("guest").join("init.py"),
            Path::new("guest").join("types.py"),
            Path::new("guest").join("types.pyi"),
            PathBuf::from("manifest.toml"),
        ],
        "external generation must retain exactly the canonical guest provider bindings"
    );
    for output in [&default, &external] {
        assert!(
            output
                .values()
                .all(|content| !content.contains("register_generated_internal_plugin")),
            "default/external generation must not depend on the internal-plugin profile"
        );
    }
}

#[test]
fn distinct_python_internal_plugin_bundles_emit_collision_free_packages() {
    let temp = tempfile::tempdir().expect("create temporary directory");
    let first_api = temp.path().join("first.toml");
    let second_api = temp.path().join("second.toml");
    let first_bundle = temp.path().join("first-bundle.toml");
    let second_bundle = temp.path().join("second-bundle.toml");
    write_api(&first_api);
    write_api(&second_api);
    write_internal_bundle(
        &first_bundle,
        "python_first",
        "first.toml",
        "first_provider",
    );
    write_internal_bundle(
        &second_bundle,
        "python_second",
        "second.toml",
        "second_provider",
    );

    let first = generate_internal_python(InternalPythonGenerateConfig {
        bundle_toml: first_bundle,
        out_dir: temp.path().join("out"),
        layout: Default::default(),
    })
    .expect("generate first internal bundle");
    let second = generate_internal_python(InternalPythonGenerateConfig {
        bundle_toml: second_bundle,
        out_dir: temp.path().join("out"),
        layout: Default::default(),
    })
    .expect("generate second internal bundle");
    let mut paths: HashSet<PathBuf> = HashSet::new();
    for file in first.files.iter().chain(second.files.iter()) {
        assert!(
            paths.insert(file.path.clone()),
            "two Python internal bundles emitted colliding path `{}`",
            file.path.display()
        );
    }
    let first_namespace =
        Path::new("internal").join(format!("python_first-{:016x}", bundle_id("python_first")));
    let second_namespace =
        Path::new("internal").join(format!("python_second-{:016x}", bundle_id("python_second")));
    assert!(paths.iter().any(|path| path.starts_with(&first_namespace)));
    assert!(paths.iter().any(|path| path.starts_with(&second_namespace)));
}

#[test]
fn generated_python_internal_plugin_registration_imports_and_consumes_real_provider_input() {
    let temp = tempfile::tempdir().expect("create temporary directory");
    let api = temp.path().join("api.toml");
    let bundle = temp.path().join("bundle.toml");
    let out = temp.path().join("out");
    write_api(&api);
    fs::write(
    &bundle,
    "[bundle]\nname = \"python_execute\"\nversion = \"1.0\"\napi = \"api.toml\"\n\n[[plugin]]\nname = \"first_provider\"\nimplements = [\"python.profile@1.0\"]\n\n[[plugin]]\nname = \"second_provider\"\nimplements = [\"python.profile@1.0\"]\n",
)
.expect("write two-provider bundle");
    let output = generate_internal_python(InternalPythonGenerateConfig {
        bundle_toml: bundle,
        out_dir: out.clone(),
        layout: Default::default(),
    })
    .expect("generate internal Python profile");
    let callers = output
        .files
        .iter()
        .find(|file| file.path.ends_with(Path::new("host").join("callers.py")))
        .expect("generated profile callers");
    let buffer_start = callers
        .content
        .find("    def buffer(")
        .expect("generated Buffer caller");
    let buffer_end = callers.content[buffer_start + "    def buffer(".len()..]
        .find("\n    def ")
        .map_or(callers.content.len(), |offset| {
            buffer_start + "    def buffer(".len() + offset
        });
    let buffer_method = &callers.content[buffer_start..buffer_end];
    assert!(
        buffer_method.contains("return ctypes.string_at(out_val.ptr, out_val.len)")
            && !buffer_method.contains("return out_val"),
        "internal Python Buffer caller must decode ABI Buffer results:\n{buffer_method}"
    );
    write_output(&output, &out).expect("write internal Python profile");
    let namespace = format!("python_execute-{:016x}", bundle_id("python_execute"));
    let package_root = out.join("internal").join(&namespace);
    let script = temp.path().join("verify.py");
    fs::write(
    &script,
r#"import importlib.util
import sys
from pathlib import Path

root = Path(sys.argv[1])
name = "generated_python_execute"
spec = importlib.util.spec_from_file_location(
    name,
    root / "__init__.py",
    submodule_search_locations=[str(root)],
)
assert spec is not None and spec.loader is not None
package = importlib.util.module_from_spec(spec)
sys.modules[name] = package
spec.loader.exec_module(package)
internal = importlib.import_module(name + ".internal")


from polyplug import Runtime

class FirstImplementation:
    def __init__(self):
        self.calls = 0
    def scalar(self, value):
        if value == 999:
            raise RuntimeError("expected provider error")
        self.calls += 1
        return self.calls
    def text(self, value):
        return value
    def many(self, mode, item):
        inner = type("InnerResult", (), {"name": "nested"})()
        return type("OuterResult", (), {"inner": inner, "payload": item.payload})()
    def buffer(self, value):
        return value
    def array_roundtrip(self, items):
        return [
            type("ArrayInner", (), {"name": "first"})(),
            type("ArrayInner", (), {"name": "second"})(),
        ]

class SecondImplementation(FirstImplementation):
    pass

runtime = Runtime()
from polyplug_abi import HostApi
assert HostApi.from_address(runtime._host).runtime % 8 == 0
from polyplug_abi import GuestContractInstance
providers = internal.InternalPluginProviders(FirstImplementation, SecondImplementation)
bundle = providers._consume()
bundle_id, handles = runtime.register_generated_internal_plugin(
    internal.INTERNAL_PLUGIN_MANIFEST, bundle
)
assert runtime._host_struct.registry_revision(runtime._host) >= 0
interface = runtime._host_struct.resolve_guest_contract(runtime._host, handles[0])
instance = GuestContractInstance()
runtime._host_struct.create_guest_instance(
    runtime._host, interface, None, __import__("ctypes").byref(instance)
)
assert runtime._host_struct.registry_revision(runtime._host) >= 0
runtime._host_struct.destroy_guest_instance(runtime._host, interface, instance)
first = runtime.create_generated_internal_plugin_caller(
    internal.PythonProfileContractCaller, handles[0]
)
second = runtime.create_generated_internal_plugin_caller(
    internal.PythonProfileContractCaller, handles[1]
)
registration = internal.InternalPluginRegistration(bundle_id, first, second)
second = registration.second_provider_python_profile
assert first._host.value == runtime._host
callers = importlib.import_module(name + ".host.callers")
assert first._live_revision() >= 0
assert callers.HostApi.__module__ == HostApi.__module__
assert __import__("ctypes").sizeof(callers.HostApi) == 184
assert callers.HostApi.registry_revision.offset == 168
assert callers.HostApi is type(runtime._host_struct)
assert first._live_revision() >= 0
assert first.scalar(1) == 1
assert first.scalar(1) == 2
assert second.scalar(1) == 1
assert first.text("text") == "text"
buffer_result = first.buffer(b"bytes")
assert buffer_result == b"bytes"
for _ in range(8):
    assert first.buffer(b"bytes") == b"bytes"
assert len(runtime._internal_plugin_residents[bundle_id]._adapters[0]._buffers[first._instance.data]) <= 1
from generated_python_execute.host.types import Inner, Mode, Outer
nested_text = __import__("ctypes").create_string_buffer(b"nested")
nested_payload = __import__("ctypes").create_string_buffer(b"payload")
nested_inner = Inner()
nested_inner.name = __import__("polyplug_abi").StringView(
    __import__("ctypes").cast(nested_text, __import__("ctypes").c_void_p), 6
)
nested_outer = Outer()
nested_outer.inner = nested_inner
nested_outer.payload = __import__("polyplug_abi").Buffer(
    __import__("ctypes").cast(nested_payload, __import__("ctypes").c_void_p), 7, 7
)
assert first.many(Mode.FAST, nested_outer).inner.name.len == 6
from generated_python_execute.host.types import ArrayOf_Inner
array_result = first.array_roundtrip(ArrayOf_Inner())
assert array_result.len == 2
array_items = __import__("ctypes").cast(array_result.items, __import__("ctypes").POINTER(Inner))
assert __import__("polyplug_abi").to_str(array_items[0].name) == "first"
assert __import__("polyplug_abi").to_str(array_items[1].name) == "second"
try:
    first.scalar(999)
except Exception:
    pass
else:
    raise AssertionError("generated caller must surface provider errors")
try:
    internal.register(runtime, providers)
except RuntimeError:
    pass
else:
    raise AssertionError("provider input must be consumed after its first registration attempt")
import gc
del first, second, registration
gc.collect()
runtime.unload_bundle(bundle_id)
"#,
)
.expect("write executable verification script");
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates directory")
        .parent()
        .expect("workspace directory");
    let python_path = join_paths([
        workspace.join("sdks").join("python"),
        workspace.join("sdks").join("python").join("host"),
        workspace.join("sdks").join("python").join("polyplug_abi"),
        workspace.join("sdks").join("python").join("guest"),
    ])
    .expect("join Python import paths");
    let library = workspace
        .join("target")
        .join("debug")
        .join(native_library_name("polyplug"));
    assert!(
        library.is_file(),
        "build libpolyplug before running Python profile E2E"
    );
    let result = Command::new("python3")
        .arg(&script)
        .arg(package_root)
        .env("PYTHONPATH", python_path)
        .env("POLYPLUG_LIB", library)
        .output()
        .expect("run generated Python registration");
    assert!(
        result.status.success(),
        "generated registration failed:\n{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn split_python_internal_profile_uses_external_canonical_domain_and_contract_packages() {
    let temp = tempfile::tempdir().expect("create temporary directory");
    let api = temp.path().join("api.toml");
    let bundle = temp.path().join("bundle.toml");
    let bindings_root = temp.path().join("bindings");
    let domain_root = temp.path().join("domain");
    let contracts_root = temp.path().join("contracts");
    write_api(&api);
    write_internal_bundle(&bundle, "python_split", "api.toml", "python_provider");
    let layout = OutputLayout {
        bindings: OutputDestination::Inline,
        domain_types: OutputDestination::Emit {
            root: domain_root.clone(),
            import: ValidatedImport::parse(Lang::Python, "domain").expect("valid domain import"),
        },
        guest_contracts: OutputDestination::Emit {
            root: contracts_root.clone(),
            import: ValidatedImport::parse(Lang::Python, "guest_contracts")
                .expect("valid guest-contract import"),
        },
    };
    let output = generate_internal_python(InternalPythonGenerateConfig {
        bundle_toml: bundle,
        out_dir: bindings_root.clone(),
        layout,
    })
    .expect("generate split internal Python profile");
    assert!(
        output.files.iter().any(|file| {
            file.path.ends_with(Path::new("domain.py"))
                && file.partition == crate::OutputPartition::DomainTypes
        }),
        "split output must emit canonical domain values"
    );
    assert!(
        output.files.iter().any(|file| {
            file.path.ends_with(Path::new("guest_contracts.py"))
                && file.partition == crate::OutputPartition::GuestContracts
        }),
        "split output must emit guest contract declarations"
    );
    let declarations = output
        .files
        .iter()
        .find(|file| file.path.ends_with(Path::new("guest_contracts.py")))
        .expect("guest contract declarations");
    assert!(
        declarations
            .content
            .contains("class PYTHON_PROVIDERPythonProfilePlugin(Protocol):"),
        "split declarations must expose a structural provider Protocol"
    );
    let runtime_contracts = output
        .files
        .iter()
        .find(|file| file.path.ends_with(Path::new("guest/contracts.py")))
        .expect("guest runtime bindings");
    assert!(
        runtime_contracts
            .content
            .contains("import guest_contracts as _guest_contracts")
    );
    assert!(
        !runtime_contracts
            .content
            .contains("class PythonProviderPythonProfilePlugin:")
    );
    write_output(&output, &bindings_root).expect("write split Python profile");

    let namespace = format!("python_split-{:016x}", bundle_id("python_split"));
    let package_root = bindings_root.join("internal").join(&namespace);
    let script = temp.path().join("verify_split.py");
    fs::write(
        &script,
        r#"import ctypes
import gc
import importlib
import importlib.util
import sys
from pathlib import Path

root = Path(sys.argv[1])
name = "generated_python_split"
spec = importlib.util.spec_from_file_location(
    name, root / "__init__.py", submodule_search_locations=[str(root)]
)
assert spec is not None and spec.loader is not None
package = importlib.util.module_from_spec(spec)
sys.modules[name] = package
spec.loader.exec_module(package)
internal = importlib.import_module(name + ".internal")

import domain
import guest_contracts as contracts
host_types = importlib.import_module(name + ".host.types")
guest_types = importlib.import_module(name + ".guest.types")
assert host_types.Inner is domain.Inner
assert guest_types.Outer is domain.Outer
assert internal.PYTHON_PROVIDERPythonProfilePlugin is contracts.PYTHON_PROVIDERPythonProfilePlugin

class Implementation:
    def __init__(self):
        self.calls = 0
    def scalar(self, value):
        self.calls += 1
        return self.calls
    def text(self, value):
        return value
    def many(self, mode, item):
        _ = mode
        nested = type("Nested", (), {"name": "stateful"})()
        return type("Result", (), {"inner": nested, "payload": item.payload})()
    def buffer(self, value):
        return value
    def array_roundtrip(self, items):
        return [
            type("First", (), {"name": "one"})(),
            type("Second", (), {"name": "two"})(),
        ]

from polyplug import Runtime
from polyplug_abi import Buffer, StringView, to_str
runtime = Runtime()
registration = internal.register(runtime, internal.InternalPluginProviders(Implementation))
caller = registration.python_provider_python_profile
assert caller.scalar(7) == 1
assert caller.scalar(7) == 2
text = ctypes.create_string_buffer(b"nested")
payload = ctypes.create_string_buffer(b"payload")
inner = domain.Inner()
inner.name = StringView(ctypes.cast(text, ctypes.c_void_p), 6)
outer = domain.Outer()
outer.inner = inner
outer.payload = Buffer(ctypes.cast(payload, ctypes.c_void_p), 7, 7)
result = caller.many(domain.Mode.FAST, outer)
assert to_str(result.inner.name) == "stateful"
array_result = caller.array_roundtrip(domain.ArrayOf_Inner())
array_items = ctypes.cast(array_result.items, ctypes.POINTER(domain.Inner))
assert array_result.len == 2
assert [to_str(array_items[index].name) for index in range(2)] == ["one", "two"]
bundle_id = registration.bundle_id
del caller, registration
gc.collect()
runtime.unload_bundle(bundle_id)
"#,
    )
    .expect("write split verification script");
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates directory")
        .parent()
        .expect("workspace directory");
    let python_path = join_paths([
        domain_root.join("internal").join(&namespace),
        contracts_root.join("internal").join(&namespace),
        workspace.join("sdks").join("python"),
        workspace.join("sdks").join("python").join("host"),
        workspace.join("sdks").join("python").join("polyplug_abi"),
        workspace.join("sdks").join("python").join("guest"),
    ])
    .expect("join Python import paths");
    let library = workspace
        .join("target")
        .join("debug")
        .join(native_library_name("polyplug"));
    assert!(
        library.is_file(),
        "build libpolyplug before running Python split E2E"
    );
    let result = Command::new("python3")
        .arg(&script)
        .arg(package_root)
        .env("PYTHONPATH", python_path)
        .env("POLYPLUG_LIB", library)
        .output()
        .expect("run generated split Python profile");
    assert!(
        result.status.success(),
        "generated split profile failed:\n{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn split_python_ordinary_host_and_guest_bindings_import_external_packages() {
    let temp = tempfile::tempdir().expect("create temporary directory");
    let api = temp.path().join("api.toml");
    let bundle = temp.path().join("bundle.toml");
    let host_root = temp.path().join("host");
    let guest_root = temp.path().join("guest");
    let domain_root = temp.path().join("domain");
    let contracts_root = temp.path().join("contracts");
    write_api(&api);
    fs::write(
        &bundle,
        "[bundle]\nname = \"python_external_split\"\nversion = \"1.0\"\nloader = \"python\"\nfile = \"plugin.py\"\napi = \"api.toml\"\n\n[[plugin]]\nname = \"python_provider\"\nimplements = [\"python.profile@1.0\"]\n",
    )
    .expect("write external Python bundle");
    let guest_layout = OutputLayout {
        bindings: OutputDestination::Inline,
        domain_types: OutputDestination::Emit {
            root: domain_root.clone(),
            import: ValidatedImport::parse(Lang::Python, "domain").expect("valid domain import"),
        },
        guest_contracts: OutputDestination::Emit {
            root: contracts_root.clone(),
            import: ValidatedImport::parse(Lang::Python, "guest_contracts")
                .expect("valid guest-contract import"),
        },
    };
    let host_layout = OutputLayout {
        bindings: OutputDestination::Inline,
        domain_types: OutputDestination::ImportOnly {
            import: ValidatedImport::parse(Lang::Python, "domain").expect("valid domain import"),
        },
        guest_contracts: OutputDestination::ImportOnly {
            import: ValidatedImport::parse(Lang::Python, "guest_contracts")
                .expect("valid guest-contract import"),
        },
    };
    let guest = generate(GenerateConfig {
        api_toml: bundle,
        lang: Lang::Python,
        side: Side::Guest,
        layout: guest_layout,
    })
    .expect("generate split guest bindings");
    let host = generate(GenerateConfig {
        api_toml: api,
        lang: Lang::Python,
        side: Side::Host,
        layout: host_layout,
    })
    .expect("generate host bindings importing split domain package");
    write_output(&guest, &guest_root).expect("write split guest bindings");
    write_output(&host, &host_root).expect("write split host bindings");
    let script = temp.path().join("verify_ordinary_split.py");
    fs::write(
        &script,
        r#"import guest.contracts as guest_bindings
import guest.types as guest_types
import guest_contracts
import host.types as host_types
import domain

assert host_types.Inner is domain.Inner
assert guest_types.Outer is domain.Outer
assert guest_bindings.PYTHON_PROVIDERPythonProfilePlugin is guest_contracts.PYTHON_PROVIDERPythonProfilePlugin
assert guest_bindings.polyplug_abi_version() == 2
"#,
    )
    .expect("write ordinary split verification script");
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates directory")
        .parent()
        .expect("workspace directory");
    let python_path = join_paths([
        host_root,
        guest_root,
        domain_root,
        contracts_root,
        workspace.join("sdks").join("python"),
        workspace.join("sdks").join("python").join("host"),
        workspace.join("sdks").join("python").join("polyplug_abi"),
        workspace.join("sdks").join("python").join("guest"),
    ])
    .expect("join Python import paths");
    let result = Command::new("python3")
        .arg(&script)
        .env("PYTHONPATH", python_path)
        .output()
        .expect("run ordinary split package verification");
    assert!(
        result.status.success(),
        "ordinary split imports failed:\n{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn split_python_guest_contract_omit_keeps_required_runtime_declarations_local() {
    let temp = tempfile::tempdir().expect("create temporary directory");
    let api = temp.path().join("api.toml");
    let bundle = temp.path().join("bundle.toml");
    write_api(&api);
    fs::write(
        &bundle,
        "[bundle]\nname = \"python_omit\"\nversion = \"1.0\"\nloader = \"python\"\nfile = \"plugin.py\"\napi = \"api.toml\"\n\n[[plugin]]\nname = \"python_provider\"\nimplements = [\"python.profile@1.0\"]\n",
    )
    .expect("write external Python bundle");
    let output = generate(GenerateConfig {
        api_toml: bundle,
        lang: Lang::Python,
        side: Side::Guest,
        layout: OutputLayout {
            bindings: OutputDestination::Inline,
            domain_types: OutputDestination::Emit {
                root: temp.path().join("domain"),
                import: ValidatedImport::parse(Lang::Python, "domain")
                    .expect("valid domain import"),
            },
            guest_contracts: OutputDestination::Omit,
        },
    })
    .expect("generate guest bindings without a guest-contract package");
    assert!(
        output
            .files
            .iter()
            .all(|file| file.partition != crate::OutputPartition::GuestContracts),
        "omitted guest declarations must not produce a separate package"
    );
    let runtime_contracts = output
        .files
        .iter()
        .find(|file| file.path == Path::new("guest/contracts.py"))
        .expect("guest runtime bindings");
    assert!(
        runtime_contracts
            .content
            .contains("class PYTHON_PROVIDERPythonProfilePlugin:"),
        "the bindings must remain executable when declaration output is omitted"
    );
}

#[test]
fn mixed_python_internal_layout_imports_inline_guest_contracts_from_its_package() {
    let temp = tempfile::tempdir().expect("create mixed Python layout fixture");
    let api = temp.path().join("api.toml");
    let bundle = temp.path().join("bundle.toml");
    let bindings_root = temp.path().join("bindings");
    let domain_root = temp.path().join("domain");
    write_api(&api);
    write_internal_bundle(&bundle, "python_mixed", "api.toml", "python_provider");
    let output = generate_internal_python(InternalPythonGenerateConfig {
        bundle_toml: bundle,
        out_dir: bindings_root.clone(),
        layout: OutputLayout {
            bindings: OutputDestination::Inline,
            domain_types: OutputDestination::Emit {
                root: domain_root.clone(),
                import: ValidatedImport::parse(Lang::Python, "domain")
                    .expect("valid external domain import"),
            },
            guest_contracts: OutputDestination::Inline,
        },
    })
    .expect("generate mixed internal Python profile");
    let runtime_contracts = output
        .files
        .iter()
        .find(|file| file.path.ends_with(Path::new("guest").join("contracts.py")))
        .expect("mixed guest runtime contracts");
    assert!(
        runtime_contracts
            .content
            .contains("from ..guest_contracts import "),
        "nested guest bindings must import inline declarations from their package"
    );
    let internal = output
        .files
        .iter()
        .find(|file| file.path.ends_with(Path::new("internal.py")))
        .expect("mixed internal facade");
    assert!(
        internal.content.contains("from .guest_contracts import "),
        "internal facade must import inline declarations from its package"
    );
    write_output(&output, &bindings_root).expect("write mixed Python profile");

    let namespace = format!("python_mixed-{:016x}", bundle_id("python_mixed"));
    let package_root = bindings_root.join("internal").join(&namespace);
    let script = temp.path().join("mixed-layout-e2e.py");
    fs::write(
        &script,
        r#"import importlib
import importlib.util
import sys
from pathlib import Path

root = Path(sys.argv[1])
name = "generated_python_mixed"
spec = importlib.util.spec_from_file_location(
    name, root / "__init__.py", submodule_search_locations=[str(root)]
)
assert spec is not None and spec.loader is not None
package = importlib.util.module_from_spec(spec)
sys.modules[name] = package
spec.loader.exec_module(package)

import domain
internal = importlib.import_module(name + ".internal")
host_types = importlib.import_module(name + ".host.types")
guest_contracts = importlib.import_module(name + ".guest_contracts")
runtime_contracts = importlib.import_module(name + ".guest.contracts")
assert host_types.Inner is domain.Inner
assert runtime_contracts.PYTHON_PROVIDERPythonProfilePlugin is guest_contracts.PYTHON_PROVIDERPythonProfilePlugin
assert internal.PYTHON_PROVIDERPythonProfilePlugin is guest_contracts.PYTHON_PROVIDERPythonProfilePlugin
"#,
    )
    .expect("write mixed Python E2E script");
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates directory")
        .parent()
        .expect("workspace directory");
    let python_path = join_paths([
        domain_root.join("internal").join(&namespace),
        workspace.join("sdks").join("python"),
        workspace.join("sdks").join("python").join("host"),
        workspace.join("sdks").join("python").join("polyplug_abi"),
        workspace.join("sdks").join("python").join("guest"),
    ])
    .expect("join Python import paths");
    let result = Command::new("python3")
        .arg(&script)
        .arg(package_root)
        .env("PYTHONPATH", python_path)
        .output()
        .expect("run mixed Python import E2E");
    assert!(
        result.status.success(),
        "mixed Python layout must execute\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );
}

#[test]
fn mixed_python_ordinary_layout_imports_inline_contracts_from_the_bindings_root() {
    let temp = tempfile::tempdir().expect("create ordinary mixed Python layout fixture");
    let api = temp.path().join("api.toml");
    let bundle = temp.path().join("bundle.toml");
    let guest_root = temp.path().join("guest");
    let host_root = temp.path().join("host");
    let domain_root = temp.path().join("domain");
    write_api(&api);
    fs::write(
        &bundle,
        "[bundle]\nname = \"python_ordinary_mixed\"\nversion = \"1.0\"\nloader = \"python\"\nfile = \"plugin.py\"\napi = \"api.toml\"\n\n[[plugin]]\nname = \"python_provider\"\nimplements = [\"python.profile@1.0\"]\n",
    )
    .expect("write ordinary Python bundle");
    let guest = generate(GenerateConfig {
        api_toml: bundle,
        lang: Lang::Python,
        side: Side::Guest,
        layout: OutputLayout {
            bindings: OutputDestination::Inline,
            domain_types: OutputDestination::Emit {
                root: domain_root.clone(),
                import: ValidatedImport::parse(Lang::Python, "domain")
                    .expect("valid external domain import"),
            },
            guest_contracts: OutputDestination::Inline,
        },
    })
    .expect("generate ordinary mixed Python guest bindings");
    let host = generate(GenerateConfig {
        api_toml: api,
        lang: Lang::Python,
        side: Side::Host,
        layout: OutputLayout {
            bindings: OutputDestination::Inline,
            domain_types: OutputDestination::ImportOnly {
                import: ValidatedImport::parse(Lang::Python, "domain")
                    .expect("valid external domain import"),
            },
            guest_contracts: OutputDestination::Omit,
        },
    })
    .expect("generate ordinary Python host bindings");
    let runtime_contracts = guest
        .files
        .iter()
        .find(|file| file.path == Path::new("guest").join("contracts.py"))
        .expect("ordinary guest runtime contracts");
    assert!(
        runtime_contracts
            .content
            .contains("from guest_contracts import "),
        "ordinary guest bindings must import inline declarations from their root"
    );
    write_output(&guest, &guest_root).expect("write ordinary mixed Python guest bindings");
    write_output(&host, &host_root).expect("write ordinary Python host bindings");
    let script = temp.path().join("ordinary-mixed-layout-e2e.py");
    fs::write(
        &script,
        r#"import guest.contracts as runtime_contracts
import guest.types as guest_types
import guest_contracts
import host.types as host_types
import domain

assert host_types.Inner is domain.Inner
assert guest_types.Outer is domain.Outer
assert runtime_contracts.PYTHON_PROVIDERPythonProfilePlugin is guest_contracts.PYTHON_PROVIDERPythonProfilePlugin
"#,
    )
    .expect("write ordinary mixed Python E2E script");
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates directory")
        .parent()
        .expect("workspace directory");
    let python_path = join_paths([
        host_root,
        guest_root,
        domain_root,
        workspace.join("sdks").join("python"),
        workspace.join("sdks").join("python").join("host"),
        workspace.join("sdks").join("python").join("polyplug_abi"),
        workspace.join("sdks").join("python").join("guest"),
    ])
    .expect("join Python import paths");
    let result = Command::new("python3")
        .arg(&script)
        .env("PYTHONPATH", python_path)
        .output()
        .expect("run ordinary mixed Python E2E");
    assert!(
        result.status.success(),
        "ordinary mixed Python layout must execute\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );
}

#[test]
fn internal_python_root_rejects_stale_host_callers_fingerprint() {
    let temp = tempfile::tempdir().expect("create Python stale-binding fixture");
    let api = temp.path().join("api.toml");
    let bundle = temp.path().join("bundle.toml");
    let out = temp.path().join("out");
    write_api(&api);
    write_internal_bundle(
        &bundle,
        "python_stale_callers",
        "api.toml",
        "python_provider",
    );
    let output = generate_internal_python(InternalPythonGenerateConfig {
        bundle_toml: bundle,
        out_dir: out.clone(),
        layout: OutputLayout::unified(),
    })
    .expect("generate Python profile");
    write_output(&output, &out).expect("write Python profile");
    let package = out.join("internal").join(format!(
        "python_stale_callers-{:016x}",
        bundle_id("python_stale_callers")
    ));
    let callers = package.join("host/callers.py");
    let source = fs::read_to_string(&callers).expect("read generated callers");
    fs::write(
        &callers,
        source.replacen(
            "_polyplug_internal_generation_fingerprint = 0x",
            "_polyplug_internal_generation_fingerprint = 0x0 #",
            1,
        ),
    )
    .expect("write stale callers");
    let script = temp.path().join("import.py");
    fs::write(
        &script,
        "import importlib.util, sys\nfrom pathlib import Path\nroot = Path(sys.argv[1])\nname = 'stale_python'\nspec = importlib.util.spec_from_file_location(name, root / '__init__.py', submodule_search_locations=[str(root)])\npkg = importlib.util.module_from_spec(spec)\nsys.modules[name] = pkg\nspec.loader.exec_module(pkg)\nimportlib.import_module(name + '.internal')\n",
    )
    .expect("write Python import script");
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates directory")
        .parent()
        .expect("workspace directory");
    let python_path = join_paths([
        workspace.join("sdks").join("python"),
        workspace.join("sdks").join("python").join("host"),
        workspace.join("sdks").join("python").join("polyplug_abi"),
        workspace.join("sdks").join("python").join("guest"),
    ])
    .expect("join Python import paths");
    let result = Command::new("python3")
        .arg(&script)
        .arg(&package)
        .env("PYTHONPATH", python_path)
        .output()
        .expect("import stale Python profile");
    assert!(
        !result.status.success()
            && String::from_utf8_lossy(&result.stderr)
                .contains("generated internal partitions are incompatible"),
        "stale generated callers must fail Python root import:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );
}

#[test]
fn internal_python_root_rejects_stale_host_types_fingerprint() {
    let temp = tempfile::tempdir().expect("create Python stale-types fixture");
    let api = temp.path().join("api.toml");
    let bundle = temp.path().join("bundle.toml");
    let out = temp.path().join("out");
    write_api(&api);
    write_internal_bundle(&bundle, "python_stale_types", "api.toml", "python_provider");
    let output = generate_internal_python(InternalPythonGenerateConfig {
        bundle_toml: bundle,
        out_dir: out.clone(),
        layout: OutputLayout::unified(),
    })
    .expect("generate Python profile");
    write_output(&output, &out).expect("write Python profile");
    let package = out.join("internal").join(format!(
        "python_stale_types-{:016x}",
        bundle_id("python_stale_types")
    ));
    let types = package.join("host/types.py");
    let source = fs::read_to_string(&types).expect("read generated host types");
    fs::write(
        &types,
        source.replacen(
            "_polyplug_internal_generation_fingerprint = 0x",
            "_polyplug_internal_generation_fingerprint = 0x0 #",
            1,
        ),
    )
    .expect("write stale host types");
    let script = temp.path().join("import.py");
    fs::write(
        &script,
        "import importlib.util, sys\nfrom pathlib import Path\nroot = Path(sys.argv[1])\nname = 'stale_types'\nspec = importlib.util.spec_from_file_location(name, root / '__init__.py', submodule_search_locations=[str(root)])\npkg = importlib.util.module_from_spec(spec)\nsys.modules[name] = pkg\nspec.loader.exec_module(pkg)\nimportlib.import_module(name + '.internal')\n",
    )
    .expect("write Python import script");
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates directory")
        .parent()
        .expect("workspace directory");
    let python_path = join_paths([
        workspace.join("sdks").join("python"),
        workspace.join("sdks").join("python").join("host"),
        workspace.join("sdks").join("python").join("polyplug_abi"),
        workspace.join("sdks").join("python").join("guest"),
    ])
    .expect("join Python import paths");
    let result = Command::new("python3")
        .arg(&script)
        .arg(&package)
        .env("PYTHONPATH", python_path)
        .output()
        .expect("import stale Python profile");
    assert!(
        !result.status.success()
            && String::from_utf8_lossy(&result.stderr)
                .contains("generated internal partitions are incompatible"),
        "stale generated host types must fail Python root import:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );
}
