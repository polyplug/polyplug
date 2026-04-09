//! Tests for host-side interface factory generation.
//!
//! Verifies that the Rust generator produces valid interface factories for host contracts:
//!   1. Both NATIVE and VM factory functions are generated
//!   2. Interface header contains correct contract_id
//!   3. Interface header contains correct function_count
//!   4. Thunks have panic safety via catch_unwind

#![allow(clippy::expect_used)]

use polyplug_utils::host_contract_id;
use polyplug_codegen::{GenerateConfig, Lang, Side};
use polyplugc::generate;
use std::path::PathBuf;

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Create a test API TOML with a host contract containing multiple functions.
fn create_test_api_with_host_contract(tmp_dir: &PathBuf) -> PathBuf {
    std::fs::create_dir_all(tmp_dir).expect("create tmp_dir");
    let api_toml_path: PathBuf = tmp_dir.join("test_interface_api.toml");
    let content: &str = r#"# Test API with host contract for interface factory tests
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

/// Generate host-side Rust code and return the interface_factories.rs content.
fn generate_host_interface_factories(tmp_dir: &PathBuf) -> String {
    let api_toml: PathBuf = create_test_api_with_host_contract(tmp_dir);

    let config: GenerateConfig = GenerateConfig {
        api_toml: api_toml.clone(),
        lang: Lang::Rust,
        side: Side::Host,
        out_dir: tmp_dir.clone(),
    };

    let output = generate(config).expect("polyplugc::generate failed");

    // Write all generated files to disk
    for file in &output.files {
        let file_path: PathBuf = tmp_dir.join(&file.path);
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent).expect("failed to create parent dir");
        }
        std::fs::write(&file_path, &file.content).expect("failed to write generated file");
    }

    // Read the interface_factories.rs file
    let interface_factories_path: PathBuf = tmp_dir.join("host").join("interface_factories.rs");
    std::fs::read_to_string(&interface_factories_path).expect("read interface_factories.rs")
}

/// Compute expected contract ID for a host contract.
fn expected_host_contract_id(name: &str, major: u32) -> u64 {
    host_contract_id(name, major)
}

// ─── Test 1: NATIVE and VM factory functions are generated ─────────────────────

/// Verifies that both `create_<contract>_interface` (NATIVE) and
/// `create_<contract>_interface_vm` (VM) factory functions are generated.
#[test]
fn test_interface_factory_generates_native_and_vm_factories() {
    let tmp_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("interface_factories_native_vm");
    let interfaces: String = generate_host_interface_factories(&tmp_dir);

    // NATIVE factory must exist
    assert!(
        interfaces.contains("pub fn create_host_logger_interface"),
        "NATIVE factory `create_host_logger_interface` must be generated:\n{interfaces}"
    );

    // VM factory must exist
    assert!(
        interfaces.contains("pub fn create_host_logger_interface_vm"),
        "VM factory `create_host_logger_interface_vm` must be generated:\n{interfaces}"
    );

    // NATIVE factory must return HostContractInterface
    assert!(
        interfaces.contains("-> &'static HostContractInterface"),
        "Factories must return &'static HostContractInterface:\n{interfaces}"
    );

    // NATIVE factory must take Box<dyn Trait>
    assert!(
        interfaces.contains("implementation: Box<dyn HostLogger>"),
        "NATIVE factory must take Box<dyn HostLogger>:\n{interfaces}"
    );

    // VM factory must take bridge_data and dispatch_fn
    assert!(
        interfaces.contains("bridge_data: *mut c_void"),
        "VM factory must take bridge_data:\n{interfaces}"
    );
    assert!(
        interfaces.contains("dispatch_fn: unsafe extern \"C\" fn"),
        "VM factory must take dispatch_fn:\n{interfaces}"
    );
}

// ─── Test 2: Interface header has correct contract_id ─────────────────────────────

