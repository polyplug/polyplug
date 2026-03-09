//! Error — error type hierarchy for polyplug.

use thiserror::Error;

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
}

/// Convenience alias — the public API surface uses this name.
pub type PolyplugError = RuntimeError;

/// Errors from the bundle loading phase.
#[derive(Debug, Error)]
pub enum LoaderError {
    #[error("failed to load plugin bundle at `{path}`: {source}")]
    LoadFailed {
        path: String,
        #[source]
        source: libloading::Error,
    },

    #[error("ABI version mismatch in `{bundle}`: expected={expected}, found={found}")]
    AbiVersionMismatch {
        bundle: String,
        expected: u32,
        found: u32,
    },

    #[error("missing symbol `{symbol}` in bundle `{bundle}`")]
    MissingSymbol { bundle: String, symbol: String },

    #[error("init failed for bundle `{bundle}`: {error}")]
    InitFailed { bundle: String, error: String },

    #[error("manifest parse error for `{path}`: {reason}")]
    ManifestParse { path: String, reason: String },

    #[error(
        "duplicate loader for runtime \"{runtime_name}\": \
         a loader for this runtime is already registered"
    )]
    DuplicateLoader { runtime_name: String },

    #[error(
        "bundle \"{bundle}\" requires runtime \"{runtime_name}\" \
         but no loader is registered for runtime \"{runtime_name}\".\n\
         Add polyplug-{runtime_name} as a dependency and register \
         the loader at init."
    )]
    NoLoaderForRuntime {
        bundle: String,
        runtime_name: String,
    },

    #[error(
        "runtime \"{runtime_name}\" is not yet implemented \
         (this adapter crate is a stub)"
    )]
    RuntimeNotImplemented { runtime_name: String },

    #[error("hostfxr not found: searched DOTNET_ROOT, PATH, and well-known paths")]
    HostfxrNotFound,

    #[error("CLR initialization failed for runtime config `{path}`: {reason}")]
    ClrInitFailed { path: String, reason: String },

    #[error("assembly not found at path `{path}`")]
    AssemblyNotFound { path: String },

    #[error(
        "init symbol missing in assembly `{bundle}`: expected `[UnmanagedCallersOnly] polyplug_init`"
    )]
    InitSymbolMissing { bundle: String },

    #[error(".NET runtime version mismatch: required={required}, found={found}")]
    RuntimeVersionMismatch { required: String, found: String },

    #[error("Python interpreter initialization failed: {reason}")]
    PythonInitFailed { reason: String },

    #[error("failed to import Python module at `{path}`: {reason}")]
    PythonModuleImportFailed { path: String, reason: String },

    #[error("Python init function raised exception in bundle `{bundle}`: {message}")]
    PythonInitRaisedException { bundle: String, message: String },

    #[error("lua vm init failed: {reason}")]
    LuaVmInitFailed { reason: String },

    #[error("lua script load failed: path={path}, reason={reason}")]
    LuaScriptLoadFailed { path: String, reason: String },

    #[error("lua plugin missing polyplug_init function: bundle={bundle}")]
    LuaInitFunctionMissing { bundle: String },

    #[error("lua polyplug_init raised error: bundle={bundle}, message={message}")]
    LuaInitRaisedError { bundle: String, message: String },
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

    #[error("stale plugin handle: index={index}, generation={expected} (found={found})")]
    StaleHandle {
        index: u32,
        expected: u32,
        found: u32,
    },

    #[error("no plugin found for contract_id=0x{contract_id:016X} with min_version={min_version}")]
    PluginNotFound { contract_id: u64, min_version: u32 },
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
