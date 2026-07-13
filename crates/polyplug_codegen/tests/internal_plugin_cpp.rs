#![allow(clippy::expect_used)]

use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use polyplug_codegen::{
    GenerateConfig, InternalCppGenerateConfig, Lang, Side, generate, generate_internal_cpp,
    write_output,
};
use tempfile::TempDir;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace crates directory")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

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
name = "Envelope"
fields = [
  {{ name = "inner", type = "Inner" }},
  {{ name = "entries", type = "Array<Inner>" }},
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
params = [{{ name = "address", type = "u64" }}, {{ name = "bytes", type = "Buffer" }}]
return = "u32"

[[plugin_contract.functions]]
name = "inspect"
params = [
  {{ name = "label", type = "StringView" }},
  {{ name = "mode", type = "Mode" }},
  {{ name = "entries", type = "Array<Inner>" }},
]
return = "StringView"

[[plugin_contract.functions]]
name = "take_inner"
params = [{{ name = "inner", type = "Inner" }}]
return = "u32"
"#
        ),
    )
    .expect("write API fixture");
}

fn write_bundle(path: &Path, api: &str, bundle_name: &str, plugin_name: &str, contract: &str) {
    fs::write(
        path,
        format!(
            "[bundle]\nname = \"{bundle_name}\"\nversion = \"1.0\"\napi = \"{api}\"\n\n[[plugin]]\nname = \"{plugin_name}\"\nimplements = [\"{contract}@1.0\"]\n"
        ),
    )
    .expect("write internal bundle fixture");
}

fn internal_output(temp: &TempDir, name: &str, contract: &str) -> polyplug_codegen::GenerateOutput {
    let api = temp.path().join(format!("{name}.toml"));
    let bundle = temp.path().join(format!("{name}.bundle.toml"));
    write_api(&api, contract);
    write_bundle(
        &bundle,
        &format!("{name}.toml"),
        name,
        &format!("{name}.provider"),
        contract,
    );
    generate_internal_cpp(InternalCppGenerateConfig {
        bundle_toml: bundle,
        out_dir: temp.path().join("out"),
    })
    .expect("generate internal C++ profile")
}

#[test]
fn internal_cpp_profile_is_opt_in_artifactless_and_typed() {
    let temp = TempDir::new().expect("create temporary profile fixture");
    let output = internal_output(&temp, "cpp_internal_profile", "platform.Plugin");
    let paths = output
        .files
        .iter()
        .map(|file| file.path.to_string_lossy().into_owned())
        .collect::<HashSet<_>>();
    assert!(
        paths
            .iter()
            .all(|path| path.starts_with("internal/cpp_internal_profile-"))
    );
    assert!(
        paths
            .iter()
            .any(|path| path.ends_with("guest/internal_plugin.hpp"))
    );
    assert!(
        paths
            .iter()
            .any(|path| path.ends_with("host/host_callers.hpp"))
    );
    assert!(!paths.iter().any(|path| path.ends_with("manifest.toml")));

    let internal = output
        .files
        .iter()
        .find(|file| file.path.ends_with("guest/internal_plugin.hpp"))
        .expect("internal registrar header")
        .content
        .as_str();
    for required in [
        "struct InternalPluginRegistration",
        "InternalPluginRegistration register_internal_plugin",
        "std::move",
        "create_instance",
        "destroy_instance",
        "thread_local std::exception_ptr construction_failure_",
        "internal_plugin.rethrow_factory_failure",
        "runtime.unload_bundle(committed.bundle_id)",
        "std::rethrow_exception(primary)",
    ] {
        assert!(
            internal.contains(required),
            "missing `{required}` in generated internal profile"
        );
    }
    let callers = output
        .files
        .iter()
        .find(|file| file.path.ends_with("host/host_callers.hpp"))
        .expect("internal host caller header")
        .content
        .as_str();
    assert!(callers.contains("if (iface == interface_) {"));
    assert!(callers.contains("cached_revision_ = polyplug_load_revision(revision_host_);"));
    let interfaces = output
        .files
        .iter()
        .find(|file| file.path.ends_with("guest/interfaces.hpp"))
        .expect("internal ABI dispatch header")
        .content
        .as_str();
    for required in ["metadata", "read", "write", "inspect", "take_inner"] {
        assert!(
            interfaces.contains(required),
            "missing typed ABI dispatch for `{required}`"
        );
    }

    let external_bundle = temp.path().join("external.bundle.toml");
    fs::write(
        &external_bundle,
        "[bundle]\nname = \"cpp_external_profile\"\nversion = \"1.0\"\napi = \"cpp_internal_profile.toml\"\nloader = \"python\"\nfile = \"external.py\"\n\n[[plugin]]\nname = \"external.provider\"\nimplements = [\"platform.Plugin@1.0\"]\n",
    )
    .expect("write external bundle fixture");
    let external = generate(GenerateConfig {
        api_toml: external_bundle,
        lang: Lang::Cpp,
        side: Side::Guest,
        out_dir: temp.path().join("external"),
    })
    .expect("generate external C++ bindings");
    assert!(
        external
            .files
            .iter()
            .all(|file| !file.path.starts_with("guest/internal")),
        "external generation must not emit internal-plugin artifacts"
    );
}

