#![allow(clippy::expect_used)]

use polyplug_codegen::{GenerateConfig, Lang, Side};
use polyplugc::generate;
use std::path::Path;
use std::path::PathBuf;

fn create_test_api_with_host_contracts(tmp_dir: &Path) -> PathBuf {
    std::fs::create_dir_all(tmp_dir).expect("create tmp_dir");
    let api_toml_path: PathBuf = tmp_dir.join("test_host_contract_api.toml");
    let content: &str = r#"# Test API with host contracts
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
"#;
    std::fs::write(&api_toml_path, content).expect("failed to write test api.toml");
    api_toml_path
}

#[test]
fn test_rust_host_contract_generates_host_contracts_file() {
    let tmp_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("test_rust_host_contract");
    let api_toml: PathBuf = create_test_api_with_host_contracts(&tmp_dir);

    let config = GenerateConfig {
        api_toml: api_toml.clone(),
        lang: Lang::Rust,
        side: Side::Host,
        out_dir: tmp_dir.clone(),
    };

    let output = generate(config).expect("polyplugc::generate failed");

    for file in &output.files {
        let file_path = tmp_dir.join(&file.path);
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent).expect("failed to create parent dir");
        }
        std::fs::write(&file_path, &file.content).expect("failed to write generated file");
    }

    let host_contracts_path: PathBuf = tmp_dir.join("host").join("host_contracts.rs");
    assert!(
        host_contracts_path.exists(),
        "host/host_contracts.rs must exist"
    );

    let content: String =
        std::fs::read_to_string(&host_contracts_path).expect("read host_contracts.rs");

    assert!(
        content.contains("trait HostLogger"),
        "must contain trait HostLogger"
    );
    assert!(content.contains("fn log"), "must contain fn log");
    assert!(
        content.contains("HOSTLOGGER_CONTRACT_ID"),
        "must contain contract ID"
    );

    println!("test_rust_host_contract_generates_host_contracts_file: passed ✓");
}

#[test]
fn test_rust_host_contract_guest_generates_caller() {
    let tmp_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("test_rust_host_contract_guest");
    let api_toml: PathBuf = create_test_api_with_host_contracts(&tmp_dir);

    let config = GenerateConfig {
        api_toml: api_toml.clone(),
        lang: Lang::Rust,
        side: Side::Guest,
        out_dir: tmp_dir.clone(),
    };

    let output = generate(config).expect("polyplugc::generate failed");

    for file in &output.files {
        let file_path = tmp_dir.join(&file.path);
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent).expect("failed to create parent dir");
        }
        std::fs::write(&file_path, &file.content).expect("failed to write generated file");
    }

    let caller_path: PathBuf = tmp_dir.join("guest").join("host_contract_callers.rs");
    assert!(
        caller_path.exists(),
        "guest/host_contract_callers.rs must exist"
    );

    let content: String =
        std::fs::read_to_string(&caller_path).expect("read host_contract_callers.rs");

    assert!(
        content.contains("struct HostLoggerCaller"),
        "must contain HostLoggerCaller"
    );
    assert!(content.contains("from_host"), "must contain from_host");
    assert!(content.contains("is_valid"), "must contain is_valid");

    println!("test_rust_host_contract_guest_generates_caller: passed ✓");
}

#[test]
fn test_cpp_host_contract_generates_host_contracts_file() {
    let tmp_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("test_cpp_host_contract");
    let api_toml: PathBuf = create_test_api_with_host_contracts(&tmp_dir);

    let config = GenerateConfig {
        api_toml: api_toml.clone(),
        lang: Lang::Cpp,
        side: Side::Host,
        out_dir: tmp_dir.clone(),
    };

    let output = generate(config).expect("polyplugc::generate failed");

    for file in &output.files {
        let file_path = tmp_dir.join(&file.path);
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent).expect("failed to create parent dir");
        }
        std::fs::write(&file_path, &file.content).expect("failed to write generated file");
    }

    let host_contracts_path: PathBuf = tmp_dir.join("host").join("host_contracts.hpp");
    assert!(
        host_contracts_path.exists(),
        "host/host_contracts.hpp must exist"
    );

    let content: String =
        std::fs::read_to_string(&host_contracts_path).expect("read host_contracts.hpp");

    assert!(
        content.contains("class HostLogger"),
        "must contain class HostLogger"
    );
    assert!(content.contains("virtual"), "must contain virtual method");
    assert!(
        content.contains("HOSTLOGGER_CONTRACT_ID"),
        "must contain contract ID"
    );

    println!("test_cpp_host_contract_generates_host_contracts_file: passed ✓");
}

