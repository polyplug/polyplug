//! Runtime test: a polyplugc-GENERATED Deno host caller (#28) drives the real
//! `test.add` native guest over Deno FFI.
//!
//! `polyplugc::generate` emits the Deno host caller (`host/callers.ts`) for the
//! `test.add` contract, then a Deno driver imports that generated class and the
//! polyplug Deno FFI host SDK, loads the native `test_plugin` bundle, and calls
//! `add({a, b})`. This exercises the generated struct-parameter + scalar-return
//! marshalling end to end — the compiler-invisible class the generated-text unit
//! tests (in `crates/polyplugc`) cannot prove.
//!
//! Skip policy: when `deno`, the native loader cdylib, or the prebuilt fixtures
//! are unavailable, the test logs an explicit skip — never a silent pass.

#![allow(clippy::expect_used)]

mod cli_support;
use cli_support::cli_generate;

use std::path::PathBuf;
use std::process::Command;

use polyplug_codegen::GenerateConfig;
use polyplug_codegen::GenerateOutput;
use polyplug_codegen::Lang;
use polyplug_codegen::OutputLayout;
use polyplug_codegen::Side;

const POLYPLUG_SO: &str = env!("POLYPLUG_SO");
const POLYPLUG_NATIVE_LIB: &str = env!("POLYPLUG_NATIVE_LIB");
const TEST_PLUGIN_DIR: &str = env!("TEST_PLUGIN_DIR");

fn deno_available() -> bool {
    Command::new("deno")
        .arg("--version")
        .output()
        .map(|o: std::process::Output| o.status.success())
        .unwrap_or(false)
}

