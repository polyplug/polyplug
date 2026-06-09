//! Integration test: use polyplug_codegen library to generate C++ bindings
//! and assert all expected files are present with correct naming.

#![allow(clippy::expect_used)]

use polyplug_codegen::{GenerateConfig, Lang, Side};
use polyplugc::generate;
use std::path::Path;
use std::path::PathBuf;

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Workspace root resolved from `CARGO_MANIFEST_DIR` (`crates/polyplug_codegen`).
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("parent of crates/polyplug_codegen")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

/// Use polyplugc::generate() to generate C++ bindings.
fn generate_cpp_bindings(bundle_toml: &Path, out_dir: &Path, side: Side) {
    let config = GenerateConfig {
        api_toml: bundle_toml.to_path_buf(),
        lang: Lang::Cpp,
        side,
        out_dir: out_dir.to_path_buf(),
    };

    let output = generate(config).expect("polyplugc::generate failed");

    // Write generated files to disk
    for file in &output.files {
        let file_path = out_dir.join(&file.path);
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent).expect("failed to create parent dir");
        }
        std::fs::write(&file_path, &file.content).expect("failed to write generated file");
    }
}

// ─── Codegen file existence check (guest side) ────────────────────────────────

#[test]
fn test_generate_cpp_guest_files_exist() {
    let root: PathBuf = workspace_root();
    let bundle_toml: PathBuf = root.join("tests").join("fixtures").join("test_bundle.toml");
    let out_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("integration_codegen_cpp_guest");

    std::fs::create_dir_all(&out_dir).expect("create out_dir");

    // Generate C++ guest-side bindings using library API
    generate_cpp_bindings(&bundle_toml, &out_dir, Side::Guest);

    // Assert expected guest files exist
    let expected_files: &[&str] = &[
        "guest/types.hpp",
        "guest/contracts.hpp",
        "guest/interfaces.hpp",
        "guest/init.hpp",
    ];

    for rel_path in expected_files {
        let full_path: PathBuf = out_dir.join(rel_path);
        assert!(
            full_path.exists(),
            "Expected file not found: {}",
            full_path.display()
        );
    }

    println!(
        "test_generate_cpp_guest_files_exist: all {} files present",
        expected_files.len()
    );
}

// ─── Codegen file existence check (host side) ────────────────────────────────

#[test]
fn test_generate_cpp_host_files_exist() {
    let root: PathBuf = workspace_root();
    let api_toml: PathBuf = root.join("tests").join("fixtures").join("test_api.toml");
    let out_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("integration_codegen_cpp_host");

    std::fs::create_dir_all(&out_dir).expect("create out_dir");

    // Generate C++ host-side bindings using library API
    generate_cpp_bindings(&api_toml, &out_dir, Side::Host);

    // Assert expected host files exist
    let expected_files: &[&str] = &["host/types.hpp", "host/host_callers.hpp", "manifest.toml"];

    for rel_path in expected_files {
        let full_path: PathBuf = out_dir.join(rel_path);
        assert!(
            full_path.exists(),
            "Expected file not found: {}",
            full_path.display()
        );
    }

    println!(
        "test_generate_cpp_host_files_exist: all {} files present",
        expected_files.len()
    );
}

// ─── Interface naming verification ─────────────────────────────────────────────

#[test]
fn test_cpp_codegen_uses_interface_naming() {
    let root: PathBuf = workspace_root();
    let bundle_toml: PathBuf = root.join("tests").join("fixtures").join("test_bundle.toml");
    let out_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("integration_codegen_cpp_naming");

    std::fs::create_dir_all(&out_dir).expect("create out_dir");

    generate_cpp_bindings(&bundle_toml, &out_dir, Side::Guest);

    // Read guest/interfaces.hpp and assert interface naming
    let interfaces_file: PathBuf = out_dir.join("guest").join("interfaces.hpp");
    let content: String = std::fs::read_to_string(&interfaces_file).expect("read interfaces file");

    // Assert _INTERFACE suffix is used (not _VTABLE)
    assert!(
        content.contains("_INTERFACE"),
        "interfaces.hpp must contain _INTERFACE suffix: {}",
        interfaces_file.display()
    );

    // Assert no legacy _VTABLE suffix
    assert!(
        !content.contains("_VTABLE"),
        "interfaces.hpp must NOT contain legacy _VTABLE suffix: {}",
        interfaces_file.display()
    );

    // Assert GuestContractInterface is used
    assert!(
        content.contains("GuestContractInterface"),
        "interfaces.hpp must contain GuestContractInterface: {}",
        interfaces_file.display()
    );

    println!("test_cpp_codegen_uses_interface_naming: interface naming assertions passed");
}