#[test]
fn test_cpp_host_contract_guest_generates_caller() {
    let tmp_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("test_cpp_host_contract_guest");
    let api_toml: PathBuf = create_test_api_with_host_contracts(&tmp_dir);

    let config = GenerateConfig {
        api_toml: api_toml.clone(),
        lang: Lang::Cpp,
        side: Side::Guest,
        out_dir: tmp_dir.clone(),
    };

    let output = generate(config).expect("polyplugc::generate failed");

    for file in &output.files {
        let file_path = tmp_dir.join(&file.path);
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent).expect("failed to create parent dir");
        }
        std::fs::write(&file_path, &file.content).expect("failed to write generated file");
    }

    let caller_path: PathBuf = tmp_dir.join("guest").join("host_contracts.hpp");
    assert!(caller_path.exists(), "guest/host_contracts.hpp must exist");

    let content: String =
        std::fs::read_to_string(&caller_path).expect("read guest/host_contracts.hpp");

    assert!(
        content.contains("HostLoggerContract"),
        "must contain HostLoggerContract"
    );
    assert!(content.contains("from_host"), "must contain from_host");
    assert!(content.contains("is_valid"), "must contain is_valid");

    println!("test_cpp_host_contract_guest_generates_caller: passed ✓");
}

#[test]
fn test_csharp_host_contract_generates_contracts_file() {
    let tmp_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("test_csharp_host_contract");
    let api_toml: PathBuf = create_test_api_with_host_contracts(&tmp_dir);

    let config = GenerateConfig {
        api_toml: api_toml.clone(),
        lang: Lang::CSharp,
        side: Side::Host,
        out_dir: tmp_dir.clone(),
    };

    let output = generate(config).expect("polyplugc::generate failed");

    for file in &output.files {
        let file_path = tmp_dir.join(&file.path);
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent).expect("failed to create parent dir");
        }
        std::fs::write(&file_path, &file.content).expect("failed to write generated file");
    }

    let contracts_path: PathBuf = tmp_dir.join("host").join("Contracts.cs");
    assert!(contracts_path.exists(), "host/Contracts.cs must exist");

    let content: String = std::fs::read_to_string(&contracts_path).expect("read Contracts.cs");

    assert!(
        content.contains("interface IHostLogger"),
        "must contain IHostLogger"
    );
    assert!(content.contains("void Log"), "must contain Log method");
    assert!(
        content.contains("HOSTLOGGER_CONTRACT_ID"),
        "must contain contract ID"
    );

    println!("test_csharp_host_contract_generates_contracts_file: passed ✓");
}

#[test]
fn test_csharp_host_contract_guest_generates_caller() {
    let tmp_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("test_csharp_host_contract_guest");
    let api_toml: PathBuf = create_test_api_with_host_contracts(&tmp_dir);

    let config = GenerateConfig {
        api_toml: api_toml.clone(),
        lang: Lang::CSharp,
        side: Side::Guest,
        out_dir: tmp_dir.clone(),
    };

    let output = generate(config).expect("polyplugc::generate failed");

    for file in &output.files {
        let file_path = tmp_dir.join(&file.path);
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent).expect("failed to create parent dir");
        }
        std::fs::write(&file_path, &file.content).expect("failed to write generated file");
    }

    let caller_path: PathBuf = tmp_dir.join("guest").join("HostContracts.cs");
    assert!(caller_path.exists(), "guest/HostContracts.cs must exist");

    let content: String =
        std::fs::read_to_string(&caller_path).expect("read guest/HostContracts.cs");

    assert!(content.contains("HostLogger"), "must contain HostLogger");
    assert!(content.contains("FromHost"), "must contain FromHost");
    assert!(content.contains("IsValid"), "must contain IsValid");

    println!("test_csharp_host_contract_guest_generates_caller: passed ✓");
}

