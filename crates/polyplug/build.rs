//! Build script for polyplug.
//!
//! Compiles `test_plugin` (a cdylib workspace member) and copies the resulting
//! shared library to `tests/fixtures/` so integration tests can load it.
//!

// Build scripts have no caller to propagate errors to — .expect() is the only
// option because a missing env var or failed compilation must abort the build.
#![allow(clippy::expect_used)]

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

struct ManifestBuilder {
    id: u64,
    name: String,
    version: String,
    runtime: String,
    file: String,
    provides: Vec<String>,
    function_count: Vec<(String, u32)>,
    needs_reinit_on_dep_reload: bool,
    dependencies: Vec<DependencyEntry>,
}

struct DependencyEntry {
    kind: String,
    contract: String,
    min_version: String,
    bundle: Option<String>,
    contract_id: u64,
    bundle_id: Option<u64>,
}

impl ManifestBuilder {
    fn to_toml(&self) -> String {
        let mut out: String = String::new();

        out.push_str(&format!("id = {}\n", self.id));
        out.push_str(&format!("name = \"{}\"\n", self.name));
        out.push_str(&format!("version = \"{}\"\n", self.version));
        out.push_str(&format!("runtime = \"{}\"\n", self.runtime));
        out.push_str(&format!("file = \"{}\"\n", self.file));

        if !self.provides.is_empty() {
            let provides: String = self
                .provides
                .iter()
                .map(|s| format!("\"{}\"", s))
                .collect::<Vec<String>>()
                .join(", ");
            out.push_str(&format!("provides = [{}]\n", provides));
        }

        if self.needs_reinit_on_dep_reload {
            out.push_str("needs_reinit_on_dep_reload = true\n");
        }

        if !self.function_count.is_empty() {
            out.push_str("\n[function_count]\n");
            for (contract, count) in &self.function_count {
                out.push_str(&format!("\"{}\" = {}\n", contract, count));
            }
        }

        for dep in &self.dependencies {
            out.push_str("\n[[dependency]]\n");
            out.push_str(&format!("kind = \"{}\"\n", dep.kind));
            out.push_str(&format!("contract = \"{}\"\n", dep.contract));
            out.push_str(&format!("min_version = \"{}\"\n", dep.min_version));
            if let Some(bundle) = &dep.bundle {
                out.push_str(&format!("bundle = \"{}\"\n", bundle));
            }
            if dep.contract_id != 0 {
                out.push_str(&format!("contract_id = {}\n", dep.contract_id));
            }
            if let Some(bundle_id) = dep.bundle_id {
                out.push_str(&format!("bundle_id = {}\n", bundle_id));
            }
        }

        out
    }
}