/// Render an absolute filesystem path as a `file://` URL usable as a Deno module
/// specifier on every platform. Unix paths begin with `/` (→ `file:///abs`);
/// Windows paths begin with a drive letter (→ `file:///C:/abs`).
fn to_file_url(path: &std::path::Path) -> String {
    let forward: String = path.to_string_lossy().replace('\\', "/");
    if forward.starts_with('/') {
        format!("file://{forward}")
    } else {
        format!("file:///{forward}")
    }
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("parent of tests/integration")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

#[test]
fn generated_deno_caller_dispatches_struct_param_through_native_guest() {
    if !deno_available() {
        eprintln!("[SKIP] deno not found on PATH");
        return;
    }
    for (label, path) in [
        ("POLYPLUG_SO", POLYPLUG_SO),
        ("POLYPLUG_NATIVE_LIB", POLYPLUG_NATIVE_LIB),
    ] {
        if path.is_empty() || !PathBuf::from(path).exists() {
            eprintln!("[SKIP] {label} not built at {path:?} — run `cargo build`");
            return;
        }
    }
    let plugin_dir: PathBuf = PathBuf::from(TEST_PLUGIN_DIR);
    if !plugin_dir.join("manifest.toml").exists() {
        eprintln!("[SKIP] test_plugin bundle not built at {TEST_PLUGIN_DIR:?}");
        return;
    }

    let root: PathBuf = workspace_root();
    let api_toml: PathBuf = root.join("tests").join("fixtures").join("test_api.toml");
    assert!(
        api_toml.exists(),
        "test_api.toml fixture missing: {api_toml:?}"
    );

    // Unique scratch dir for the generated caller + driver.
    let scratch: PathBuf =
        std::env::temp_dir().join(format!("polyplug_deno_caller_{}", std::process::id()));
    let gen_dir: PathBuf = scratch.join("generated");
    std::fs::create_dir_all(&gen_dir).expect("create scratch generated dir");

    // Emit the Deno host caller (host/types.ts + host/callers.ts) for test.add.
    let config: GenerateConfig = GenerateConfig {
        api_toml: api_toml.clone(),
        lang: Lang::JsQuickJs,
        side: Side::Host,
        layout: OutputLayout::unified(),
    };
    let output: GenerateOutput =
        cli_generate(&config, &gen_dir).expect("polyplugc generate (js-quickjs host)");
    for file in &output.files {
        let file_path: PathBuf = gen_dir.join(&file.path);
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent).expect("create generated parent dir");
        }
        std::fs::write(&file_path, &file.content).expect("write generated file");
    }

    // Driver imports the generated caller class + the Deno host SDK by absolute
    // `file://` URL (the SDK's own relative imports resolve from its workspace
    // location). A bare absolute path is a valid module specifier on Unix but not
    // on Windows (`C:/…` is not a URL), so both are normalized to `file://` URLs.
    let host_mod: String = to_file_url(&root.join("sdks/js/host/mod.js"));
    let native_loader_mod: String = to_file_url(&root.join("sdks/js/loaders/native/mod.ts"));
    let driver_ts: String = format!(
        "import {{ openPolyplug, runtimeNew }} from \"{host_mod}\";\n\
         import {{ registerNativeLoader }} from \"{native_loader_mod}\";\n\
         import {{ TestAddContract }} from \"./generated/host/callers.ts\";\n\
         \n\
         const POLYPLUG_SO = Deno.env.get(\"POLYPLUG_SO\") ?? \"\";\n\
         const TEST_PLUGIN_DIR = Deno.env.get(\"TEST_PLUGIN_DIR\") ?? \"\";\n\
         \n\
         const lib = openPolyplug(POLYPLUG_SO);\n\
         try {{\n\
         \x20   const rt = runtimeNew(lib);\n\
         \x20   try {{\n\
         \x20       registerNativeLoader(rt);\n\
         \x20       rt.loadBundle(TEST_PLUGIN_DIR);\n\
         \x20       const caller = TestAddContract.create(rt);\n\
         \x20       if (caller === null) throw new Error(\"TestAddContract.create returned null\");\n\
         \x20       const sum = caller.add({{ a: 2, b: 3 }});\n\
         \x20       if (sum !== 5) throw new Error(`add({{a:2,b:3}}) = ${{sum}}, expected 5`);\n\
         \x20       const sum2 = caller.add({{ a: 40, b: 2 }});\n\
         \x20       if (sum2 !== 42) throw new Error(`add({{a:40,b:2}}) = ${{sum2}}, expected 42`);\n\
         \x20       caller.destroy();\n\
         \x20       console.log(\"OK: generated Deno caller struct-param dispatch returned correct sums\");\n\
         \x20   }} finally {{ rt[Symbol.dispose](); }}\n\
         }} finally {{ lib.close(); }}\n",
    );
    let driver_path: PathBuf = scratch.join("driver.ts");
    std::fs::write(&driver_path, driver_ts).expect("write driver.ts");

    // Resolve the SDK's bare `@polyplug/*` specifiers (declared as `jsr:` in the
    // SDK's own deno.json) to the in-tree source. The import map's relative paths
    // resolve against the map file's location, not the scratch driver's.
    let import_map: PathBuf = root.join("tests/fixtures/deno_local_imports.json");

    let output: std::process::Output = Command::new("deno")
        .arg("run")
        .arg("--import-map")
        .arg(&import_map)
        .arg("--allow-ffi")
        .arg("--allow-env")
        .arg("--allow-read")
        .arg(&driver_path)
        .env("POLYPLUG_SO", POLYPLUG_SO)
        .env("POLYPLUG_NATIVE_LIB", POLYPLUG_NATIVE_LIB)
        .env("TEST_PLUGIN_DIR", TEST_PLUGIN_DIR)
        .output()
        .expect("failed to spawn deno");

    let stdout: &str = core::str::from_utf8(&output.stdout).unwrap_or("");
    let stderr: &str = core::str::from_utf8(&output.stderr).unwrap_or("");

    // Best-effort cleanup before asserting (so a failure still leaves no scratch).
    let _ = std::fs::remove_dir_all(&scratch);

    assert!(
        output.status.success(),
        "generated Deno caller driver failed.\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("OK: generated Deno caller struct-param dispatch returned correct sums"),
        "driver did not report success.\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}
