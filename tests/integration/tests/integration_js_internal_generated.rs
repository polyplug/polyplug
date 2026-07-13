//! Runtime proof for the generated JavaScript internal-plugin profile.

#![allow(clippy::expect_used)]

use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use polyplug_codegen::InternalJavaScriptGenerateConfig;
use polyplug_codegen::generate_internal_javascript;
use polyplug_codegen::write_output;

const POLYPLUG_SO: &str = env!("POLYPLUG_SO");

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("parent of integration tests")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

fn deno_available() -> bool {
    Command::new("deno")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn to_file_url(path: &Path) -> String {
    let normalized: String = path.to_string_lossy().replace('\\', "/");
    if normalized.starts_with('/') {
        format!("file://{normalized}")
    } else {
        format!("file:///{normalized}")
    }
}

#[cfg(windows)]
fn copy_directory(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).expect("create copied profile directory");
    for entry in fs::read_dir(source).expect("read generated profile directory") {
        let entry = entry.expect("read generated profile entry");
        let target = destination.join(entry.file_name());
        if entry
            .file_type()
            .expect("generated profile entry type")
            .is_dir()
        {
            copy_directory(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).expect("copy generated profile file");
        }
    }
}

#[test]
fn generated_javascript_internal_profile_registers_and_dispatches() {
    if !deno_available() {
        eprintln!("[SKIP] deno is not available");
        return;
    }
    if POLYPLUG_SO.is_empty() || !Path::new(POLYPLUG_SO).exists() {
        eprintln!("[SKIP] polyplug host library is not built at {POLYPLUG_SO:?}");
        return;
    }

    let root: PathBuf = workspace_root();
    let js_bridge: PathBuf = root.join("target/debug/libpolyplug_js.so");
    if !js_bridge.exists() {
        eprintln!(
            "[SKIP] JavaScript bridge is not built at {}",
            js_bridge.display()
        );
        return;
    }

    let scratch: tempfile::TempDir = tempfile::tempdir().expect("create temporary profile project");
    let api: PathBuf = scratch.path().join("api.toml");
    let bundle: PathBuf = scratch.path().join("bundle.toml");
    let older_bundle: PathBuf = scratch.path().join("older.toml");
    let generated: PathBuf = scratch.path().join("generated");
    fs::write(
        &api,
        "[[enum]]\nname = \"Mode\"\nrepr = \"u32\"\n\n[[enum.variants]]\nname = \"Fast\"\nvalue = \"1\"\n\n[[types]]\nname = \"Inner\"\nfields = [{ name = \"name\", type = \"StringView\" }]\n\n[[types]]\nname = \"Outer\"\nfields = [{ name = \"inner\", type = \"Inner\" }, { name = \"payload\", type = \"Buffer\" }]\n\n[[plugin_contract]]\nname = \"generated.js.profile\"\nversion = \"1.0\"\n\n[[plugin_contract.functions]]\nname = \"next\"\nreturn = \"u32\"\n\n[[plugin_contract.functions]]\nname = \"scalar\"\nparams = [{ name = \"value\", type = \"u32\" }]\nreturn = \"u32\"\n\n[[plugin_contract.functions]]\nname = \"text\"\nparams = [{ name = \"value\", type = \"StringView\" }]\nreturn = \"StringView\"\n\n[[plugin_contract.functions]]\nname = \"accept\"\nparams = [{ name = \"mode\", type = \"Mode\" }, { name = \"item\", type = \"Outer\" }, { name = \"values\", type = \"Array<u32>\" }]\n\n[[plugin_contract.functions]]\nname = \"list\"\nreturn = \"Array<u32>\"\n\n[[plugin_contract.functions]]\nname = \"fail\"\n",
    )
    .expect("write API");
    fs::write(
        &bundle,
        "[bundle]\nname = \"generated_js_internal\"\nversion = \"1.0\"\napi = \"api.toml\"\n\n[[plugin]]\nname = \"stateful_provider\"\nimplements = [\"generated.js.profile@1.0\"]\n\n[[plugin]]\nname = \"same_contract_provider\"\nimplements = [\"generated.js.profile@1.0\"]\n",
    )
    .expect("write artifactless internal bundle");
    fs::write(
        &older_bundle,
        "[bundle]\nname = \"older_js_internal\"\nversion = \"1.0\"\napi = \"api.toml\"\n\n[[plugin]]\nname = \"old_provider\"\nimplements = [\"generated.js.profile@1.0\"]\n",
    )
    .expect("write older artifactless internal bundle");

    let output = generate_internal_javascript(InternalJavaScriptGenerateConfig {
        bundle_toml: bundle,
        out_dir: generated.clone(),
    })
    .expect("generate JavaScript internal profile");
    write_output(&output, &generated).expect("write JavaScript internal profile");
    let older_output = generate_internal_javascript(InternalJavaScriptGenerateConfig {
        bundle_toml: older_bundle,
        out_dir: generated.clone(),
    })
    .expect("generate older JavaScript internal profile");
    write_output(&older_output, &generated).expect("write older JavaScript internal profile");
    let profile_dir: PathBuf = output
        .files
        .iter()
        .find_map(|file| {
            file.path
                .parent()
                .and_then(Path::parent)
                .map(Path::to_path_buf)
        })
        .expect("generated internal namespace");
    let profile_root: PathBuf = generated.join(profile_dir);
    let older_dir: PathBuf = older_output
        .files
        .iter()
        .find_map(|file| {
            file.path
                .parent()
                .and_then(Path::parent)
                .map(Path::to_path_buf)
        })
        .expect("generated older namespace");
    let older_root: PathBuf = generated.join(older_dir);

    let host_mod: String = to_file_url(&root.join("sdks/js/host/mod.js"));
    let import_map: PathBuf = scratch.path().join("deno-imports.json");
    fs::write(
        &import_map,
        format!(
            "{{\"imports\":{{\"@polyplug/abi\":\"{}\",\"@polyplug/host\":\"{}\",\"@polyplug/loaders/js\":\"{}\"}}}}",
            to_file_url(&root.join("sdks/js/abi/polyplug_abi.ts")),
            host_mod,
            to_file_url(&root.join("sdks/js/loaders/js/mod.ts"))
        ),
    )
    .expect("write local Deno import map");
    let driver: PathBuf = scratch.path().join("driver.ts");
    fs::write(
        &driver,
        format!(
            "import {{ openPolyplug, runtimeNew }} from \"{host_mod}\";\n\
             import {{ InternalProviders as OlderProviders, register as registerOlder }} from \"./older/internal.ts\";\n\
             import {{ InternalProviders, register }} from \"./profile/internal.ts\";\n\
             const lib = openPolyplug(Deno.env.get(\"POLYPLUG_SO\") ?? \"\");\n\
             try {{\n\
             \x20   const runtime = runtimeNew(lib);\n\
             \x20   try {{\n\
             \x20       let retryRejected = false;\n\
             \x20       try {{ register(runtime, new InternalProviders({{ stateful_provider_generated_js_profile: () => {{ throw new Error(\"factory failure\"); }}, same_contract_provider_generated_js_profile: () => {{ throw new Error(\"factory failure\"); }} }})); }} catch {{ retryRejected = true; }}\n\
             \x20       if (!retryRejected) throw new Error(\"failed attempt did not surface\");\n\
             \x20       registerOlder(runtime, new OlderProviders({{ old_provider_generated_js_profile: () => ({{ next: () => 0, scalar: value => value + 500, text: () => \"older\", accept: () => {{}}, list: () => [], fail: () => {{}} }}) }}));\n\
             \x20       let checkedAccept = false;\n\
             \x20       const registered = register(runtime, new InternalProviders({{\n\
             \x20           stateful_provider_generated_js_profile: () => {{\n\
             \x20               let count = 0;\n\
             \x20               return {{\n\
             \x20                   next() {{ count += 1; return count; }},\n\
             \x20                   scalar(value) {{ return value + 1; }},\n\
             \x20                   text(_value) {{ return \"generated internal text\"; }},\n\
             \x20                   accept(mode, item, values) {{\n\
             \x20                       if (mode !== 1 || item.inner.name.len !== 5 || item.payload.len !== 3 || values.len.lo !== 2) throw new Error(\"typed aggregate mismatch\");\n\
             \x20                       checkedAccept = true;\n\
             \x20                   }},\n\
             \x20                   list() {{ return [7, 9]; }},\n\
             \x20                   fail() {{ throw new Error(\"expected generated error\"); }},\n\
             \x20               }};\n\
             \x20           }},\n\
             \x20           same_contract_provider_generated_js_profile: () => {{ let count = 0; return {{ next: () => {{ count += 1; return count; }}, scalar: value => value + 100, text: () => \"same contract\", accept: () => {{}}, list: () => [1], fail: () => {{}} }}; }},\n\
             \x20       }}));\n\
             \x20       if (registered.bundleId === 0n) throw new Error(\"missing canonical bundle id\");\n\
             \x20       const caller = registered.stateful_provider_generated_js_profile;\n\
             \x20       const sameContractCaller = registered.same_contract_provider_generated_js_profile;\n\
             \x20       if (caller.scalar(41) !== 42 || sameContractCaller.scalar(41) !== 141 || caller.text(\"ignored\") !== \"generated internal text\") throw new Error(\"exact committed caller dispatch failed\");\n\
             \x20       const values = new Uint32Array([3, 4]);\n\
             \x20       const item = {{ inner: {{ name: \"hello\" }}, payload: new Uint8Array([1, 2, 3]) }};\n\
             \x20       caller.accept(1, item, {{ items: BigInt(Deno.UnsafePointer.value(Deno.UnsafePointer.of(values))), len: 2n }});\n\
             \x20       if (!checkedAccept) throw new Error(\"enum/nested struct/buffer/array dispatch failed\");\n\
             \x20       const listed = caller.list();\n\
             \x20       if (listed.len !== 2n) throw new Error(\"array return failed\");\n\
             \x20       if (caller.next() !== 1 || sameContractCaller.next() !== 1 || caller.next() !== 2) throw new Error(\"independent same-contract providers failed\");\n\
             \x20       let consumedRejected = false;\n\
             \x20       const consumed = new InternalProviders({{ stateful_provider_generated_js_profile: () => ({{ next: () => 0, scalar: value => value, text: value => value, accept: () => {{}}, list: () => [], fail: () => {{}} }}), same_contract_provider_generated_js_profile: () => ({{ next: () => 0, scalar: value => value, text: value => value, accept: () => {{}}, list: () => [], fail: () => {{}} }}) }});\n\
             \x20       try {{ register(runtime, consumed); }} catch {{}}\n\
             \x20       try {{ register(runtime, consumed); }} catch {{ consumedRejected = true; }}\n\
             \x20       if (!consumedRejected) throw new Error(\"consumed provider input was reusable\");\n\
             \x20       let errorObserved = false; try {{ caller.fail(); }} catch {{ errorObserved = true; }}\n\
             \x20       if (!errorObserved) throw new Error(\"generated error did not propagate\");\n\
             \x20       caller.destroy();\n\
             \x20       runtime.unloadBundle(registered.bundleId);\n\
             \x20       console.log(\"OK: generated JavaScript internal profile E2E\");\n\
             \x20   }} finally {{ runtime[Symbol.dispose](); }}\n\
             }} finally {{ lib.close(); }}\n"
        ),
    )
    .expect("write Deno driver");
    fs::create_dir_all(driver.parent().expect("driver parent")).expect("driver parent exists");
    let profile_link: PathBuf = scratch.path().join("profile");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&profile_root, &profile_link).expect("link generated profile");
    #[cfg(windows)]
    copy_directory(&profile_root, &profile_link);
    let older_link: PathBuf = scratch.path().join("older");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&older_root, &older_link).expect("link older generated profile");
    #[cfg(windows)]
    copy_directory(&older_root, &older_link);

    let result = Command::new("deno")
        .arg("run")
        .arg("--import-map")
        .arg(import_map)
        .arg("--allow-ffi")
        .arg("--allow-env")
        .arg("--allow-read")
        .arg(&driver)
        .env("POLYPLUG_SO", POLYPLUG_SO)
        .env("POLYPLUG_JS_LIB", js_bridge)
        .output()
        .expect("run generated JavaScript internal driver");
    let stdout = String::from_utf8_lossy(&result.stdout);
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        result.status.success(),
        "generated JavaScript internal profile driver failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(stdout.contains("OK: generated JavaScript internal profile E2E"));
}
