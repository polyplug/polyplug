//! Tests for host-side vtable factory generation.
//!
//! Verifies that the Rust generator produces valid vtable factories for host contracts:
//!   1. Both NATIVE and VM factory functions are generated
//!   2. Vtable header contains correct contract_id
//!   3. Vtable header contains correct function_count
//!   4. Thunks have panic safety via catch_unwind

#![allow(clippy::expect_used)]

use polyplug_abi::host_contract_id;
use polyplug_codegen::{generate, GenerateConfig, Lang, Side};
use std::path::PathBuf;

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Create a test API TOML with a host contract containing multiple functions.
fn create_test_api_with_host_contract(tmp_dir: &PathBuf) -> PathBuf {
    std::fs::create_dir_all(tmp_dir).expect("create tmp_dir");
    let api_toml_path: PathBuf = tmp_dir.join("test_vtable_api.toml");
    let content: &str = r#"# Test API with host contract for vtable factory tests
[[plugin_contract]]
name = "example.worker"
version = "1.0.0"

[[plugin_contract.functions]]
name = "do_work"
params = [{ name = "input", type = "StringView" }]
return = "StringView"

[[host_contract]]
name = "host.logger"
version = "1.0.0"

[[host_contract.functions]]
name = "log"
params = [{ name = "message", type = "StringView" }]
returns = "void"

[[host_contract.functions]]
name = "log_level"
params = [{ name = "message", type = "StringView" }, { name = "level", type = "u32" }]
returns = "void"

[[host_contract.functions]]
name = "get_level"
returns = "u32"
"#;
    std::fs::write(&api_toml_path, content).expect("failed to write test api.toml");
    api_toml_path
}

/// Generate host-side Rust code and return the vtable_factories.rs content.
fn generate_host_vtable_factories(tmp_dir: &PathBuf) -> String {
    let api_toml: PathBuf = create_test_api_with_host_contract(tmp_dir);

    let config: GenerateConfig = GenerateConfig {
        api_toml: api_toml.clone(),
        lang: Lang::Rust,
        side: Side::Host,
        out_dir: tmp_dir.clone(),
    };

    let output = generate(config).expect("polyplug_codegen::generate failed");

    // Write all generated files to disk
    for file in &output.files {
        let file_path: PathBuf = tmp_dir.join(&file.path);
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent).expect("failed to create parent dir");
        }
        std::fs::write(&file_path, &file.content).expect("failed to write generated file");
    }

    // Read the vtable_factories.rs file
    let vtable_factories_path: PathBuf = tmp_dir.join("host").join("vtable_factories.rs");
    std::fs::read_to_string(&vtable_factories_path).expect("read vtable_factories.rs")
}

/// Compute expected contract ID for a host contract.
fn expected_host_contract_id(name: &str, major: u32) -> u64 {
    host_contract_id(name, major)
}

// ─── Test 1: NATIVE and VM factory functions are generated ─────────────────────

/// Verifies that both `create_<contract>_vtable` (NATIVE) and
/// `create_<contract>_vtable_vm` (VM) factory functions are generated.
#[test]
fn test_vtable_factory_generates_native_and_vm_factories() {
    let tmp_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("vtable_factories_native_vm");
    let vtables: String = generate_host_vtable_factories(&tmp_dir);

    // NATIVE factory must exist
    assert!(
        vtables.contains("pub fn create_host_logger_vtable"),
        "NATIVE factory `create_host_logger_vtable` must be generated:\n{vtables}"
    );

    // VM factory must exist
    assert!(
        vtables.contains("pub fn create_host_logger_vtable_vm"),
        "VM factory `create_host_logger_vtable_vm` must be generated:\n{vtables}"
    );

    // NATIVE factory must return HostContractVTable
    assert!(
        vtables.contains("-> &'static HostContractVTable"),
        "Factories must return &'static HostContractVTable:\n{vtables}"
    );

    // NATIVE factory must take Box<dyn Trait>
    assert!(
        vtables.contains("implementation: Box<dyn HostLogger>"),
        "NATIVE factory must take Box<dyn HostLogger>:\n{vtables}"
    );

    // VM factory must take bridge_data and dispatch_fn
    assert!(
        vtables.contains("bridge_data: *mut c_void"),
        "VM factory must take bridge_data:\n{vtables}"
    );
    assert!(
        vtables.contains("dispatch_fn: unsafe extern \"C\" fn"),
        "VM factory must take dispatch_fn:\n{vtables}"
    );
}