/// Verifies that the generated interface header contains the correct contract_id
/// computed via FNV-1a hash of "host_contract:<name>@<major>".
#[test]
fn test_interface_factory_header_has_correct_contract_id() {
    let tmp_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("interface_factories_contract_id");
    let interfaces: String = generate_host_interface_factories(&tmp_dir);

    // Expected contract ID for host.logger@1
    let expected_id: u64 = expected_host_contract_id("host.logger", 1);
    let expected_id_hex: String = format!("HostContractId::from(0x{expected_id:016X}_u64)");

    // NATIVE factory must have correct contract_id
    assert!(
        interfaces.contains(&expected_id_hex),
        "NATIVE interface must have correct contract_id `{expected_id_hex}`:\n{interfaces}"
    );

    // VM factory must also have correct contract_id (appears twice)
    let contract_id_count: usize = interfaces.matches(&expected_id_hex).count();
    assert_eq!(
        contract_id_count, 2,
        "contract_id must appear in both NATIVE and VM factories (expected 2, got {contract_id_count}):\n{interfaces}"
    );
}

// ─── Test 3: Interface header has correct function_count ──────────────────────────

/// Verifies that the generated interface header contains the correct function_count
/// matching the number of functions declared in the host contract.
#[test]
fn test_interface_factory_header_has_correct_function_count() {
    let tmp_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("interface_factories_fn_count");
    let interfaces: String = generate_host_interface_factories(&tmp_dir);

    // host.logger has 3 functions: log, log_level, get_level
    let expected_count: usize = 3;
    let expected_count_str: String = format!("function_count: {expected_count}");

    // NATIVE factory must have correct function_count
    assert!(
        interfaces.contains(&expected_count_str),
        "NATIVE interface must have function_count={expected_count}:\n{interfaces}"
    );

    // Note: VM factory uses VmDispatch which doesn't have function_count field
    // function_count only appears in NATIVE factory (NativeDispatch)

    // Static FUNCTIONS array must have correct size
    let expected_array_size: String = format!(
        "[unsafe extern \"C\" fn(*const c_void, *const (), *mut ()) -> AbiError; {expected_count}]"
    );
    assert!(
        interfaces.contains(&expected_array_size),
        "FUNCTIONS array must have size {expected_count}:\n{interfaces}"
    );
}

// ─── Test 4: Thunks have panic safety ──────────────────────────────────────────

/// Verifies that each thunk function wraps its body in `std::panic::catch_unwind`
/// to ensure panic safety at the ABI boundary.
#[test]
fn test_interface_factory_thunks_have_panic_safety() {
    let tmp_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("interface_factories_panic_safety");
    let interfaces: String = generate_host_interface_factories(&tmp_dir);

    // Each thunk must use catch_unwind
    assert!(
        interfaces.contains("std::panic::catch_unwind"),
        "Thunks must use std::panic::catch_unwind for panic safety:\n{interfaces}"
    );

    // Each thunk must use AssertUnwindSafe
    assert!(
        interfaces.contains("std::panic::AssertUnwindSafe"),
        "Thunks must use AssertUnwindSafe wrapper:\n{interfaces}"
    );

    // Panic must return AbiErrorCode::Panic
    assert!(
        interfaces.contains("AbiErrorCode::Panic"),
        "Panic handler must return AbiErrorCode::Panic:\n{interfaces}"
    );

    // Count catch_unwind occurrences - should be 3 (one per thunk)
    let catch_unwind_count: usize = interfaces.matches("catch_unwind").count();
    assert_eq!(
        catch_unwind_count, 3,
        "Expected 3 catch_unwind calls (one per thunk), got {catch_unwind_count}:\n{interfaces}"
    );

    // Verify each thunk name is present
    assert!(
        interfaces.contains("host_logger_log_thunk"),
        "Thunk `host_logger_log_thunk` must exist:\n{interfaces}"
    );
    assert!(
        interfaces.contains("host_logger_log_level_thunk"),
        "Thunk `host_logger_log_level_thunk` must exist:\n{interfaces}"
    );
    assert!(
        interfaces.contains("host_logger_get_level_thunk"),
        "Thunk `host_logger_get_level_thunk` must exist:\n{interfaces}"
    );
}

// ─── Test 5: NATIVE factory uses correct dispatch_type ─────────────────────────