/// Generate the full C# host side once and return (Callers.cs, InterfaceFactories.cs).
fn generate_csharp_host_side(tmp_dir: &Path) -> (String, String) {
    let api_toml: PathBuf = create_test_api_with_host_contracts(tmp_dir);
    let config = GenerateConfig {
        api_toml,
        lang: Lang::CSharp,
        side: Side::Host,
        out_dir: tmp_dir.to_path_buf(),
    };
    let output = generate(config).expect("polyplugc::generate failed");
    for file in &output.files {
        let file_path = tmp_dir.join(&file.path);
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent).expect("failed to create parent dir");
        }
        std::fs::write(&file_path, &file.content).expect("failed to write generated file");
    }
    let callers: String =
        std::fs::read_to_string(tmp_dir.join("host").join("Callers.cs")).expect("read Callers.cs");
    let factories: String =
        std::fs::read_to_string(tmp_dir.join("host").join("InterfaceFactories.cs"))
            .expect("read InterfaceFactories.cs");
    (callers, factories)
}

/// The host-caller path (host/Callers.cs) must use the real Runtime + ABI APIs:
/// a one-argument `Create(Runtime rt)` factory, `ResolveGuestContract` (not the
/// phantom `ResolveContract`), an `unsafe` class for its pointer fields, and it
/// must NOT reject a null instance handle (valid for stateless contracts).
#[test]
fn test_csharp_host_callers_use_real_runtime_api() {
    let tmp_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("test_csharp_host_callers_api");
    let (callers, _factories): (String, String) = generate_csharp_host_side(&tmp_dir);

    assert!(
        callers.contains("public static ExampleWorkerContractCaller? Create(Runtime rt) {"),
        "Callers.cs must emit a one-arg Create(Runtime rt) factory: {callers}"
    );
    assert!(
        callers.contains("rt.ResolveGuestContract(handle)"),
        "Callers.cs must call the real Runtime.ResolveGuestContract: {callers}"
    );
    assert!(
        !callers.contains("ResolveContract(handle)"),
        "Callers.cs must NOT call the phantom ResolveContract: {callers}"
    );
    assert!(
        callers.contains("public sealed unsafe class ExampleWorkerContractCaller"),
        "the caller class must be `unsafe` to hold pointer fields: {callers}"
    );
    assert!(
        callers.contains("(HostApi*)rt.HostHandle"),
        "Create must derive the HostApi pointer from Runtime.HostHandle: {callers}"
    );
    assert!(
        !callers.contains("if (inst.Data == nint.Zero) { return null; }"),
        "a null instance handle is valid for stateless contracts and must not abort Create: {callers}"
    );
    // create_instance / destroy_instance are IntPtr fields and must be cast to
    // function pointers before invocation, not called directly.
    assert!(
        callers.contains("(delegate* unmanaged[Cdecl]<HostApi*, void*, GuestContractInstance>)iface->CreateInstance"),
        "Create must cast the IntPtr CreateInstance field to a function pointer: {callers}"
    );

    println!("test_csharp_host_callers_use_real_runtime_api: passed ✓");
}