// ─── Test 2: Vtable header has correct contract_id ─────────────────────────────

/// Verifies that the generated vtable header contains the correct contract_id
/// computed via FNV-1a hash of "host_contract:<name>@<major>".
#[test]
fn test_vtable_factory_header_has_correct_contract_id() {
    let tmp_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("vtable_factories_contract_id");
    let vtables: String = generate_host_vtable_factories(&tmp_dir);

    // Expected contract ID for host.logger@1
    let expected_id: u64 = expected_host_contract_id("host.logger", 1);
    let expected_id_hex: String = format!("contract_id: 0x{expected_id:016X}");

    // NATIVE factory must have correct contract_id
    assert!(
        vtables.contains(&expected_id_hex),
        "NATIVE vtable must have correct contract_id `{expected_id_hex}`:\n{vtables}"
    );

    // VM factory must also have correct contract_id (appears twice)
    let contract_id_count: usize = vtables.matches(&expected_id_hex).count();
    assert_eq!(
        contract_id_count, 2,
        "contract_id must appear in both NATIVE and VM factories (expected 2, got {contract_id_count}):\n{vtables}"
    );
}

// ─── Test 3: Vtable header has correct function_count ──────────────────────────

/// Verifies that the generated vtable header contains the correct function_count
/// matching the number of functions declared in the host contract.
#[test]
fn test_vtable_factory_header_has_correct_function_count() {
    let tmp_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("vtable_factories_fn_count");
    let vtables: String = generate_host_vtable_factories(&tmp_dir);

    // host.logger has 3 functions: log, log_level, get_level
    let expected_count: usize = 3;
    let expected_count_str: String = format!("function_count: {expected_count}");

    // NATIVE factory must have correct function_count
    assert!(
        vtables.contains(&expected_count_str),
        "NATIVE vtable must have function_count={expected_count}:\n{vtables}"
    );

    // VM factory must also have correct function_count (appears twice)
    let fn_count_count: usize = vtables.matches(&expected_count_str).count();
    assert_eq!(
        fn_count_count, 2,
        "function_count must appear in both NATIVE and VM factories (expected 2, got {fn_count_count}):\n{vtables}"
    );

    // Static FUNCTIONS array must have correct size
    let expected_array_size: String = format!(
        "[unsafe extern \"C\" fn(*const c_void, *const (), *mut ()) -> AbiError; {expected_count}]"
    );
    assert!(
        vtables.contains(&expected_array_size),
        "FUNCTIONS array must have size {expected_count}:\n{vtables}"
    );
}

// ─── Test 4: Thunks have panic safety ──────────────────────────────────────────

/// Verifies that each thunk function wraps its body in `std::panic::catch_unwind`
/// to ensure panic safety at the ABI boundary.
#[test]
fn test_vtable_factory_thunks_have_panic_safety() {
    let tmp_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("vtable_factories_panic_safety");
    let vtables: String = generate_host_vtable_factories(&tmp_dir);

    // Each thunk must use catch_unwind
    assert!(
        vtables.contains("std::panic::catch_unwind"),
        "Thunks must use std::panic::catch_unwind for panic safety:\n{vtables}"
    );

    // Each thunk must use AssertUnwindSafe
    assert!(
        vtables.contains("std::panic::AssertUnwindSafe"),
        "Thunks must use AssertUnwindSafe wrapper:\n{vtables}"
    );

    // Panic must return ABI_ERROR_PANIC
    assert!(
        vtables.contains("ABI_ERROR_PANIC"),
        "Panic handler must return ABI_ERROR_PANIC:\n{vtables}"
    );

    // Count catch_unwind occurrences - should be 3 (one per thunk)
    let catch_unwind_count: usize = vtables.matches("catch_unwind").count();
    assert_eq!(
        catch_unwind_count, 3,
        "Expected 3 catch_unwind calls (one per thunk), got {catch_unwind_count}:\n{vtables}"
    );

    // Verify each thunk name is present
    assert!(
        vtables.contains("host_logger_log_thunk"),
        "Thunk `host_logger_log_thunk` must exist:\n{vtables}"
    );
    assert!(
        vtables.contains("host_logger_log_level_thunk"),
        "Thunk `host_logger_log_level_thunk` must exist:\n{vtables}"
    );
    assert!(
        vtables.contains("host_logger_get_level_thunk"),
        "Thunk `host_logger_get_level_thunk` must exist:\n{vtables}"
    );
}