/// Verifies that NATIVE factory sets dispatch_type to Native.
#[test]
fn test_interface_factory_native_dispatch_type() {
    let tmp_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("interface_factories_native_dispatch");
    let interfaces: String = generate_host_interface_factories(&tmp_dir);

    // NATIVE factory must set dispatch_type: DispatchType::Native
    assert!(
        interfaces.contains("dispatch_type: DispatchType::Native"),
        "NATIVE factory must set dispatch_type to Native:\n{interfaces}"
    );
}

// ─── Test 6: VM factory uses correct dispatch_type ─────────────────────────────

/// Verifies that VM factory sets dispatch_type to VirtualMachine.
#[test]
fn test_interface_factory_vm_dispatch_type() {
    let tmp_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("interface_factories_vm_dispatch");
    let interfaces: String = generate_host_interface_factories(&tmp_dir);

    // VM factory must set dispatch_type: DispatchType::VirtualMachine
    assert!(
        interfaces.contains("dispatch_type: DispatchType::VirtualMachine"),
        "VM factory must set dispatch_type to VirtualMachine:\n{interfaces}"
    );
}

// ─── Test 7: NATIVE factory leaks interface correctly ─────────────────────────────

/// Verifies that NATIVE factory uses Box::leak to create a 'static interface.
#[test]
fn test_interface_factory_native_leaks_interface() {
    let tmp_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("interface_factories_native_leak");
    let interfaces: String = generate_host_interface_factories(&tmp_dir);

    // NATIVE factory must use Box::leak
    assert!(
        interfaces.contains("Box::leak(Box::new(interface))"),
        "NATIVE factory must use Box::leak to return 'static interface:\n{interfaces}"
    );

    // Implementation must be leaked via Box::into_raw
    assert!(
        interfaces.contains("Box::into_raw(implementation)"),
        "NATIVE factory must leak implementation via Box::into_raw:\n{interfaces}"
    );
}

// ─── Test 8: VM factory leaks interface correctly ─────────────────────────────────

/// Verifies that VM factory uses Box::leak to create a 'static interface.
#[test]
fn test_interface_factory_vm_leaks_interface() {
    let tmp_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("interface_factories_vm_leak");
    let interfaces: String = generate_host_interface_factories(&tmp_dir);

    // VM factory must use Box::leak (appears twice - once for NATIVE, once for VM)
    let leak_count: usize = interfaces.matches("Box::leak(Box::new(interface))").count();
    assert_eq!(
        leak_count, 2,
        "Both factories must use Box::leak (expected 2, got {leak_count}):\n{interfaces}"
    );
}

// ─── Test 9: Interface header has correct version fields ──────────────────────────

/// Verifies that the generated interface header contains correct version
/// in the contract_version field.
#[test]
fn test_interface_factory_header_has_correct_version() {
    let tmp_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("interface_factories_version");
    let interfaces: String = generate_host_interface_factories(&tmp_dir);

    // host.logger@1.0.0 -> major=1, minor=0, patch=0
    assert!(
        interfaces.contains("contract_version: Version { major: 1, minor: 0, patch: 0 }"),
        "interface must have contract_version with major=1, minor=0, patch=0:\n{interfaces}"
    );
}

// ─── Test 10: Thunks have SAFETY comments ──────────────────────────────────────

/// Verifies that each thunk has proper SAFETY comments explaining the unsafe operations.
#[test]
fn test_interface_factory_thunks_have_safety_comments() {
    let tmp_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("interface_factories_safety_comments");
    let interfaces: String = generate_host_interface_factories(&tmp_dir);

    // Thunks must have SAFETY comments
    assert!(
        interfaces.contains("// SAFETY:"),
        "Thunks must have SAFETY comments:\n{interfaces}"
    );

    // SAFETY comment for impl_ptr dereference
    assert!(
        interfaces.contains("impl_ptr is a valid"),
        "SAFETY comment must explain impl_ptr validity:\n{interfaces}"
    );

    // SAFETY comment for args pointers
    assert!(
        interfaces.contains("args is a valid"),
        "SAFETY comment must explain args validity:\n{interfaces}"
    );
}
