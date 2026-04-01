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