/// The interface-factory path (host/InterfaceFactories.cs) must build the real
/// ABI structs and never reference the phantom VTable type names.
#[test]
fn test_csharp_interface_factories_use_real_abi_types() {
    let tmp_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("test_csharp_factory_abi");
    let (_callers, factories): (String, String) = generate_csharp_host_side(&tmp_dir);

    assert!(
        factories
            .contains("public static unsafe HostContractInterface CreateHostLoggerInterface<T>"),
        "factory must return the canonical HostContractInterface: {factories}"
    );
    assert!(
        factories.contains("new NativeDispatch {"),
        "factory must build a NativeDispatch: {factories}"
    );
    assert!(
        factories.contains("new VmDispatch {"),
        "VM factory must build a VmDispatch: {factories}"
    );
    for phantom in [
        "HostContractVTable",
        "HostContractVTableHeader",
        "NativeHostContractDispatch",
        "VmHostContractDispatchFn",
        "HostContractDispatch",
    ] {
        assert!(
            !factories.contains(phantom),
            "InterfaceFactories.cs must not reference phantom type `{phantom}`: {factories}"
        );
    }

    println!("test_csharp_interface_factories_use_real_abi_types: passed ✓");
}

/// Generate the C# guest side once and return the guest host-contract caller
/// file (guest/HostContracts.cs).
fn generate_csharp_guest_host_contracts(tmp_dir: &Path) -> String {
    let api_toml: PathBuf = create_test_api_with_host_contracts(tmp_dir);
    let config = GenerateConfig {
        api_toml,
        lang: Lang::CSharp,
        side: Side::Guest,
        out_dir: tmp_dir.to_path_buf(),
    };
    let output = generate(config).expect("polyplugc::generate failed");
    for file in &output.files {
        let file_path = tmp_dir.join(&file.path);
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent).expect("failed to create parent dir");
        }
        std::fs::write(&file_path, &file.content).expect("failed to write generated file");
    }
    std::fs::read_to_string(tmp_dir.join("guest").join("HostContracts.cs"))
        .expect("read guest/HostContracts.cs")
}

/// The guest-caller path (guest/HostContracts.cs) must resolve host contracts
/// through the real HostApi ABI: `GetHostContract` for the instance and
/// `ResolveHostContractInterface` for the interface, then dispatch via the flat
/// `HostContractInterface` (DispatchType + Dispatch.Native / Dispatch.Vm). It
/// must never reference the phantom VTable / header types that the old emitter
/// produced.
#[test]
fn test_csharp_guest_host_contract_callers_use_real_abi_types() {
    let tmp_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("test_csharp_guest_caller_abi");
    let callers: String = generate_csharp_guest_host_contracts(&tmp_dir);

    // Real ABI mechanisms must be present.
    assert!(
        callers.contains("HostContractInterface*"),
        "guest caller must cast the interface to the canonical HostContractInterface*: {callers}"
    );
    assert!(
        callers.contains("GetHostContract"),
        "guest caller must obtain the instance via GetHostContract: {callers}"
    );
    assert!(
        callers.contains("ResolveHostContractInterface"),
        "guest caller must resolve the interface via ResolveHostContractInterface: {callers}"
    );
    assert!(
        callers.contains("contract->DispatchType"),
        "guest caller must branch on the real DispatchType field: {callers}"
    );
    assert!(
        callers.contains("contract->Dispatch.Native.Functions")
            && callers.contains("contract->Dispatch.Native.FunctionCount"),
        "guest caller must read the flat Dispatch.Native fields: {callers}"
    );
    assert!(
        callers.contains("contract->Dispatch.Vm.Call")
            && callers.contains("contract->Dispatch.Vm.LoaderData"),
        "guest caller must read the flat Dispatch.Vm fields: {callers}"
    );
    assert!(
        callers.contains("err.Code != AbiErrorCode.Ok"),
        "guest caller must check the AbiError code via the AbiErrorCode enum: {callers}"
    );

    // Phantom types from the old emitter must NOT appear.
    for phantom in [
        "HostContractVTable",
        "header->Header",
        "Dispatch.VM.Call",
        "Dispatch.VM.BridgeData",
        "BridgeData",
    ] {
        assert!(
            !callers.contains(phantom),
            "guest/HostContracts.cs must not reference phantom `{phantom}`: {callers}"
        );
    }

    println!("test_csharp_guest_host_contract_callers_use_real_abi_types: passed ✓");
}