#[test]
fn two_internal_cpp_bundles_compile_without_symbol_collisions() {
    let temp = TempDir::new().expect("create temporary coexistence fixture");
    let first = internal_output(&temp, "cpp_first", "first.Plugin");
    let second = internal_output(&temp, "cpp_second", "second.Plugin");
    let paths = first
        .files
        .iter()
        .chain(&second.files)
        .map(|file| file.path.clone())
        .collect::<HashSet<_>>();
    assert_eq!(paths.len(), first.files.len() + second.files.len());

    let generated = temp.path().join("generated");
    write_output(&first, &generated).expect("write first internal C++ profile");
    write_output(&second, &generated).expect("write second internal C++ profile");
    let first_header = first
        .files
        .iter()
        .find(|file| file.path.ends_with("guest/internal_plugin.hpp"))
        .expect("first internal header")
        .path
        .to_string_lossy()
        .into_owned();
    let second_header = second
        .files
        .iter()
        .find(|file| file.path.ends_with("guest/internal_plugin.hpp"))
        .expect("second internal header")
        .path
        .to_string_lossy()
        .into_owned();
    let consumer = generated.join("coexist.cpp");
    fs::write(
        &consumer,
        format!(
            "#include \"{first_header}\"\n#include \"{second_header}\"\nint main() {{ return 0; }}\n"
        ),
    )
    .expect("write C++ coexistence consumer");
    let root = workspace_root();
    let status = Command::new("g++")
        .args(["-std=c++20", "-fsyntax-only"])
        .arg(&consumer)
        .arg("-I")
        .arg(&generated)
        .arg("-I")
        .arg(root.join("sdks/cpp/host"))
        .arg("-I")
        .arg(root.join("sdks/cpp/abi"))
        .status()
        .expect("invoke g++ for generated coexistence consumer");
    assert!(
        status.success(),
        "two generated internal C++ bundles must compile together"
    );
}

