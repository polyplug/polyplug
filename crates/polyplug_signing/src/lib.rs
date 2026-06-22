//! Bundle signing and verification for the polyplug plugin runtime.
//!
//! # Canonical digest algorithm
//!
//! The signed digest is computed deterministically so any toolchain can reproduce it:
//!
//! 1. Walk the bundle directory recursively. Collect every file path EXCEPT `bundle.sig`.
//! 2. Compute each file's path RELATIVE to the bundle root using `/` as separator on all
//!    platforms.
//! 3. Sort the file list lexicographically by the relative-path bytes.
//! 4. Build a canonical byte buffer: for each file in sorted order, append:
//!    - the relative-path UTF-8 bytes
//!    - a single `0x00` byte (NUL separator)
//!    - the 32-byte SHA-256 of that file's contents
//! 5. The signed message digest = SHA-256 of the entire canonical buffer (32 bytes).
//! 6. The Ed25519 signature is over this 32-byte digest.
//!
//! # bundle.sig on-disk format
//!
//! ```text
//! Offset  Size  Description
//! 0       6     Magic: b"PPSIG\0"
//! 6       1     Format version: 0x01
//! 7       32    Ed25519 verifying (public) key bytes
//! 39      64    Ed25519 signature bytes
//! Total:  103 bytes
//! ```
//!
//! # Key file format
//!
//! Signing (private) key file:
//! ```text
//! Offset  Size  Description
//! 0       6     Magic: b"PPKEY\0"
//! 6       1     Key type: 0x01 = Ed25519 signing key
//! 7       32    Ed25519 signing key bytes (scalar seed)
//! Total:  39 bytes
//! ```
//!
//! Verifying (public) key file:
//! ```text
//! Offset  Size  Description
//! 0       6     Magic: b"PPKEY\0"
//! 6       1     Key type: 0x02 = Ed25519 verifying key
//! 7       32    Ed25519 verifying key bytes
//! Total:  39 bytes
//! ```

mod digest;
mod error;
mod key;
mod sig;
mod verifier;

pub use digest::canonical_digest;
pub use error::SigError;
pub use key::{
    generate_keypair, load_signing_key, load_verifying_key, save_signing_key, save_verifying_key,
};
pub use sig::{BundleSig, sign_bundle};
pub use verifier::{BundleVerifier, Ed25519Verifier, VerifiedBundle, verify_bundle};
