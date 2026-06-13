//! Error — error type hierarchy for polyplug.

use thiserror::Error;

use polyplug_abi::types::Version;

/// Top-level runtime error — this is what the public API returns.
#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error(transparent)]
    Loader(#[from] LoaderError),

    #[error(transparent)]
    Registry(#[from] RegistryError),

    #[error(transparent)]
    Graph(#[from] GraphError),

    #[error(transparent)]
    Allocator(#[from] AllocatorError),

    #[error(transparent)]
    HostContract(#[from] HostContractError),

    #[error(
        "undeclared dependency: bundle_id={bundle_id:#x} attempted to resolve contract_id={contract_id:#x} without declaring it"
    )]
    UndeclaredDependency { bundle_id: u64, contract_id: u64 },

    #[error("dependency not found: contract={contract_name} min_version={min_version}")]
    DependencyNotFound {
        contract_name: String,
        min_version: u32,
    },

    #[error("bundle not found for contract: bundle={bundle_name} contract={contract_name}")]
    BundleNotFound {
        bundle_name: String,
        contract_name: String,
    },

    #[error("reload failed for bundle `{bundle}`: {reason}")]
    ReloadFailed { bundle: String, reason: String },

    #[error("invalid UTF-8 in plugin-provided data: context={context}")]
    InvalidUtf8 { context: String },

    #[error("hot-reload is disabled in runtime config")]
    HotReloadDisabled,

    #[error(
        "cannot unload bundle `{provider}`: still-loaded bundles declared a dependency on it: {dependents:?}"
    )]
    DependencyInUse {
        provider: String,
        dependents: Vec<String>,
    },
}

/// Errors from the bundle loading phase.
#[derive(Debug, Error)]
pub enum LoaderError {
    #[error("init failed for bundle `{bundle}`: {error}")]
    InitFailed { bundle: String, error: String },

    #[error("manifest parse error for `{path}`: {reason}")]
    ManifestParse { path: String, reason: String },

    #[error(
        "duplicate loader \"{loader_name}\": \
         a loader with this name is already registered"
    )]
    DuplicateLoader { loader_name: String },

    #[error(
        "bundle \"{bundle}\" declares loader \"{loader_name}\" \
         but no loader is registered for \"{loader_name}\".\n\
         Add polyplug_{loader_name} as a dependency and register \
         the loader at init."
    )]
    NoLoaderForName { bundle: String, loader_name: String },

    #[error(
        "init symbol missing in assembly `{bundle}`: expected `[UnmanagedCallersOnly] polyplug_init`"
    )]
    InitSymbolMissing { bundle: String },

    #[error("failed to read bundle at `{path}`: {source}")]
    BundleReadFailed {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("version mismatch for contract `{contract}`: required={required}, found={found}")]
    VersionMismatch {
        contract: String,
        required: Version,
        found: Version,
    },

    #[error(
        "function count mismatch for contract `{contract}`: expected={expected}, found={found}"
    )]
    FunctionCountMismatch {
        contract: String,
        expected: u32,
        found: u32,
    },

    #[error("bundle path is not a directory: `{path}`")]
    BundleNotADirectory { path: std::path::PathBuf },

    #[error("bundle \"{bundle}\" manifest.toml has an empty or missing `file` field")]
    ManifestMissingFile { bundle: String },

    #[error(
        "loader `{loader}` does not support bundle source kind `{source_kind}` (bundle `{bundle}`)"
    )]
    UnsupportedBundleSource {
        loader: &'static str,
        source_kind: &'static str,
        bundle: String,
    },

    #[error(
        "loader `{loader}` received non-UTF-8 source bytes for bundle `{bundle}`: \
         {source_kind} sources must be valid UTF-8 text"
    )]
    InvalidSourceEncoding {
        loader: &'static str,
        source_kind: &'static str,
        bundle: String,
    },

    #[error(
        "bundle \"{bundle}\" tampered with bundle_id: expected={expected:#x}, found={found:#x}"
    )]
    BundleTampered {
        bundle: String,
        expected: u64,
        found: u64,
    },

    #[error("loader `{loader_name}` does not support hot-reload")]
    HotReloadUnsupported { loader_name: String },
}

/// Errors from the plugin registry.
#[derive(Debug, Error)]
pub enum RegistryError {
    #[error("contract ID collision: hash=0x{id:016X} claimed by both `{name_a}` and `{name_b}`")]
    ContractIdCollision {
        id: u64,
        name_a: String,
        name_b: String,
    },

    #[error("duplicate provider for contract `{contract}`: `{existing}` already registered")]
    DuplicateProvider { contract: String, existing: String },

    #[error("invalid plugin handle: index={index} is out of bounds")]
    InvalidHandle { index: u32 },