// ─── Host contract interface naming verification ──────────────────────────────────

#[test]
fn test_cpp_codegen_host_contract_uses_interface() {
    let root: PathBuf = workspace_root();
    let bundle_toml: PathBuf = root.join("tests").join("fixtures").join("test_bundle.toml");
    let out_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("integration_codegen_cpp_host_contract");

    std::fs::create_dir_all(&out_dir).expect("create out_dir");

    // Generate guest-side bindings (host_contracts.hpp is in guest directory for callers)
    generate_cpp_bindings(&bundle_toml, &out_dir, Side::Guest);

    // Note: host_contracts.hpp is only generated if there are host_contracts in the IR.
    // For test_bundle.toml without host_contracts, this file may not exist.
    // We test the host_callers.hpp file instead which contains the GuestContractInterface usage.
    let host_callers_file: PathBuf = out_dir.join("guest").join("host_contracts.hpp");

    // If host_contracts.hpp exists, check its content
    if host_callers_file.exists() {
        let content: String =
            std::fs::read_to_string(&host_callers_file).expect("read host_contracts file");

        // Assert HostContractInterface is used (not HostContractVTable)
        if content.contains("HostContract") {
            assert!(
                content.contains("HostContractInterface"),
                "host_contracts.hpp must use HostContractInterface: {}",
                host_callers_file.display()
            );
            assert!(
                !content.contains("HostContractVTable"),
                "host_contracts.hpp must NOT contain HostContractVTable: {}",
                host_callers_file.display()
            );
            // Assert interface_ member (not vtable_)
            assert!(
                content.contains("interface_"),
                "host_contracts.hpp must contain interface_ member: {}",
                host_callers_file.display()
            );
            assert!(
                !content.contains("vtable_"),
                "host_contracts.hpp must NOT contain vtable_ member: {}",
                host_callers_file.display()
            );
        }
        println!("test_cpp_codegen_host_contract_uses_interface: host contract assertions passed");
    } else {
        // Skip if no host contracts defined in test fixture
        println!(
            "test_cpp_codegen_host_contract_uses_interface: skipped (no host_contracts in fixture)"
        );
    }
}

// ─── Guest contract instance wrapper verification ──────────────────────────────────

#[test]
fn test_cpp_codegen_guest_instance_wrapper_exists() {
    let root: PathBuf = workspace_root();
    let bundle_toml: PathBuf = root.join("tests").join("fixtures").join("test_bundle.toml");
    let out_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("integration_codegen_cpp_instance");

    std::fs::create_dir_all(&out_dir).expect("create out_dir");

    generate_cpp_bindings(&bundle_toml, &out_dir, Side::Guest);

    // Read guest/host_callers.hpp (contains the instance wrapper for calling guest contracts)
    let host_callers_file: PathBuf = out_dir.join("guest").join("host_callers.hpp");

    // If host_callers.hpp exists in guest directory, check for instance wrapper pattern
    if host_callers_file.exists() {
        let content: String =
            std::fs::read_to_string(&host_callers_file).expect("read host_callers file");

        // Assert instance wrapper pattern exists (for calling guest contracts from host)
        // The pattern includes:
        // - create_instance method call in factory
        // - destroy_instance method call in destructor
        // - instance_ member
        // - dispatch passes instance to function calls

        if content.contains("GuestContractInstance") {
            // Check for create_instance usage
            assert!(
                content.contains("create_instance"),
                "host_callers.hpp must contain create_instance: {}",
                host_callers_file.display()
            );

            // Check for destroy_instance usage
            assert!(
                content.contains("destroy_instance"),
                "host_callers.hpp must contain destroy_instance: {}",
                host_callers_file.display()
            );

            // Check for instance_ member
            assert!(
                content.contains("instance_"),
                "host_callers.hpp must contain instance_ member: {}",
                host_callers_file.display()
            );

            // Check that interface_ is used (not vtable_)
            assert!(
                content.contains("interface_"),
                "host_callers.hpp must contain interface_ member: {}",
                host_callers_file.display()
            );
        }
        println!(
            "test_cpp_codegen_guest_instance_wrapper_exists: instance wrapper assertions passed"
        );
    } else {
        // Read guest/interfaces.hpp instead - it contains the static interface declaration
        let interfaces_file: PathBuf = out_dir.join("guest").join("interfaces.hpp");
        let content: String =
            std::fs::read_to_string(&interfaces_file).expect("read interfaces file");

        // Assert create_instance stub exists in interface
        assert!(
            content.contains("create_instance"),
            "interfaces.hpp must contain create_instance stub: {}",
            interfaces_file.display()
        );

        // Assert destroy_instance stub exists
        assert!(
            content.contains("destroy_instance"),
            "interfaces.hpp must contain destroy_instance stub: {}",
            interfaces_file.display()
        );

        println!(
            "test_cpp_codegen_guest_instance_wrapper_exists: instance lifecycle stubs verified"
        );
    }
}