// ─── Test 5: NATIVE factory uses correct dispatch_type ─────────────────────────

/// Verifies that NATIVE factory sets dispatch_type to Native.
#[test]
fn test_vtable_factory_native_dispatch_type() {
    let tmp_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("vtable_factories_native_dispatch");
    let vtables: String = generate_host_vtable_factories(&tmp_dir);

    // NATIVE factory must set dispatch_type: DispatchType::Native
    assert!(
        vtables.contains("dispatch_type: DispatchType::Native"),
        "NATIVE factory must set dispatch_type to Native:\n{vtables}"
    );
}

// ─── Test 6: VM factory uses correct dispatch_type ─────────────────────────────

/// Verifies that VM factory sets dispatch_type to VirtualMachine.
#[test]
fn test_vtable_factory_vm_dispatch_type() {
    let tmp_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("vtable_factories_vm_dispatch");
    let vtables: String = generate_host_vtable_factories(&tmp_dir);

    // VM factory must set dispatch_type: DispatchType::VirtualMachine
    assert!(
        vtables.contains("dispatch_type: DispatchType::VirtualMachine"),
        "VM factory must set dispatch_type to VirtualMachine:\n{vtables}"
    );
}

// ─── Test 7: NATIVE factory leaks vtable correctly ─────────────────────────────

/// Verifies that NATIVE factory uses Box::leak to create a 'static vtable.
#[test]
fn test_vtable_factory_native_leaks_vtable() {
    let tmp_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("vtable_factories_native_leak");
    let vtables: String = generate_host_vtable_factories(&tmp_dir);

    // NATIVE factory must use Box::leak
    assert!(
        vtables.contains("Box::leak(Box::new(vtable))"),
        "NATIVE factory must use Box::leak to return 'static vtable:\n{vtables}"
    );

    // Implementation must be leaked via Box::into_raw
    assert!(
        vtables.contains("Box::into_raw(implementation)"),
        "NATIVE factory must leak implementation via Box::into_raw:\n{vtables}"
    );
}

// ─── Test 8: VM factory leaks vtable correctly ─────────────────────────────────

/// Verifies that VM factory uses Box::leak to create a 'static vtable.
#[test]
fn test_vtable_factory_vm_leaks_vtable() {
    let tmp_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("vtable_factories_vm_leak");
    let vtables: String = generate_host_vtable_factories(&tmp_dir);

    // VM factory must use Box::leak (appears twice - once for NATIVE, once for VM)
    let leak_count: usize = vtables.matches("Box::leak(Box::new(vtable))").count();
    assert_eq!(
        leak_count, 2,
        "Both factories must use Box::leak (expected 2, got {leak_count}):\n{vtables}"
    );
}

// ─── Test 9: Vtable header has correct version fields ──────────────────────────

/// Verifies that the generated vtable header contains correct contract_major
/// and contract_minor fields.
#[test]
fn test_vtable_factory_header_has_correct_version() {
    let tmp_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("vtable_factories_version");
    let vtables: String = generate_host_vtable_factories(&tmp_dir);

    // host.logger@1.0.0 -> major=1, minor=0
    assert!(
        vtables.contains("contract_major: 1"),
        "vtable must have contract_major=1:\n{vtables}"
    );
    assert!(
        vtables.contains("contract_minor: 0"),
        "vtable must have contract_minor=0:\n{vtables}"
    );
}

// ─── Test 10: Thunks have SAFETY comments ──────────────────────────────────────

/// Verifies that each thunk has proper SAFETY comments explaining the unsafe operations.
#[test]
fn test_vtable_factory_thunks_have_safety_comments() {
    let tmp_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("vtable_factories_safety_comments");
    let vtables: String = generate_host_vtable_factories(&tmp_dir);

    // Thunks must have SAFETY comments
    assert!(
        vtables.contains("// SAFETY:"),
        "Thunks must have SAFETY comments:\n{vtables}"
    );

    // SAFETY comment for impl_ptr dereference
    assert!(
        vtables.contains("impl_ptr is a valid"),
        "SAFETY comment must explain impl_ptr validity:\n{vtables}"
    );

    // SAFETY comment for args pointers
    assert!(
        vtables.contains("args is a valid"),
        "SAFETY comment must explain args validity:\n{vtables}"
    );
}