    #[error("stale plugin handle: index={index} refers to a retired or recycled slot")]
    StaleHandle { index: u32 },

    #[error("no plugin found for contract_id=0x{contract_id:016X} with min_version={min_version}")]
    PluginNotFound { contract_id: u64, min_version: u32 },

    #[error("invalid dispatch_type {value} in guest contract interface (untrusted plugin)")]
    InvalidDispatchType { value: u32 },

    #[error("invalid UTF-8 in plugin-provided data: context={context}")]
    InvalidUtf8 { context: String },
}

/// Errors from the capability graph.
#[derive(Debug, Error)]
pub enum GraphError {
    #[error("dependency cycle detected involving: {participants:?}")]
    DependencyCycle { participants: Vec<String> },

    #[error(
        "unsatisfied capability: `{requester}` requires `{capability}` but no loaded plugin provides it"
    )]
    UnsatisfiedCapability {
        requester: String,
        capability: String,
    },
}

/// Errors from the host allocator.
#[derive(Debug, Error)]
pub enum AllocatorError {
    #[error("allocation failed: requested {size} bytes (system allocator returned null)")]
    AllocationFailed { size: usize },

    #[error("invalid layout: size={size}, align={align}")]
    InvalidLayout { size: usize, align: usize },
}

/// Errors from host contract registration.
#[derive(Debug, Error)]
pub enum HostContractError {
    #[error("duplicate host contract registration: contract_id=0x{contract_id:016X}")]
    DuplicateContract { contract_id: u64 },

    #[error("host contract not found: contract_id=0x{contract_id:016X}, min_version={min_version}")]
    ContractNotFound { contract_id: u64, min_version: u32 },