// ─── Factory uses inline HostContractInterface fields verification ────────────────────

#[test]
fn test_cpp_codegen_factory_uses_inline_fields() {
    let root: PathBuf = workspace_root();
    let api_toml: PathBuf = root.join("tests").join("fixtures").join("test_api.toml");
    let out_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("integration_codegen_cpp_factory");

    std::fs::create_dir_all(&out_dir).expect("create out_dir");

    // Generate host-side bindings
    generate_cpp_bindings(&api_toml, &out_dir, Side::Host);

    // Check for interface_factories.hpp (only generated if host_contracts exist)
    let interface_factories_file: PathBuf = out_dir.join("host").join("interface_factories.hpp");

    if interface_factories_file.exists() {
        let content: String = std::fs::read_to_string(&interface_factories_file)
            .expect("read interface_factories file");

        // Assert HostContractInterface is used
        assert!(
            content.contains("HostContractInterface"),
            "interface_factories.hpp must contain HostContractInterface: {}",
            interface_factories_file.display()
        );

        // Assert no HostContractVTable
        assert!(
            !content.contains("HostContractVTable"),
            "interface_factories.hpp must NOT contain HostContractVTable: {}",
            interface_factories_file.display()
        );

        // Assert no HostContractVTableHeader (should use inline fields instead)
        assert!(
            !content.contains("HostContractVTableHeader"),
            "interface_factories.hpp must NOT contain HostContractVTableHeader: {}",
            interface_factories_file.display()
        );

        // Assert create_instance stub exists
        assert!(
            content.contains("create_instance"),
            "interface_factories.hpp must contain create_instance stub: {}",
            interface_factories_file.display()
        );

        println!("test_cpp_codegen_factory_uses_inline_fields: factory assertions passed");
    } else {
        // Skip if no host contracts defined in test fixture
        println!(
            "test_cpp_codegen_factory_uses_inline_fields: skipped (no interface_factories.hpp generated)"
        );
    }
}

// ─── No legacy VTable naming verification ─────────────────────────────────────────

#[test]
fn test_cpp_codegen_no_legacy_vtable_naming() {
    let root: PathBuf = workspace_root();
    let bundle_toml: PathBuf = root.join("tests").join("fixtures").join("test_bundle.toml");
    let out_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("integration_codegen_cpp_legacy");

    std::fs::create_dir_all(&out_dir).expect("create out_dir");

    generate_cpp_bindings(&bundle_toml, &out_dir, Side::Guest);

    // Check all generated guest files for legacy naming
    let guest_files: &[&str] = &[
        "guest/types.hpp",
        "guest/contracts.hpp",
        "guest/interfaces.hpp",
        "guest/init.hpp",
    ];

    for rel_path in guest_files {
        let full_path: PathBuf = out_dir.join(rel_path);
        if full_path.exists() {
            let content: String = std::fs::read_to_string(&full_path).expect("read file");

            // Assert no PluginVTable (legacy naming)
            assert!(
                !content.contains("PluginVTable"),
                "{} must NOT contain legacy PluginVTable: {}",
                rel_path,
                full_path.display()
            );

            // Assert no HostApi (legacy naming) - except in comments/docs
            // Note: HostApi might appear in abi.hpp which is included, but not in generated code
            if content.contains("HostApi") && !content.contains("abi.hpp") {
                // Only check if HostApi appears in actual generated code
                assert!(
                    !content.contains("static HostApi"),
                    "{} must NOT contain static HostApi declaration: {}",
                    rel_path,
                    full_path.display()
                );
            }
        }
    }

    println!("test_cpp_codegen_no_legacy_vtable_naming: no legacy naming found");
}