#[test]
fn test_python_host_contract_generates_contracts_file() {
    let tmp_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("test_python_host_contract");
    let api_toml: PathBuf = create_test_api_with_host_contracts(&tmp_dir);

    let config = GenerateConfig {
        api_toml: api_toml.clone(),
        lang: Lang::Python,
        side: Side::Host,
        out_dir: tmp_dir.clone(),
    };

    let output = generate(config).expect("polyplugc::generate failed");

    for file in &output.files {
        let file_path = tmp_dir.join(&file.path);
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent).expect("failed to create parent dir");
        }
        std::fs::write(&file_path, &file.content).expect("failed to write generated file");
    }

    let contracts_path: PathBuf = tmp_dir.join("host").join("contracts.py");
    assert!(contracts_path.exists(), "host/contracts.py must exist");

    let content: String = std::fs::read_to_string(&contracts_path).expect("read contracts.py");

    assert!(
        content.contains("class HostLogger"),
        "must contain HostLogger"
    );
    assert!(content.contains("ABC"), "must contain ABC");
    assert!(content.contains("def log"), "must contain log method");
    assert!(
        content.contains("HOSTLOGGER_CONTRACT_ID"),
        "must contain contract ID"
    );

    println!("test_python_host_contract_generates_contracts_file: passed ✓");
}

#[test]
fn test_python_host_contract_guest_generates_caller() {
    let tmp_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("test_python_host_contract_guest");
    let api_toml: PathBuf = create_test_api_with_host_contracts(&tmp_dir);

    let config = GenerateConfig {
        api_toml: api_toml.clone(),
        lang: Lang::Python,
        side: Side::Guest,
        out_dir: tmp_dir.clone(),
    };

    let output = generate(config).expect("polyplugc::generate failed");

    for file in &output.files {
        let file_path = tmp_dir.join(&file.path);
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent).expect("failed to create parent dir");
        }
        std::fs::write(&file_path, &file.content).expect("failed to write generated file");
    }

    let caller_path: PathBuf = tmp_dir.join("guest").join("host_contracts.py");
    assert!(caller_path.exists(), "guest/host_contracts.py must exist");

    let content: String =
        std::fs::read_to_string(&caller_path).expect("read guest/host_contracts.py");

    assert!(
        content.contains("HostLoggerContract"),
        "must contain HostLoggerContract"
    );
    assert!(content.contains("from_host"), "must contain from_host");
    assert!(content.contains("is_valid"), "must contain is_valid");

    println!("test_python_host_contract_guest_generates_caller: passed ✓");
}

#[test]
fn test_lua_host_contract_generates_contracts_file() {
    let tmp_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("test_lua_host_contract");
    let api_toml: PathBuf = create_test_api_with_host_contracts(&tmp_dir);

    let config = GenerateConfig {
        api_toml: api_toml.clone(),
        lang: Lang::Lua,
        side: Side::Host,
        out_dir: tmp_dir.clone(),
    };

    let output = generate(config).expect("polyplugc::generate failed");

    for file in &output.files {
        let file_path = tmp_dir.join(&file.path);
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent).expect("failed to create parent dir");
        }
        std::fs::write(&file_path, &file.content).expect("failed to write generated file");
    }

    let contracts_path: PathBuf = tmp_dir.join("host").join("contracts.lua");
    assert!(contracts_path.exists(), "host/contracts.lua must exist");

    let content: String = std::fs::read_to_string(&contracts_path).expect("read contracts.lua");

    assert!(content.contains("HostLogger"), "must contain HostLogger");
    assert!(
        content.contains("function HostLogger:log"),
        "must contain log method"
    );
    assert!(
        content.contains("HOSTLOGGER_CONTRACT_ID"),
        "must contain contract ID"
    );

    println!("test_lua_host_contract_generates_contracts_file: passed ✓");
}