    #[error(
        "host contract version mismatch: contract_id=0x{contract_id:016X}, required={required}, found={found}"
    )]
    VersionMismatch {
        contract_id: u64,
        required: u32,
        found: u32,
    },
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use super::{
        AllocatorError, GraphError, HostContractError, LoaderError, RegistryError, RuntimeError,
    };
    use polyplug_abi::types::Version;

    // --- RuntimeError / RuntimeError ---

    #[test]
    fn polyplug_error_undeclared_dependency_display() {
        let err: RuntimeError = RuntimeError::UndeclaredDependency {
            bundle_id: 0xDEAD_BEEF_0000_0001,
            contract_id: 0xCAFE_BABE_0000_0002,
        };
        let s: String = err.to_string();
        assert!(s.contains("undeclared dependency"), "got: {s}");
        assert!(
            s.contains("0xdeadbeef00000001")
                || s.contains("deadbeef")
                || s.contains("DEADBEEF")
                || s.to_lowercase().contains("deadbeef"),
            "got: {s}"
        );
        assert!(
            s.to_lowercase().contains("cafebabe")
                || s.contains("cafebabe")
                || s.contains("CAFEBABE"),
            "got: {s}"
        );
    }

    #[test]
    fn polyplug_error_dependency_not_found_display() {
        let err: RuntimeError = RuntimeError::DependencyNotFound {
            contract_name: "my_contract".to_owned(),
            min_version: 3,
        };
        let s: String = err.to_string();
        assert!(s.contains("dependency not found"), "got: {s}");
        assert!(s.contains("my_contract"), "got: {s}");
        assert!(s.contains('3'), "got: {s}");
    }

    #[test]
    fn polyplug_error_bundle_not_found_display() {
        let err: RuntimeError = RuntimeError::BundleNotFound {
            bundle_name: "audio_plugin".to_owned(),
            contract_name: "audio_contract".to_owned(),
        };
        let s: String = err.to_string();
        assert!(s.contains("bundle not found"), "got: {s}");
        assert!(s.contains("audio_plugin"), "got: {s}");
        assert!(s.contains("audio_contract"), "got: {s}");
    }

    #[test]
    fn polyplug_error_reload_failed_display() {
        let err: RuntimeError = RuntimeError::ReloadFailed {
            bundle: "my_bundle".to_owned(),
            reason: "library locked".to_owned(),
        };
        let s: String = err.to_string();
        assert!(s.contains("reload failed"), "got: {s}");
        assert!(s.contains("my_bundle"), "got: {s}");
        assert!(s.contains("library locked"), "got: {s}");
    }

    #[test]
    fn polyplug_error_invalid_utf8_display() {
        let err: RuntimeError = RuntimeError::InvalidUtf8 {
            context: "plugin_name_field".to_owned(),
        };
        let s: String = err.to_string();
        assert!(
            s.contains("invalid UTF-8") || s.contains("invalid utf-8") || s.contains("UTF-8"),
            "got: {s}"
        );
        assert!(s.contains("plugin_name_field"), "got: {s}");
    }

    // --- LoaderError ---

    #[test]
    fn loader_error_init_failed_display() {
        let err: LoaderError = LoaderError::InitFailed {
            bundle: "init_bundle".to_owned(),
            error: "null pointer dereference".to_owned(),
        };
        let s: String = err.to_string();
        assert!(s.contains("init failed"), "got: {s}");
        assert!(s.contains("init_bundle"), "got: {s}");
        assert!(s.contains("null pointer dereference"), "got: {s}");
    }

    #[test]
    fn loader_error_manifest_parse_display() {
        let err: LoaderError = LoaderError::ManifestParse {
            path: "/opt/plugins/manifest.toml".to_owned(),
            reason: "unexpected key `foobar`".to_owned(),
        };
        let s: String = err.to_string();
        assert!(s.contains("manifest parse error"), "got: {s}");
        assert!(s.contains("/opt/plugins/manifest.toml"), "got: {s}");
        assert!(s.contains("foobar"), "got: {s}");
    }

    #[test]
    fn loader_error_duplicate_loader_display() {
        let err: LoaderError = LoaderError::DuplicateLoader {
            loader_name: "python".to_owned(),
        };
        let s: String = err.to_string();
        assert!(s.contains("duplicate loader"), "got: {s}");
        assert!(s.contains("python"), "got: {s}");
    }

    #[test]
    fn loader_error_no_loader_for_name_display() {
        let err: LoaderError = LoaderError::NoLoaderForName {
            bundle: "my.bundle".to_owned(),
            loader_name: "lua".to_owned(),
        };
        let s: String = err.to_string();
        assert!(
            s.contains("no loader") || s.contains("loader is registered"),
            "got: {s}"
        );
        assert!(s.contains("my.bundle"), "got: {s}");
        assert!(s.contains("lua"), "got: {s}");
    }

    #[test]
    fn loader_error_version_mismatch_display() {
        let err: LoaderError = LoaderError::VersionMismatch {
            contract: "audio_v2".to_owned(),
            required: Version {
                major: 2,
                minor: 0,
                patch: 0,
            },
            found: Version {
                major: 1,
                minor: 9,
                patch: 0,
            },
        };
        let s: String = err.to_string();
        assert!(s.contains("version mismatch"), "got: {s}");
        assert!(s.contains("audio_v2"), "got: {s}");
        assert!(s.contains("2.0"), "got: {s}");
        assert!(s.contains("1.9"), "got: {s}");
    }

    #[test]
    fn loader_error_function_count_mismatch_display() {
        let err: LoaderError = LoaderError::FunctionCountMismatch {
            contract: "render_contract".to_owned(),
            expected: 5,
            found: 3,
        };
        let s: String = err.to_string();
        assert!(s.contains("function count mismatch"), "got: {s}");
        assert!(s.contains("render_contract"), "got: {s}");
        assert!(s.contains('5'), "got: {s}");
        assert!(s.contains('3'), "got: {s}");
    }

    #[test]
    fn loader_error_bundle_not_a_directory_display() {
        let err: LoaderError = LoaderError::BundleNotADirectory {
            path: std::path::PathBuf::from("/usr/lib/plugins/myplugin.so"),
        };
        let s: String = err.to_string();
        assert!(s.contains("not a directory"), "got: {s}");
        assert!(s.contains("myplugin.so"), "got: {s}");
    }

    #[test]
    fn loader_error_manifest_missing_file_display() {
        let err: LoaderError = LoaderError::ManifestMissingFile {
            bundle: "orphan_bundle".to_owned(),
        };
        let s: String = err.to_string();
        assert!(s.contains("missing") || s.contains("empty"), "got: {s}");
        assert!(s.contains("orphan_bundle"), "got: {s}");
    }

    #[test]
    fn loader_error_bundle_tampered_display() {
        let err: LoaderError = LoaderError::BundleTampered {
            bundle: "malicious_bundle".to_owned(),
            expected: 0xDEAD_BEEF_u64,
            found: 0xCAFE_BABE_u64,
        };
        let s: String = err.to_string();
        assert!(s.contains("tampered"), "got: {s}");
        assert!(s.contains("malicious_bundle"), "got: {s}");
        assert!(
            s.to_lowercase().contains("deadbeef") || s.contains("deadbeef"),
            "got: {s}"
        );
        assert!(
            s.to_lowercase().contains("cafebabe") || s.contains("cafebabe"),
            "got: {s}"
        );
    }

    // --- RegistryError ---

    #[test]
    fn registry_error_contract_id_collision_display() {
        let err: RegistryError = RegistryError::ContractIdCollision {
            id: 0xABCD_EF01_2345_6789,
            name_a: "plugin_alpha".to_owned(),
            name_b: "plugin_beta".to_owned(),
        };
        let s: String = err.to_string();
        assert!(s.contains("collision"), "got: {s}");
        assert!(s.to_lowercase().contains("abcdef01"), "got: {s}");
        assert!(s.contains("plugin_alpha"), "got: {s}");
        assert!(s.contains("plugin_beta"), "got: {s}");
    }

    #[test]
    fn registry_error_duplicate_provider_display() {
        let err: RegistryError = RegistryError::DuplicateProvider {
            contract: "logging_contract".to_owned(),
            existing: "log4plug".to_owned(),
        };
        let s: String = err.to_string();
        assert!(s.contains("duplicate provider"), "got: {s}");
        assert!(s.contains("logging_contract"), "got: {s}");
        assert!(s.contains("log4plug"), "got: {s}");
    }

    #[test]
    fn registry_error_invalid_handle_display() {
        let err: RegistryError = RegistryError::InvalidHandle { index: 7 };
        let s: String = err.to_string();
        assert!(s.contains("invalid"), "got: {s}");
        assert!(s.contains('7'), "got: {s}");
    }

    #[test]
    fn registry_error_plugin_not_found_display() {
        let err: RegistryError = RegistryError::PluginNotFound {
            contract_id: 0x1234_5678_9ABC_DEF0,
            min_version: 5,
        };
        let s: String = err.to_string();
        assert!(s.contains("no plugin found"), "got: {s}");
        assert!(
            s.to_lowercase().contains("123456789abc") || s.to_lowercase().contains("12345678"),
            "got: {s}"
        );
        assert!(s.contains('5'), "got: {s}");
    }

    // --- GraphError ---

    #[test]
    fn graph_error_dependency_cycle_display() {
        let err: GraphError = GraphError::DependencyCycle {
            participants: vec!["alpha".to_owned(), "beta".to_owned(), "gamma".to_owned()],
        };
        let s: String = err.to_string();
        assert!(s.contains("cycle"), "got: {s}");
        assert!(s.contains("alpha"), "got: {s}");
        assert!(s.contains("beta"), "got: {s}");
        assert!(s.contains("gamma"), "got: {s}");
    }

    #[test]
    fn graph_error_unsatisfied_capability_display() {
        let err: GraphError = GraphError::UnsatisfiedCapability {
            requester: "ui_plugin".to_owned(),
            capability: "gpu_render".to_owned(),
        };
        let s: String = err.to_string();
        assert!(s.contains("unsatisfied capability"), "got: {s}");
        assert!(s.contains("ui_plugin"), "got: {s}");
        assert!(s.contains("gpu_render"), "got: {s}");
    }

    // --- AllocatorError ---

    #[test]
    fn allocator_error_allocation_failed_display() {
        let err: AllocatorError = AllocatorError::AllocationFailed { size: 4096 };
        let s: String = err.to_string();
        assert!(s.contains("allocation failed"), "got: {s}");
        assert!(s.contains("4096"), "got: {s}");
    }

    #[test]
    fn allocator_error_invalid_layout_display() {
        let err: AllocatorError = AllocatorError::InvalidLayout { size: 0, align: 3 };
        let s: String = err.to_string();
        assert!(s.contains("invalid layout"), "got: {s}");
        assert!(s.contains('0'), "got: {s}");
        assert!(s.contains('3'), "got: {s}");
    }

    // --- HostContractError ---

    #[test]
    fn host_contract_error_duplicate_contract_display() {
        let err: HostContractError = HostContractError::DuplicateContract {
            contract_id: 0xABCD_EF01_2345_6789,
        };
        let s: String = err.to_string();
        assert!(s.contains("duplicate"), "got: {s}");
        assert!(s.to_lowercase().contains("abcdef01"), "got: {s}");
    }

    #[test]
    fn host_contract_error_contract_not_found_display() {
        let err: HostContractError = HostContractError::ContractNotFound {
            contract_id: 0x1234_5678_9ABC_DEF0,
            min_version: 2,
        };
        let s: String = err.to_string();
        assert!(s.contains("not found"), "got: {s}");
        assert!(s.to_lowercase().contains("12345678"), "got: {s}");
        assert!(s.contains('2'), "got: {s}");
    }

    #[test]
    fn host_contract_error_version_mismatch_display() {
        let err: HostContractError = HostContractError::VersionMismatch {
            contract_id: 0xCAFE_BABE_0000_0001,
            required: 3,
            found: 1,
        };
        let s: String = err.to_string();
        assert!(s.contains("version mismatch"), "got: {s}");
        assert!(s.to_lowercase().contains("cafebabe"), "got: {s}");
        assert!(s.contains('3'), "got: {s}");
        assert!(s.contains('1'), "got: {s}");
    }
}
