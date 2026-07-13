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
use crate::Side;
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
        "[[enum]]\nname = \"Mode\"\nrepr = \"u32\"\n\n[[enum.variants]]\nname = \"Fast\"\nvalue = \"1\"\n\n[[types]]\nname = \"Inner\"\nfields = [{ name = \"name\", type = \"StringView\" }]\n\n[[types]]\nname = \"Outer\"\nfields = [{ name = \"inner\", type = \"Inner\" }, { name = \"payload\", type = \"Buffer\" }]\n\n[[plugin_contract]]\nname = \"python.profile\"\nversion = \"1.0\"\n\n[[plugin_contract.functions]]\nname = \"scalar\"\nparams = [{ name = \"value\", type = \"u32\" }]\nreturn = \"u32\"\n\n[[plugin_contract.functions]]\nname = \"text\"\nparams = [{ name = \"value\", type = \"StringView\" }]\nreturn = \"StringView\"\n\n[[plugin_contract.functions]]\nname = \"many\"\nparams = [{ name = \"mode\", type = \"Mode\" }, { name = \"item\", type = \"Outer\" }]\nreturn = \"Outer\"\n\n[[plugin_contract.functions]]\nname = \"buffer\"\nparams = [{ name = \"value\", type = \"Buffer\" }]\nreturn = \"Buffer\"\n",
    )
    .expect("write API TOML");
    let mut api = fs::OpenOptions::new()
        .append(true)
        .open(path)
        .expect("open API TOML for array contract");
    api.write_all(
        b"\n[[plugin_contract.functions]]\nname = \"array_roundtrip\"\nparams = [{ name = \"items\", type = \"Array<Inner>\" }]\nreturn = \"Array<Inner>\"\n",
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
            out_dir: temp.path().join("host-out"),
        })
        .expect("generate default Python host bindings"),
    );
    let external = output_map(
        generate(GenerateConfig {
            api_toml: bundle,
            lang: Lang::Python,
            side: Side::Guest,
            out_dir: temp.path().join("guest-out"),
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
    })
    .expect("generate first internal bundle");
    let second = generate_internal_python(InternalPythonGenerateConfig {
        bundle_toml: second_bundle,
        out_dir: temp.path().join("out"),
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