#[test]
fn test_lua_host_contract_guest_generates_caller() {
    let tmp_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("test_lua_host_contract_guest");
    let api_toml: PathBuf = create_test_api_with_host_contracts(&tmp_dir);

    let config = GenerateConfig {
        api_toml: api_toml.clone(),
        lang: Lang::Lua,
        side: Side::Guest,
        out_dir: tmp_dir.clone(),
    };

    let output = generate(config).expect("polyplugc::generate failed");

    for file in &output.files {
        let file_path = tmp_dir.join(&file.path);
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent).expect("failed to create parent dir");
        }
        std::fs::write(&file_path, &file.content).expect("failed to write generated file");
    }

    let caller_path: PathBuf = tmp_dir.join("guest").join("host_contracts.lua");
    assert!(caller_path.exists(), "guest/host_contracts.lua must exist");

    let content: String =
        std::fs::read_to_string(&caller_path).expect("read guest/host_contracts.lua");

    assert!(
        content.contains("HostLoggerContract"),
        "must contain HostLoggerContract"
    );
    assert!(content.contains("from_host"), "must contain from_host");
    assert!(content.contains("is_valid"), "must contain is_valid");

    println!("test_lua_host_contract_guest_generates_caller: passed ✓");
}

#[test]
fn test_js_quickjs_host_contract_generates_contracts_file() {
    let tmp_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("test_js_quickjs_host_contract");
    let api_toml: PathBuf = create_test_api_with_host_contracts(&tmp_dir);

    let config = GenerateConfig {
        api_toml: api_toml.clone(),
        lang: Lang::JsQuickJs,
        side: Side::Host,
        out_dir: tmp_dir.clone(),
    };

    let output = generate(config).expect("polyplugc::generate failed");

    for file in &output.files {
        let file_path = tmp_dir.join(&file.path);
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent).expect("failed to create parent dir");
        }
        std::fs::write(&file_path, &file.content).expect("failed to write generated file");
    }

    let contracts_path: PathBuf = tmp_dir.join("host").join("contracts.ts");
    assert!(contracts_path.exists(), "host/contracts.ts must exist");

    let content: String = std::fs::read_to_string(&contracts_path).expect("read contracts.ts");

    assert!(
        content.contains("interface HostLogger"),
        "must contain HostLogger interface"
    );
    assert!(content.contains("Log"), "must contain Log method");
    assert!(
        content.contains("HOSTLOGGER_CONTRACT_ID"),
        "must contain contract ID"
    );

    println!("test_js_quickjs_host_contract_generates_contracts_file: passed ✓");
}

#[test]
fn test_js_quickjs_host_contract_guest_generates_caller() {
    let tmp_dir: PathBuf =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("test_js_quickjs_host_contract_guest");
    let api_toml: PathBuf = create_test_api_with_host_contracts(&tmp_dir);

    let config = GenerateConfig {
        api_toml: api_toml.clone(),
        lang: Lang::JsQuickJs,
        side: Side::Guest,
        out_dir: tmp_dir.clone(),
    };

    let output = generate(config).expect("polyplugc::generate failed");

    for file in &output.files {
        let file_path = tmp_dir.join(&file.path);
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent).expect("failed to create parent dir");
        }
        std::fs::write(&file_path, &file.content).expect("failed to write generated file");
    }

    let caller_path: PathBuf = tmp_dir.join("guest").join("host_contracts.ts");
    assert!(caller_path.exists(), "guest/host_contracts.ts must exist");

    let content: String =
        std::fs::read_to_string(&caller_path).expect("read guest/host_contracts.ts");

    assert!(
        content.contains("HostLoggerContract"),
        "must contain HostLoggerContract"
    );
    assert!(content.contains("fromHost"), "must contain fromHost");
    assert!(content.contains("isValid"), "must contain isValid");

    println!("test_js_quickjs_host_contract_guest_generates_caller: passed ✓");
}