#[test]
fn internal_cpp_profile_registers_dispatches_and_retries_with_real_runtime() {
    let temp = TempDir::new().expect("create temporary C++ E2E fixture");
    let output = internal_output(&temp, "cpp_runtime_profile", "platform.Plugin");
    let generated = temp.path().join("generated");
    write_output(&output, &generated).expect("write internal C++ profile");
    let internal_header = output
        .files
        .iter()
        .find(|file| file.path.ends_with("guest/internal_plugin.hpp"))
        .expect("internal registration header");
    let header_path = internal_header.path.to_string_lossy().into_owned();
    let namespace = internal_header
        .content
        .lines()
        .find_map(|line| {
            line.strip_prefix("namespace ")
                .and_then(|value| value.strip_suffix("::internal_plugin {"))
        })
        .expect("bundle-scoped C++ namespace");
    let driver = generated.join("runtime_profile.cpp");
    let source = r#"
#include <cstdint>
#include <memory>
#include <stdexcept>
#include <utility>

#include "INTERNAL_HEADER"

namespace bindings = BINDINGS;

namespace {
int factory_drops = 0;

struct DropProbe {
    ~DropProbe() { ++factory_drops; }
};

class Provider final : public bindings::plugin::PlatformPluginGuestContract {
public:
    bindings::Envelope metadata() override {
        return bindings::Envelope{};
    }

    Buffer read(uint64_t, uint32_t) override {
        static uint8_t bytes[] = {4U, 5U, 6U};
        return Buffer{bytes, 3U, 3U};
    }

    uint32_t write(uint64_t address, Buffer) override {
        if (address == 0U) {
            throw std::runtime_error("write rejected");
        }
        return ++writes_;
    }

    StringView inspect(
        StringView,
        const bindings::Mode& mode,
        const bindings::ArrayOf_Inner&) override {
        if (mode != bindings::Mode::Ready) {
            throw std::runtime_error("unexpected mode");
        }
        static constexpr uint8_t text[] = {'o', 'k'};
        return StringView{text, 2U};
    }

    uint32_t take_inner(const bindings::Inner&) override {
        return 41U;
    }

private:
    uint32_t writes_ = 0U;
};
}  // namespace

int main() {
    auto runtime = polyplug::Runtime::builder().build();
    uint8_t payload[] = {9U};
    Buffer buffer{payload, 1U, 1U};
    static constexpr uint8_t label[] = {'x'};
    bindings::Inner inner{StringView{label, 1U}, buffer};
    bindings::ArrayOf_Inner entries{};

    {
        auto registration = bindings::internal_plugin::register_internal_plugin(
            runtime,
            [](const HostApi*) { return std::make_unique<Provider>(); });
        if (registration.internal_plugin_id != bindings::internal_plugin::INTERNAL_PLUGIN_ID) return 1;
        if (registration.cpp_runtime_profile_provider_platform_plugin.write(9U, buffer) != 1U) return 2;
        if (registration.cpp_runtime_profile_provider_platform_plugin.read(0U, 3U).len != 3U) return 3;
        if (registration.cpp_runtime_profile_provider_platform_plugin.inspect(
                StringView{label, 1U}, bindings::Mode::Ready, entries).len != 2U) return 4;
        if (registration.cpp_runtime_profile_provider_platform_plugin.take_inner(inner) != 41U) return 5;
        auto second = bindings::PlatformPluginContract::create(
            runtime.find_guest_contract(
                bindings::PLATFORM_PLUGIN_CONTRACT_ID, 1U),
            runtime.host());
        if (!second || second->write(9U, buffer) != 1U) return 6;
        try {
            registration.cpp_runtime_profile_provider_platform_plugin.write(0U, buffer);
            return 7;
        } catch (const std::runtime_error&) {
        }
        try {
            bindings::internal_plugin::register_internal_plugin(
                runtime,
                [probe = std::make_unique<DropProbe>()](const HostApi*) {
                    return std::make_unique<Provider>();
                });
            return 8;
        } catch (const std::runtime_error&) {
        }
        if (factory_drops != 1) return 9;
    }

    runtime.unload_bundle(bindings::internal_plugin::INTERNAL_PLUGIN_ID);
    auto fresh = bindings::internal_plugin::register_internal_plugin(
        runtime,
        [](const HostApi*) { return std::make_unique<Provider>(); });
    if (fresh.cpp_runtime_profile_provider_platform_plugin.write(9U, buffer) != 1U) return 10;
    return 0;
}
"#;
    fs::write(
        &driver,
        source
            .replace("INTERNAL_HEADER", &header_path)
            .replace("BINDINGS", namespace),
    )
    .expect("write generated C++ runtime driver");

    let root = workspace_root();
    let build = Command::new(env!("CARGO"))
        .args(["build", "-p", "polyplug"])
        .current_dir(&root)
        .status()
        .expect("build real polyplug runtime");
    assert!(build.success(), "build real polyplug runtime");
    let runtime_dir = root.join("target/debug");
    let executable = generated.join("runtime_profile");
    let compile = Command::new("g++")
        .arg("-std=c++20")
        .arg(&driver)
        .arg("-I")
        .arg(&generated)
        .arg("-I")
        .arg(root.join("sdks/cpp/host"))
        .arg("-I")
        .arg(root.join("sdks/cpp/abi"))
        .arg("-L")
        .arg(&runtime_dir)
        .arg("-lpolyplug")
        .arg(format!("-Wl,-rpath,{}", runtime_dir.display()))
        .arg("-o")
        .arg(&executable)
        .output()
        .expect("compile generated C++ runtime driver");
    assert!(
        compile.status.success(),
        "generated C++ runtime driver did not compile:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr),
    );
    let execution = Command::new(&executable)
        .output()
        .expect("run generated C++ runtime driver");
    assert!(
        execution.status.success(),
        "generated C++ runtime driver failed with {:?}:\nstdout: {}\nstderr: {}",
        execution.status.code(),
        String::from_utf8_lossy(&execution.stdout),
        String::from_utf8_lossy(&execution.stderr),
    );
}