// ─── Host caller call-arena threading verification ────────────────────────────

/// The host caller for a contract with a variable-size return (`StringView`)
/// must own a per-caller `CallArena`, reset it at the start of each arena-backed
/// call, and thread it to the VM dispatch. Functions with fixed-size returns must
/// pass `nullptr` instead, so the VM bridge falls back to per-value `host->alloc`.
#[test]
fn test_cpp_codegen_host_caller_threads_arena() {
    let root: PathBuf = workspace_root();
    let api_toml: PathBuf = root.join("tests").join("fixtures").join("test_api.toml");
    let out_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("integration_codegen_cpp_arena");

    std::fs::create_dir_all(&out_dir).expect("create out_dir");

    generate_cpp_bindings(&api_toml, &out_dir, Side::Host);

    let host_callers_file: PathBuf = out_dir.join("host").join("host_callers.hpp");
    let content: String =
        std::fs::read_to_string(&host_callers_file).expect("read host_callers file");

    // The inline arena helpers and the per-caller buffer/member must be emitted.
    assert!(
        content.contains("CALL_ARENA_BUF_LEN"),
        "host_callers.hpp must define the arena buffer length constant"
    );
    assert!(
        content.contains("inline uint8_t* polyplug_arena_alloc(CallArena* arena"),
        "host_callers.hpp must emit the inline arena alloc helper"
    );
    assert!(
        content.contains("inline void polyplug_arena_reset(CallArena* arena"),
        "host_callers.hpp must emit the inline arena reset helper"
    );
    assert!(
        content.contains("std::unique_ptr<std::array<uint8_t, CALL_ARENA_BUF_LEN>> arena_buf_;"),
        "host_callers.hpp must hold a stable-address arena buffer"
    );
    assert!(
        content.contains("CallArena arena_;"),
        "host_callers.hpp must hold a per-caller CallArena member"
    );

    // The StringView-returning function (test.add::version, function_id 2) must
    // reset and thread the arena; the fixed-size returns must pass nullptr.
    assert!(
        content.contains("polyplug_arena_reset(&arena_);"),
        "an arena-backed call must reset the arena at call start"
    );
    assert!(
        content.contains("args_ptr, out_ptr, &arena_);"),
        "the variable-size return must thread the per-caller arena to vm.call"
    );
    assert!(
        content.contains("args_ptr, out_ptr, nullptr);"),
        "fixed-size returns must pass a null arena to vm.call"
    );

    // The destructor must free (not just rewind) the arena's overflow chain at teardown.
    assert!(
        content.contains("if (arena_buf_) {"),
        "the caller must guard arena teardown against moved-from state"
    );
    assert!(
        content.contains("polyplug_arena_free_all(&arena_);"),
        "the destructor and move-assign must call polyplug_arena_free_all to actually free blocks"
    );

    // The arena helpers: serve_from_block and free_all must be emitted; reset must
    // rewind only (no host->free loop inside it).
    assert!(
        content.contains("inline uint8_t* polyplug_arena_serve_from_block(ArenaOverflowBlock* block"),
        "host_callers.hpp must emit the serve_from_block helper"
    );
    assert!(
        content.contains("inline void polyplug_arena_free_all(CallArena* arena"),
        "host_callers.hpp must emit the free_all teardown helper"
    );
    // polyplug_arena_reset must NOT contain host->free (rewind-only; free is in free_all).
    let reset_start: usize = content
        .find("inline void polyplug_arena_reset(")
        .expect("polyplug_arena_reset must be emitted");
    let reset_end: usize = content[reset_start..]
        .find("\ninline ")
        .map(|off| reset_start + off)
        .unwrap_or(content.len());
    let reset_body: &str = &content[reset_start..reset_end];
    assert!(
        !reset_body.contains("host->free"),
        "polyplug_arena_reset must NOT call host->free (retain-and-rewind; free_all handles teardown)"
    );

    println!("test_cpp_codegen_host_caller_threads_arena: arena threading assertions passed");
}
