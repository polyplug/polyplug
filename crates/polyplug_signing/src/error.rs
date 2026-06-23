//! SigError — structured error type for bundle signing operations.

use thiserror::Error;

/// Errors that can occur during bundle signing or verification.
#[derive(Debug, Error)]
pub enum SigError {
    #[error("bundle.sig is missing in bundle `{bundle}`")]
    MissingSignature { bundle: String },

    #[error("bundle.sig has invalid magic bytes in bundle `{bundle}`")]
    BadMagic { bundle: String },

    #[error("bundle.sig has unsupported format version {version} in bundle `{bundle}`")]
    BadVersion { bundle: String, version: u8 },

    #[error(
        "bundle.sig is malformed (expected {expected} bytes, found {found}) in bundle `{bundle}`"
    )]
    MalformedLength {
        bundle: String,
        expected: usize,
        found: usize,
    },

    #[error("signature verification failed for bundle `{bundle}`: {reason}")]
    SignatureMismatch { bundle: String, reason: String },

    #[error(
        "bundle `{bundle}` is signed by a key that is not in the host's trusted-key allowlist (key pinning rejected it)"
    )]
    UntrustedKey { bundle: String },

    #[error("key file has invalid magic bytes")]
    BadKeyMagic,

    #[error("key file has unsupported key type byte {key_type}")]
    BadKeyType { key_type: u8 },

    #[error("key file is malformed (expected {expected} bytes, found {found})")]
    MalformedKeyLength { expected: usize, found: usize },

    #[error("invalid Ed25519 key data: {reason}")]
    InvalidKeyData { reason: String },

    #[error("I/O error on `{path}`: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("bundle path `{path}` is not a directory")]
    NotADirectory { path: String },

    #[error("non-UTF-8 file path in bundle `{bundle}`: {path}")]
    NonUtf8Path { bundle: String, path: String },

    #[error("symlinks are not allowed in a signable bundle `{bundle}`: {path}")]
    SymlinkNotAllowed { bundle: String, path: String },

    #[error(
        "irregular file (not a regular file or directory) is not allowed in a signable bundle `{bundle}`: {path}"
    )]
    IrregularFile { bundle: String, path: String },

    #[error("file path `{path}` is outside bundle root in bundle `{bundle}`")]
    PathOutsideBundle { bundle: String, path: String },

    #[error("bundle `{bundle}` contains no signable files")]
    EmptyBundle { bundle: String },
}