#[test]
fn internal_cpp_profile_binds_exact_handles_and_rolls_back_failed_factories() {
    let temp = TempDir::new().expect("create exact-handle fixture");
    let api = temp.path().join("shared.toml");
    fs::write(
        &api,
        "[[plugin_contract]]\nname = \"shared.Guest\"\nversion = \"1.0\"\n\n[[plugin_contract.functions]]\nname = \"value\"\nreturn = \"u32\"\n",
    )
    .expect("write shared API");
    let older_bundle = temp.path().join("older.toml");
    fs::write(
        &older_bundle,
        "[bundle]\nname = \"older_shared\"\nversion = \"1.0\"\napi = \"shared.toml\"\n\n[[plugin]]\nname = \"older.provider\"\nimplements = [\"shared.Guest@1.0\"]\n",
    )
    .expect("write older bundle");
    let current_bundle = temp.path().join("current.toml");
    fs::write(
        &current_bundle,
        "[bundle]\nname = \"current_shared\"\nversion = \"1.0\"\napi = \"shared.toml\"\n\n[[plugin]]\nname = \"first.provider\"\nimplements = [\"shared.Guest@1.0\"]\n\n[[plugin]]\nname = \"second.provider\"\nimplements = [\"shared.Guest@1.0\"]\n",
    )
    .expect("write current bundle");
    let older = generate_internal_cpp(InternalCppGenerateConfig {
        bundle_toml: older_bundle,
        out_dir: temp.path().join("generated"),
    })
    .expect("generate older profile");
    let current = generate_internal_cpp(InternalCppGenerateConfig {
        bundle_toml: current_bundle,
        out_dir: temp.path().join("generated"),
    })
    .expect("generate current profile");
    let generated = temp.path().join("generated");
    write_output(&older, &generated).expect("write older profile");
    write_output(&current, &generated).expect("write current profile");
    let older_header = older
        .files
        .iter()
        .find(|file| file.path.ends_with("guest/internal_plugin.hpp"))
        .expect("older header");
    let current_header = current
        .files
        .iter()
        .find(|file| file.path.ends_with("guest/internal_plugin.hpp"))
        .expect("current header");
    let older_namespace = older_header
        .content
        .lines()
        .find_map(|line| {
            line.strip_prefix("namespace ")
                .and_then(|value| value.strip_suffix("::internal_plugin {"))
        })
        .expect("older namespace");
    let current_namespace = current_header
        .content
        .lines()
        .find_map(|line| {
            line.strip_prefix("namespace ")
                .and_then(|value| value.strip_suffix("::internal_plugin {"))
        })
        .expect("current namespace");
    let driver = generated.join("exact_handles.cpp");
    let source = r#"
#include <cstdint>
#include <memory>
#include <stdexcept>
#include <string_view>

#include "OLDER_HEADER"
#include "CURRENT_HEADER"

namespace older = OLDER_BINDINGS;
namespace current = CURRENT_BINDINGS;

namespace {
int first_factory_calls = 0;
int throwing_factory_calls = 0;
int null_factory_calls = 0;
int retry_first_factory_calls = 0;
int retry_second_factory_calls = 0;
int first_destroyed = 0;
int older_factory_calls = 0;
}

class Older final : public older::plugin::SharedGuestGuestContract {
public:
    uint32_t calls_ = 0;
    uint32_t value() override { return ++calls_; }
};

class First final : public current::plugin::SharedGuestGuestContract {
public:
    ~First() override { ++first_destroyed; }
    uint32_t value() override { return 11U; }
};

class Second final : public current::plugin::SharedGuestGuestContract {
public:
    uint32_t value() override { return 21U; }
};


int main() {
    auto runtime = polyplug::Runtime::builder().build();
    uint64_t older_id = 0U;
    uint64_t current_id = 0U;
    {
        auto old_registration = older::internal_plugin::register_internal_plugin(
            runtime,
            [](const HostApi*) {
                ++older_factory_calls;
                return std::make_unique<Older>();
            });
        older_id = old_registration.internal_plugin_id;
        if (old_registration.older_provider_shared_guest.value() != 1U || older_factory_calls != 1) return 10;

        try {
            current::internal_plugin::register_internal_plugin(
                runtime,
                [](const HostApi*) {
                    ++first_factory_calls;
                    return std::make_unique<First>();
                },
                [](const HostApi*) -> std::unique_ptr<Second> {
                    ++throwing_factory_calls;
                    throw std::runtime_error("second factory exploded");
                });
            return 1;
        } catch (const std::runtime_error& error) {
            if (std::string_view(error.what()) != "second factory exploded") return 2;
        }
        if (first_factory_calls != 1 || throwing_factory_calls != 1 || first_destroyed != 1) return 3;

        try {
            current::internal_plugin::register_internal_plugin(
                runtime,
                [](const HostApi*) {
                    ++first_factory_calls;
                    return std::make_unique<First>();
                },
                [](const HostApi*) -> std::unique_ptr<Second> {
                    ++null_factory_calls;
                    return nullptr;
                });
            return 4;
        } catch (const std::runtime_error& error) {
            if (std::string_view(error.what()) != "generated internal provider factory returned null") return 5;
        }
        if (first_factory_calls != 2 || null_factory_calls != 1 || first_destroyed != 2) return 6;

        auto registration = current::internal_plugin::register_internal_plugin(
            runtime,
            [](const HostApi*) {
                ++retry_first_factory_calls;
                return std::make_unique<First>();
            },
            [](const HostApi*) {
                ++retry_second_factory_calls;
                return std::make_unique<Second>();
            });
        current_id = registration.internal_plugin_id;
        if (retry_first_factory_calls != 1 || retry_second_factory_calls != 1) return 7;
        if (registration.first_provider_shared_guest.value() != 11U) return 8;
        if (registration.second_provider_shared_guest.value() != 21U) return 9;
        if (old_registration.older_provider_shared_guest.value() != 2U || older_factory_calls != 1) return 11;
    }
    runtime.unload_bundle(current_id);
    runtime.unload_bundle(older_id);
    return 0;
}
"#;
    fs::write(
        &driver,
        source
            .replace("OLDER_HEADER", &older_header.path.to_string_lossy())
            .replace("CURRENT_HEADER", &current_header.path.to_string_lossy())
            .replace("OLDER_BINDINGS", older_namespace)
            .replace("CURRENT_BINDINGS", current_namespace),
    )
    .expect("write exact-handle driver");
    let root = workspace_root();
    let build = Command::new(env!("CARGO"))
        .args(["build", "-p", "polyplug"])
        .current_dir(&root)
        .status()
        .expect("build real polyplug runtime");
    assert!(build.success(), "build real polyplug runtime");
    let runtime_dir = root.join("target/debug");
    let executable = generated.join("exact_handles");
    let compile = Command::new("g++")
        .arg("-std=c++20")
        .arg(&driver)
        .arg("-I")
        .arg(&generated)
        .arg("-I")
        .arg(root.join("sdks/cpp/host"))
        .arg("-I")
        .arg(root.join("sdks/cpp/abi"))
        .arg("-L")
        .arg(&runtime_dir)
        .arg("-lpolyplug")
        .arg(format!("-Wl,-rpath,{}", runtime_dir.display()))
        .arg("-o")
        .arg(&executable)
        .output()
        .expect("compile exact-handle driver");
    assert!(
        compile.status.success(),
        "exact-handle driver did not compile:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr),
    );
    let execution = Command::new(&executable)
        .output()
        .expect("run exact-handle driver");
    assert!(
        execution.status.success(),
        "exact-handle driver failed with {:?}:\nstdout: {}\nstderr: {}",
        execution.status.code(),
        String::from_utf8_lossy(&execution.stdout),
        String::from_utf8_lossy(&execution.stderr),
    );
}
