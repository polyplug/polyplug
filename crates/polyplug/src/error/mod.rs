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
}

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