fn main() {
    // Emit -export-dynamic linker flag so that polyplug_host_alloc and
    // polyplug_host_free are visible to plugins loaded via dlopen at test time.
    // This is a no-op for non-executable link jobs (cdylib/rlib).
    #[cfg(target_os = "linux")]
    println!("cargo:rustc-link-arg=-Wl,-export-dynamic");

    // Re-run if test_plugin sources change.
    // Re-run if test_plugin sources change.
    println!("cargo:rerun-if-changed=tests/fixtures/test_plugin/src/lib.rs");
    println!("cargo:rerun-if-changed=tests/fixtures/test_plugin/Cargo.toml");

    let manifest_dir: PathBuf =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set"));

    // Workspace root is two levels up from crates/polyplug/
    let workspace_root: PathBuf = manifest_dir
        .parent()
        .expect("parent of polyplug")
        .parent()
        .expect("workspace root")
        .to_path_buf();

    let out_dir: PathBuf = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR not set"));

    // Determine the target directory: walk up from OUT_DIR to find the `debug` or `release` dir.
    // OUT_DIR is typically: <workspace>/target/<profile>/build/<crate>-<hash>/out
    // We need:               <workspace>/target/<profile>/
    // But for our subprocess build we want to use the same target dir as the workspace.
    // The safest approach is to use CARGO_TARGET_DIR if set, or default to workspace_root/target.
    let target_dir: PathBuf = env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| workspace_root.join("target"));

    // Use a dedicated subdirectory for plugin builds so the parent `cargo bench`
    // (or `cargo test`) process does not hold a file lock on the same target dir.
    // Without this, sub-cargo invocations deadlock waiting for the lock.
    let plugin_target_dir: PathBuf = target_dir.join("plugin-builds");

    // Build test_plugin as a release cdylib via cargo.
    // We pass --target-dir to avoid polluting or conflicting with the current build.
    let status: std::process::ExitStatus = Command::new(env!("CARGO"))
        .arg("build")
        .arg("--package")
        .arg("test_plugin")
        .arg("--release")
        .arg("--target-dir")
        .arg(&plugin_target_dir)
        .current_dir(&workspace_root)
        .env_remove("CARGO_TARGET_DIR")
        .status()
        .expect("failed to run cargo build for test_plugin");

    if !status.success() {
        panic!("cargo build -p test_plugin failed with status: {}", status);
    }

    // Determine the platform-specific output filename.
    let lib_filename: &str = if cfg!(target_os = "macos") {
        "libtest_plugin.dylib"
    } else if cfg!(target_os = "windows") {
        "test_plugin.dll"
    } else {
        "libtest_plugin.so"
    };

    let built_so: PathBuf = plugin_target_dir.join("release").join(lib_filename);

    // Copy to tests/fixtures/ with a stable, known name.
    let fixtures_dir: PathBuf = workspace_root.join("tests").join("fixtures");
    fs::create_dir_all(&fixtures_dir).expect("failed to create tests/fixtures/");

    let dest_so: PathBuf = fixtures_dir.join(lib_filename);
    fs::copy(&built_so, &dest_so).unwrap_or_else(|e| {
        panic!(
            "failed to copy {} to {}: {}",
            built_so.display(),
            dest_so.display(),
            e
        )
    });

    // Also copy to OUT_DIR so tests can find it via env!("OUT_DIR").
    let out_so: PathBuf = out_dir.join(lib_filename);
    fs::copy(&built_so, &out_so).unwrap_or_else(|e| {
        panic!(
            "failed to copy {} to {}: {}",
            built_so.display(),
            out_so.display(),
            e
        )
    });

    // Emit the path so integration tests can use:
    //   env!("TEST_PLUGIN_SO")
    println!("cargo:rustc-env=TEST_PLUGIN_SO={}", dest_so.display());

    // Create bundle directory for test_plugin
    let test_plugin_dir: PathBuf = fixtures_dir.join("test_plugin_dir");
    fs::create_dir_all(&test_plugin_dir)
        .unwrap_or_else(|e: std::io::Error| panic!("failed to create test_plugin_dir: {}", e));
    fs::copy(&dest_so, test_plugin_dir.join(lib_filename)).unwrap_or_else(|e: std::io::Error| {
        panic!("failed to copy test_plugin .so to bundle dir: {}", e)
    });
    let test_plugin_manifest: ManifestBuilder = ManifestBuilder {
        id: 9569986636177360922,
        name: "test_plugin".to_owned(),
        version: "1.0".to_owned(),
        runtime: "native".to_owned(),
        file: lib_filename.to_owned(),
        provides: vec!["test.add".to_owned()],
        function_count: vec![("test.add@1".to_owned(), 4)],
        needs_reinit_on_dep_reload: false,
        dependencies: Vec::new(),
    };
    fs::write(
        test_plugin_dir.join("manifest.toml"),
        test_plugin_manifest.to_toml(),
    )
    .unwrap_or_else(|e: std::io::Error| panic!("failed to write test_plugin manifest.toml: {}", e));
    println!(
        "cargo:rustc-env=TEST_PLUGIN_DIR={}",
        test_plugin_dir.display()
    );

    // ─── memory_plugin build ──────────────────────────────────────────────────
    // Re-run if memory_plugin sources change.
    println!("cargo:rerun-if-changed=tests/fixtures/memory_plugin/src/lib.rs");
    println!("cargo:rerun-if-changed=tests/fixtures/memory_plugin/Cargo.toml");

    // Build memory_plugin as a release cdylib via cargo.
    let memory_status: std::process::ExitStatus = Command::new(env!("CARGO"))
        .arg("build")
        .arg("--package")
        .arg("memory_plugin")
        .arg("--release")
        .arg("--target-dir")
        .arg(&plugin_target_dir)
        .current_dir(&workspace_root)
        .env_remove("CARGO_TARGET_DIR")
        .status()
        .expect("failed to run cargo build for memory_plugin");

    if !memory_status.success() {
        panic!(
            "cargo build -p memory_plugin failed with status: {}",
            memory_status
        );
    }

    let memory_lib_filename: &str = if cfg!(target_os = "macos") {
        "libmemory_plugin.dylib"
    } else if cfg!(target_os = "windows") {
        "memory_plugin.dll"
    } else {
        "libmemory_plugin.so"
    };

    let built_memory_so: PathBuf = plugin_target_dir.join("release").join(memory_lib_filename);
    let dest_memory_so: PathBuf = fixtures_dir.join(memory_lib_filename);
    fs::copy(&built_memory_so, &dest_memory_so)
        .unwrap_or_else(|e| panic!("failed to copy memory_plugin .so: {}", e));

    println!(
        "cargo:rustc-env=MEMORY_PLUGIN_SO={}",
        dest_memory_so.display()
    );

    // ─── error_plugin build ──────────────────────────────────────────────────
    // Re-run if error_plugin sources change.
    println!("cargo:rerun-if-changed=tests/fixtures/error_plugin/src/lib.rs");
    println!("cargo:rerun-if-changed=tests/fixtures/error_plugin/Cargo.toml");

    // Build error_plugin as a release cdylib via cargo.
    let error_status: std::process::ExitStatus = Command::new(env!("CARGO"))
        .arg("build")
        .arg("--package")
        .arg("error_plugin")
        .arg("--release")
        .arg("--target-dir")
        .arg(&plugin_target_dir)
        .current_dir(&workspace_root)
        .env_remove("CARGO_TARGET_DIR")
        .status()
        .expect("failed to run cargo build for error_plugin");

    if !error_status.success() {
        panic!(
            "cargo build -p error_plugin failed with status: {}",
            error_status
        );
    }

    let error_lib_filename: &str = if cfg!(target_os = "macos") {
        "liberror_plugin.dylib"
    } else if cfg!(target_os = "windows") {
        "error_plugin.dll"
    } else {
        "liberror_plugin.so"
    };

    let built_error_so: PathBuf = plugin_target_dir.join("release").join(error_lib_filename);
    let dest_error_so: PathBuf = fixtures_dir.join(error_lib_filename);
    fs::copy(&built_error_so, &dest_error_so)
        .unwrap_or_else(|e| panic!("failed to copy error_plugin .so: {}", e));

    println!(
        "cargo:rustc-env=ERROR_PLUGIN_SO={}",
        dest_error_so.display()
    );

    // ─── reload_plugin_v1 build ───────────────────────────────────────────────
    // Re-run if reload_plugin_v1 sources change.
    println!("cargo:rerun-if-changed=tests/fixtures/reload_plugin_v1/src/lib.rs");
    println!("cargo:rerun-if-changed=tests/fixtures/reload_plugin_v1/Cargo.toml");

    // Build reload_plugin_v1 as a release cdylib via cargo.
    let reload_v1_status: std::process::ExitStatus = Command::new(env!("CARGO"))
        .arg("build")
        .arg("--package")
        .arg("reload_plugin_v1")
        .arg("--release")
        .arg("--target-dir")
        .arg(&plugin_target_dir)
        .current_dir(&workspace_root)
        .env_remove("CARGO_TARGET_DIR")
        .status()
        .expect("failed to run cargo build for reload_plugin_v1");

    if !reload_v1_status.success() {
        panic!(
            "cargo build -p reload_plugin_v1 failed with status: {}",
            reload_v1_status
        );
    }

    let reload_v1_lib_filename: &str = if cfg!(target_os = "macos") {
        "libreload_plugin_v1.dylib"
    } else if cfg!(target_os = "windows") {
        "reload_plugin_v1.dll"
    } else {
        "libreload_plugin_v1.so"
    };

    let built_reload_v1_so: PathBuf = plugin_target_dir
        .join("release")
        .join(reload_v1_lib_filename);
    let dest_reload_v1_so: PathBuf = fixtures_dir.join(reload_v1_lib_filename);
    fs::copy(&built_reload_v1_so, &dest_reload_v1_so)
        .unwrap_or_else(|e| panic!("failed to copy reload_plugin_v1 .so: {}", e));

    println!(
        "cargo:rustc-env=RELOAD_PLUGIN_V1_SO={}",
        dest_reload_v1_so.display()
    );

    // Create bundle directory for reload_plugin_v1
    let reload_v1_dir: PathBuf = fixtures_dir.join("reload_plugin_v1");
    fs::create_dir_all(&reload_v1_dir)
        .unwrap_or_else(|e: std::io::Error| panic!("failed to create reload_v1 bundle dir: {}", e));
    fs::copy(
        &dest_reload_v1_so,
        reload_v1_dir.join(reload_v1_lib_filename),
    )
    .unwrap_or_else(|e: std::io::Error| {
        panic!("failed to copy reload_v1 .so to bundle dir: {}", e)
    });
    fs::write(
        reload_v1_dir.join("manifest.toml"),
        concat!(
            "id                         = 16808897324254478442\n",
            "name                       = \"reload_plugin_v1\"\n",
            "version                    = \"1.0\"\n",
            "runtime                    = \"native\"\n",
            "file                       = \"libreload_plugin_v1.so\"\n",
            "needs_reinit_on_dep_reload = false\n",
            "provides                   = [\"reload.test\"]\n",
            "\n",
            "[function_count]\n",
            "\"reload.test@1\" = 1\n",
        ),
    )
    .unwrap_or_else(|e: std::io::Error| panic!("failed to write reload_v1 manifest.toml: {}", e));
    println!(
        "cargo:rustc-env=RELOAD_PLUGIN_V1_DIR={}",
        reload_v1_dir.display()
    );
    println!("cargo:rustc-env=RELOAD_PLUGIN_CONTRACT_ID=16526955377754357857");

    // ─── reload_plugin_v2 build ───────────────────────────────────────────────
    // Re-run if reload_plugin_v2 sources change.
    println!("cargo:rerun-if-changed=tests/fixtures/reload_plugin_v2/src/lib.rs");
    println!("cargo:rerun-if-changed=tests/fixtures/reload_plugin_v2/Cargo.toml");

    // Build reload_plugin_v2 as a release cdylib via cargo.
    let reload_v2_status: std::process::ExitStatus = Command::new(env!("CARGO"))
        .arg("build")
        .arg("--package")
        .arg("reload_plugin_v2")
        .arg("--release")
        .arg("--target-dir")
        .arg(&plugin_target_dir)
        .current_dir(&workspace_root)
        .env_remove("CARGO_TARGET_DIR")
        .status()
        .expect("failed to run cargo build for reload_plugin_v2");

    if !reload_v2_status.success() {
        panic!(
            "cargo build -p reload_plugin_v2 failed with status: {}",
            reload_v2_status
        );
    }

    let reload_v2_lib_filename: &str = if cfg!(target_os = "macos") {
        "libreload_plugin_v2.dylib"
    } else if cfg!(target_os = "windows") {
        "reload_plugin_v2.dll"
    } else {
        "libreload_plugin_v2.so"
    };

    let built_reload_v2_so: PathBuf = plugin_target_dir
        .join("release")
        .join(reload_v2_lib_filename);
    let dest_reload_v2_so: PathBuf = fixtures_dir.join(reload_v2_lib_filename);
    fs::copy(&built_reload_v2_so, &dest_reload_v2_so)
        .unwrap_or_else(|e| panic!("failed to copy reload_plugin_v2 .so: {}", e));

    println!(
        "cargo:rustc-env=RELOAD_PLUGIN_V2_SO={}",
        dest_reload_v2_so.display()
    );

    // Create bundle directory for reload_plugin_v2
    let reload_v2_dir: PathBuf = fixtures_dir.join("reload_plugin_v2");
    fs::create_dir_all(&reload_v2_dir)
        .unwrap_or_else(|e: std::io::Error| panic!("failed to create reload_v2 bundle dir: {}", e));
    fs::copy(
        &dest_reload_v2_so,
        reload_v2_dir.join(reload_v2_lib_filename),
    )
    .unwrap_or_else(|e: std::io::Error| {
        panic!("failed to copy reload_v2 .so to bundle dir: {}", e)
    });
    fs::write(
        reload_v2_dir.join("manifest.toml"),
        concat!(
            "id                         = 16808897324254478442\n",
            "name                       = \"reload_plugin_v1\"\n",
            "version                    = \"2.0\"\n",
            "runtime                    = \"native\"\n",
            "file                       = \"libreload_plugin_v2.so\"\n",
            "needs_reinit_on_dep_reload = false\n",
            "provides                   = [\"reload.test\"]\n",
            "\n",
            "[function_count]\n",
            "\"reload.test@1\" = 1\n",
        ),
    )
    .unwrap_or_else(|e: std::io::Error| panic!("failed to write reload_v2 manifest.toml: {}", e));
    println!(
        "cargo:rustc-env=RELOAD_PLUGIN_V2_DIR={}",
        reload_v2_dir.display()
    );

    // ─── depender_plugin build ───────────────────────────────────────────────
    // Re-run if depender_plugin sources change.
    println!("cargo:rerun-if-changed=tests/fixtures/depender_plugin/src/lib.rs");
    println!("cargo:rerun-if-changed=tests/fixtures/depender_plugin/Cargo.toml");

    // Build depender_plugin as a release cdylib via cargo.
    let depender_status: std::process::ExitStatus = Command::new(env!("CARGO"))
        .arg("build")
        .arg("--package")
        .arg("depender_plugin")
        .arg("--release")
        .arg("--target-dir")
        .arg(&plugin_target_dir)
        .current_dir(&workspace_root)
        .env_remove("CARGO_TARGET_DIR")
        .status()
        .expect("failed to run cargo build for depender_plugin");

    if !depender_status.success() {
        panic!(
            "cargo build -p depender_plugin failed with status: {}",
            depender_status
        );
    }

    let depender_lib_filename: &str = if cfg!(target_os = "macos") {
        "libdepender_plugin.dylib"
    } else if cfg!(target_os = "windows") {
        "depender_plugin.dll"
    } else {
        "libdepender_plugin.so"
    };

    let built_depender_so: PathBuf = plugin_target_dir
        .join("release")
        .join(depender_lib_filename);
    let dest_depender_so: PathBuf = fixtures_dir.join(depender_lib_filename);
    fs::copy(&built_depender_so, &dest_depender_so)
        .unwrap_or_else(|e| panic!("failed to copy depender_plugin .so: {}", e));

    println!(
        "cargo:rustc-env=DEPENDER_PLUGIN_SO={}",
        dest_depender_so.display()
    );

    // Create bundle directory for depender_plugin
    let depender_dir: PathBuf = fixtures_dir.join("depender_plugin");
    fs::create_dir_all(&depender_dir)
        .unwrap_or_else(|e: std::io::Error| panic!("failed to create depender bundle dir: {}", e));
    fs::copy(&dest_depender_so, depender_dir.join(depender_lib_filename)).unwrap_or_else(
        |e: std::io::Error| panic!("failed to copy depender .so to bundle dir: {}", e),
    );
    fs::write(
        depender_dir.join("manifest.toml"),
        concat!(
            "bundle_name                = \"depender_plugin\"\n",
            "version                    = \"1.0\"\n",
            "runtime                    = \"native\"\n",
            "file                       = \"libdepender_plugin.so\"\n",
            "needs_reinit_on_dep_reload = true\n",
            "provides                   = [\"depender.test\"]\n",
            "\n",
            "[function_count]\n",
            "\"depender.test@1\" = 1\n",
            "\n",
            "[[dependency]]\n",
            "kind        = \"bundle\"\n",
            "contract    = \"reload.test@1\"\n",
            "min_version = \"1.0\"\n",
            "bundle      = \"reload_plugin_v1\"\n",
            "contract_id = 16526955377754357857\n",
            "bundle_id   = 16808897324254478442\n",
        ),
    )
    .unwrap_or_else(|e: std::io::Error| panic!("failed to write depender manifest.toml: {}", e));
    println!(
        "cargo:rustc-env=DEPENDER_PLUGIN_DIR={}",
        depender_dir.display()
    );

    // ─── no_init_plugin build ─────────────────────────────────────────────────
    // Re-run if no_init_plugin sources change.
    println!("cargo:rerun-if-changed=tests/fixtures/no_init_plugin/src/lib.rs");
    println!("cargo:rerun-if-changed=tests/fixtures/no_init_plugin/Cargo.toml");

    // Build no_init_plugin as a release cdylib via cargo.
    let no_init_status: std::process::ExitStatus = Command::new(env!("CARGO"))
        .arg("build")
        .arg("--package")
        .arg("no_init_plugin")
        .arg("--release")
        .arg("--target-dir")
        .arg(&plugin_target_dir)
        .current_dir(&workspace_root)
        .env_remove("CARGO_TARGET_DIR")
        .status()
        .expect("failed to run cargo build for no_init_plugin");

    if !no_init_status.success() {
        panic!(
            "cargo build -p no_init_plugin failed with status: {}",
            no_init_status
        );
    }

    let no_init_lib_filename: &str = if cfg!(target_os = "macos") {
        "libno_init_plugin.dylib"
    } else if cfg!(target_os = "windows") {
        "no_init_plugin.dll"
    } else {
        "libno_init_plugin.so"
    };

    let built_no_init_so: PathBuf = plugin_target_dir.join("release").join(no_init_lib_filename);
    let dest_no_init_so: PathBuf = fixtures_dir.join(no_init_lib_filename);
    fs::copy(&built_no_init_so, &dest_no_init_so)
        .unwrap_or_else(|e| panic!("failed to copy no_init_plugin .so: {}", e));

    // Create bundle directory for no_init_plugin.
    let no_init_plugin_dir: PathBuf = fixtures_dir.join("no_init_plugin");
    fs::create_dir_all(&no_init_plugin_dir).unwrap_or_else(|e: std::io::Error| {
        panic!("failed to create no_init_plugin bundle dir: {}", e)
    });
    fs::copy(
        &dest_no_init_so,
        no_init_plugin_dir.join(no_init_lib_filename),
    )
    .unwrap_or_else(|e: std::io::Error| {
        panic!("failed to copy no_init_plugin .so to bundle dir: {}", e)
    });
    fs::write(
        no_init_plugin_dir.join("manifest.toml"),
        concat!(
            "bundle_name = \"no_init_plugin\"\n",
            "runtime     = \"native\"\n",
            "file        = \"libno_init_plugin.so\"\n",
        ),
    )
    .unwrap_or_else(|e: std::io::Error| {
        panic!("failed to write no_init_plugin manifest.toml: {}", e)
    });
    println!(
        "cargo:rustc-env=NO_INIT_PLUGIN_DIR={}",
        no_init_plugin_dir.display()
    );

    // Emit polyplugc binary path so integration tests can use env!("CARGO_BIN_EXE_polyplugc").
    // polyplugc lives in the same workspace target directory.
    let polyplugc_filename: &str = if cfg!(target_os = "windows") {
        "polyplugc.exe"
    } else {
        "polyplugc"
    };
    // Determine the profile name from OUT_DIR. OUT_DIR is:
    //   <target>/<profile>/build/<crate>-<hash>/out
    // We want <target>/<profile>/polyplugc.
    let profile: &str = if out_dir.components().any(|c| c.as_os_str() == "release") {
        "release"
    } else {
        "debug"
    };
    let polyplugc_path: PathBuf = target_dir.join(profile).join(polyplugc_filename);
    println!(
        "cargo:rustc-env=CARGO_BIN_EXE_polyplugc={}",
        polyplugc_path.display()
    );
    println!("cargo:rerun-if-changed=../../crates/polyplugc/src");

    // ─── C++ test plugin compilation ─────────────────────────────────────────────────
    println!("cargo:rerun-if-changed=crates/polyplug/build.rs");

    // Check if g++ is available.
    let gpp_available: bool = Command::new("g++")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !gpp_available {
        println!("cargo:warning=g++ not available; skipping C++ test plugin build");
        println!("cargo:rustc-env=TEST_PLUGIN_CPP_SO=");
        println!("cargo:rustc-env=TEST_PLUGIN_CPP_THROW_SO=");
    } else {
        // Write C++ plugin source to OUT_DIR
        let cpp_src: &str = r#"// THIS FILE IS AUTO-GENERATED BY polyplug build.rs — test only
#include <cstdint>
#include <cstddef>
// Mirror ABI types inline (no header dependency)
struct StringView { const uint8_t* ptr; size_t len; };
struct AbiError { uint32_t code; StringView message; };
struct PluginHandle { uint32_t index; uint32_t generation; };
struct PluginVTable { uint64_t contract_id; uint32_t contract_version; uint32_t function_count; void* const* functions; };
struct PluginDescriptor { StringView name; StringView contract_name; uint32_t version_major; uint32_t version_minor; uint32_t version_patch; };
struct PluginRegistrar {
    AbiError (*register_plugin)(PluginRegistrar*, const PluginDescriptor*, const PluginVTable*);
    const void* host;
};
constexpr uint32_t ABI_OK = 0;
// test.add contract_id = FNV-1a("test.add@1") = 0xCC4232FAB0410D2B
constexpr uint64_t TEST_ADD_CONTRACT_ID = 0xCC4232FAB0410D2BULL;
struct AddArgs { uint32_t a; uint32_t b; };

extern "C" AbiError cpp_test_add(const void* args, void* out) {
    const AddArgs* add_args = static_cast<const AddArgs*>(args);
    uint32_t result = add_args->a + add_args->b;
    *static_cast<uint32_t*>(out) = result;
    AbiError ok{};
    ok.code = ABI_OK;
    ok.message.ptr = nullptr;
    ok.message.len = 0;
    return ok;
}

static void* const CPP_TEST_ADD_FNS[] = { reinterpret_cast<void*>(cpp_test_add) };
static PluginVTable CPP_TEST_ADD_VTABLE = { TEST_ADD_CONTRACT_ID, 1U << 16, 1, CPP_TEST_ADD_FNS };
static PluginDescriptor CPP_TEST_ADD_DESC = {
    { (const uint8_t*)"cpp_test_adder", 14 },
    { (const uint8_t*)"test.add", 8 },
    1, 0, 0
};

extern "C" uint32_t polyplug_abi_version() { return 1; }
extern "C" AbiError polyplug_init(PluginRegistrar* registrar) {
    return registrar->register_plugin(registrar, &CPP_TEST_ADD_DESC, &CPP_TEST_ADD_VTABLE);
}
"#;

        let cpp_src_path: PathBuf = out_dir.join("test_plugin_cpp.cpp");
        std::fs::write(&cpp_src_path, cpp_src).expect("failed to write C++ test plugin source");

        let cpp_so_filename: &str = if cfg!(target_os = "macos") {
            "libtest_plugin_cpp.dylib"
        } else {
            "libtest_plugin_cpp.so"
        };

        let cpp_so_out: PathBuf = out_dir.join(cpp_so_filename);
        let cpp_compile_status: std::process::ExitStatus = Command::new("g++")
            .arg("-std=c++17")
            .arg("-shared")
            .arg("-fPIC")
            .arg("-o")
            .arg(&cpp_so_out)
            .arg(&cpp_src_path)
            .status()
            .expect("failed to run g++");

        if !cpp_compile_status.success() {
            panic!("g++ compilation of C++ test plugin failed");
        }

        let cpp_dest_so: PathBuf = fixtures_dir.join(cpp_so_filename);
        fs::copy(&cpp_so_out, &cpp_dest_so)
            .expect("failed to copy C++ test plugin .so to fixtures/");

        println!(
            "cargo:rustc-env=TEST_PLUGIN_CPP_SO={}",
            cpp_dest_so.display()
        );

        // ─── Throwing C++ plugin ──────────────────────────────────────────────────
        let cpp_throw_src: &str = r#"// THIS FILE IS AUTO-GENERATED BY polyplug build.rs — test only
#include <cstdint>
#include <cstddef>
#include <stdexcept>
struct StringView { const uint8_t* ptr; size_t len; };
struct AbiError { uint32_t code; StringView message; };
struct PluginHandle { uint32_t index; uint32_t generation; };
struct PluginVTable { uint64_t contract_id; uint32_t contract_version; uint32_t function_count; void* const* functions; };
struct PluginDescriptor { StringView name; StringView contract_name; uint32_t version_major; uint32_t version_minor; uint32_t version_patch; };
struct PluginRegistrar {
    AbiError (*register_plugin)(PluginRegistrar*, const PluginDescriptor*, const PluginVTable*);
    const void* host;
};
constexpr uint32_t ABI_OK = 0;
constexpr uint32_t ABI_ERROR_GENERIC = 1;
// test.add contract_id = FNV-1a("test.add@1") = 0xCC4232FAB0410D2B
constexpr uint64_t TEST_ADD_CONTRACT_ID = 0xCC4232FAB0410D2BULL;
struct AddArgs { uint32_t a; uint32_t b; };

// Simulates the try/catch pattern that generated vtables.hpp emits.
extern "C" AbiError cpp_throw_abi(const void* args, void* out) noexcept {
    try {
        (void)args; (void)out;
        throw std::runtime_error("test exception from C++ plugin");
    } catch (const std::exception&) {
        AbiError err{};
        err.code = ABI_ERROR_GENERIC;
        err.message.ptr = nullptr;
        err.message.len = 0;
        return err;
    } catch (...) {
        AbiError err{};
        err.code = 3;  // ABI_ERROR_PANIC
        err.message.ptr = nullptr;
        err.message.len = 0;
        return err;
    }
}

static void* const CPP_THROW_FNS[] = { reinterpret_cast<void*>(cpp_throw_abi) };
static PluginVTable CPP_THROW_VTABLE = { TEST_ADD_CONTRACT_ID, 1U << 16, 1, CPP_THROW_FNS };
static PluginDescriptor CPP_THROW_DESC = {
    { (const uint8_t*)"cpp_throw_plugin", 16 },
    { (const uint8_t*)"test.add", 8 },
    1, 0, 0
};
extern "C" uint32_t polyplug_abi_version() { return 1; }
extern "C" AbiError polyplug_init(PluginRegistrar* registrar) {
    return registrar->register_plugin(registrar, &CPP_THROW_DESC, &CPP_THROW_VTABLE);
}
"#;

        let cpp_throw_src_path: PathBuf = out_dir.join("test_plugin_cpp_throw.cpp");
        std::fs::write(&cpp_throw_src_path, cpp_throw_src)
            .expect("failed to write throwing C++ plugin source");

        let cpp_throw_so_filename: &str = if cfg!(target_os = "macos") {
            "libtest_plugin_cpp_throw.dylib"
        } else {
            "libtest_plugin_cpp_throw.so"
        };

        let cpp_throw_so_out: PathBuf = out_dir.join(cpp_throw_so_filename);
        let cpp_throw_compile_status: std::process::ExitStatus = Command::new("g++")
            .arg("-std=c++17")
            .arg("-shared")
            .arg("-fPIC")
            .arg("-o")
            .arg(&cpp_throw_so_out)
            .arg(&cpp_throw_src_path)
            .status()
            .expect("failed to run g++ for throwing plugin");

        if !cpp_throw_compile_status.success() {
            panic!("g++ compilation of throwing C++ test plugin failed");
        }

        let cpp_throw_dest_so: PathBuf = fixtures_dir.join(cpp_throw_so_filename);
        fs::copy(&cpp_throw_so_out, &cpp_throw_dest_so)
            .expect("failed to copy throwing C++ plugin .so to fixtures/");

        println!(
            "cargo:rustc-env=TEST_PLUGIN_CPP_THROW_SO={}",
            cpp_throw_dest_so.display()
        );
    }

    // ── C# fixture plugin (dotnet build) ──────────────────────────────────────
    let csharp_csproj: PathBuf =
        workspace_root.join("tests/fixtures/csharp_plugin/CsharpPlugin.csproj");
    let csharp_out_dir: PathBuf =
        workspace_root.join("tests/fixtures/csharp_plugin/bin/Debug/net10.0");

    // Guard: only run dotnet build if the .csproj file exists AND dotnet is available on PATH
    let dotnet_available: bool = csharp_csproj.exists()
        && Command::new("dotnet")
            .arg("--version")
            .output()
            .map(|o: std::process::Output| o.status.success())
            .unwrap_or(false);

    if dotnet_available {
        let dotnet_status: std::process::ExitStatus = Command::new("dotnet")
            .arg("build")
            .arg(&csharp_csproj)
            .arg("--configuration")
            .arg("Debug")
            .arg("--nologo")
            .status()
            .expect("dotnet build failed to spawn");
        if !dotnet_status.success() {
            panic!("dotnet build failed for CsharpPlugin.csproj");
        }
        println!(
            "cargo:rustc-env=TEST_CSHARP_PLUGIN_DLL={}",
            csharp_out_dir.join("CsharpPlugin.dll").display()
        );
    } else {
        // No dotnet — emit a dummy env var so test code can skip gracefully
        println!("cargo:rustc-env=TEST_CSHARP_PLUGIN_DLL=DOTNET_NOT_AVAILABLE");
    }
    println!("cargo:rerun-if-changed=tests/fixtures/csharp_plugin/Plugin.cs");
    println!("cargo:rerun-if-changed=tests/fixtures/csharp_plugin/CsharpPlugin.csproj");

    // ─── Python fixture availability check ──────────────────────────────────────
    println!("cargo:rerun-if-changed=tests/fixtures/test_plugin.py");

    // Check if python3 is available.
    let python_available: bool = Command::new("python3")
        .arg("--version")
        .output()
        .map(|o: std::process::Output| o.status.success())
        .unwrap_or(false);

    // Locate the polyplug shared library (built in this same invocation) so that
    // integration_python tests can load it. The .so lives at:
    //   <target_dir>/<profile>/libpolyplug.so
    // We copy it to tests/fixtures/ with a stable name.
    let polyplug_lib_filename: &str = if cfg!(target_os = "macos") {
        "libpolyplug.dylib"
    } else if cfg!(target_os = "windows") {
        "polyplug.dll"
    } else {
        "libpolyplug.so"
    };

    let polyplug_so_src: PathBuf = target_dir.join(profile).join(polyplug_lib_filename);
    let polyplug_so_dest: PathBuf = fixtures_dir.join(polyplug_lib_filename);

    if polyplug_so_src.exists() {
        fs::copy(&polyplug_so_src, &polyplug_so_dest).unwrap_or_else(|e: std::io::Error| {
            panic!(
                "failed to copy {} to {}: {}",
                polyplug_so_src.display(),
                polyplug_so_dest.display(),
                e
            )
        });
        println!("cargo:rustc-env=POLYPLUG_SO={}", polyplug_so_dest.display());
    } else {
        // polyplug.so might not be built yet in the very first incremental build.
        // Emit empty env var; integration tests will skip if POLYPLUG_SO is empty.
        println!("cargo:rustc-env=POLYPLUG_SO=");
    }

    if python_available {
        println!(
            "cargo:rustc-env=TEST_PYTHON_PLUGIN={}",
            fixtures_dir.join("test_plugin.py").display()
        );
    } else {
        println!("cargo:rustc-env=TEST_PYTHON_PLUGIN=PYTHON_NOT_AVAILABLE");
    }
    // ─── Lua fixture ──────────────────────────────────────────────────────────────
    println!(
        "cargo:rerun-if-changed={}",
        fixtures_dir.join("test_plugin.lua").display()
    );
    // mlua with `vendored` embeds LuaJIT — no system install required.
    // Always emit the real fixture path.
    // Tests can opt-out via SKIP_LUA=1 env var at runtime.
    println!(
        "cargo:rustc-env=TEST_LUA_PLUGIN={}",
        fixtures_dir.join("test_plugin.lua").display()
    );

    println!(
        "cargo:rerun-if-changed={}",
        fixtures_dir
            .join("test_plugin_js")
            .join("bundle.js")
            .display()
    );
    println!(
        "cargo:rustc-env=TEST_JS_PLUGIN={}",
        fixtures_dir.join("test_plugin_js").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        fixtures_dir
            .join("test_plugin_js_deno")
            .join("index.ts")
            .display()
    );
    println!(
        "cargo:rustc-env=TEST_JS_DENO_PLUGIN={}",
        fixtures_dir.join("test_plugin_js_deno").display()
    );
}
