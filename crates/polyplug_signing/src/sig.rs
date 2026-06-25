//! bundle.sig serialization, parsing, and signing.
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

use core::array::TryFromSliceError;

use ed25519_dalek::{Signature, SignatureError, Signer, SigningKey, VerifyingKey};
use std::fs;
use std::io::Error as IoError;
use std::path::{Path, PathBuf};

use crate::SigError;
use crate::digest::{SIG_FILE_NAME, canonical_digest};

/// Magic bytes that open every bundle.sig file.
const SIG_MAGIC: &[u8; 6] = b"PPSIG\0";

/// Format version for the current bundle.sig layout.
const SIG_VERSION: u8 = 0x01;

/// Total byte length of a serialized bundle.sig file.
pub(crate) const SIG_FILE_LEN: usize = 6 + 1 + 32 + 64; // 103 bytes

/// Parsed representation of a `bundle.sig` file.
#[derive(Debug, Clone)]
pub struct BundleSig {
    /// The verifying key embedded in the signature file.
    pub verifying_key: VerifyingKey,
    /// The Ed25519 signature over the 32-byte canonical bundle digest.
    pub signature: Signature,
}

impl BundleSig {
    /// Serialize to the 103-byte on-disk format.
    pub fn serialize(&self) -> [u8; SIG_FILE_LEN] {
        let mut buf: [u8; SIG_FILE_LEN] = [0u8; SIG_FILE_LEN];
        buf[..6].copy_from_slice(SIG_MAGIC);
        buf[6] = SIG_VERSION;
        buf[7..39].copy_from_slice(self.verifying_key.as_bytes());
        buf[39..103].copy_from_slice(&self.signature.to_bytes());
        buf
    }

    /// Parse from the 103-byte on-disk format, associating errors with `bundle`.
    pub fn parse(data: &[u8], bundle: &str) -> Result<BundleSig, SigError> {
        if data.len() != SIG_FILE_LEN {
            return Err(SigError::MalformedLength {
                bundle: bundle.to_owned(),
                expected: SIG_FILE_LEN,
                found: data.len(),
            });
        }
        if &data[..6] != SIG_MAGIC {
            return Err(SigError::BadMagic {
                bundle: bundle.to_owned(),
            });
        }
        if data[6] != SIG_VERSION {
            return Err(SigError::BadVersion {
                bundle: bundle.to_owned(),
                version: data[6],
            });
        }

        let vk_bytes: [u8; 32] =
            data[7..39]
                .try_into()
                .map_err(|_: TryFromSliceError| SigError::MalformedLength {
                    bundle: bundle.to_owned(),
                    expected: SIG_FILE_LEN,
                    found: data.len(),
                })?;
        let sig_bytes: [u8; 64] =
            data[39..103]
                .try_into()
                .map_err(|_: TryFromSliceError| SigError::MalformedLength {
                    bundle: bundle.to_owned(),
                    expected: SIG_FILE_LEN,
                    found: data.len(),
                })?;

        let verifying_key: VerifyingKey =
            VerifyingKey::from_bytes(&vk_bytes).map_err(|e: SignatureError| {
                SigError::SignatureMismatch {
                    bundle: bundle.to_owned(),
                    reason: format!("invalid verifying key: {e}"),
                }
            })?;

        let signature: Signature = Signature::from_bytes(&sig_bytes);

        Ok(BundleSig {
            verifying_key,
            signature,
        })
    }
}

/// Compute the canonical digest and write a `bundle.sig` file to `bundle_dir`.
///
/// Overwrites any existing `bundle.sig`.
pub fn sign_bundle(bundle_dir: &Path, signing_key: &SigningKey) -> Result<(), SigError> {
    let digest: [u8; 32] = canonical_digest(bundle_dir)?;

    let signature: Signature = signing_key.sign(&digest);
    let verifying_key: VerifyingKey = signing_key.verifying_key();

    let bundle_sig: BundleSig = BundleSig {
        verifying_key,
        signature,
    };

    let sig_path: PathBuf = bundle_dir.join(SIG_FILE_NAME);
    fs::write(&sig_path, bundle_sig.serialize()).map_err(|e: IoError| SigError::Io {
        path: sig_path.display().to_string(),
        source: e,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use crate::key::generate_keypair;

    #[test]
    fn bundle_sig_roundtrip() {
        let (signing_key, verifying_key): (SigningKey, VerifyingKey) = generate_keypair();
        let digest: [u8; 32] = [0xABu8; 32];

        let signature: Signature = signing_key.sign(&digest);

        let bundle_sig: BundleSig = BundleSig {
            verifying_key,
            signature,
        };
        let serialized: [u8; SIG_FILE_LEN] = bundle_sig.serialize();
        let parsed: BundleSig = BundleSig::parse(&serialized, "test_bundle").expect("parse");

        assert_eq!(
            bundle_sig.verifying_key.as_bytes(),
            parsed.verifying_key.as_bytes()
        );
        assert_eq!(bundle_sig.signature.to_bytes(), parsed.signature.to_bytes());
    }

    #[test]
    fn corrupt_magic_is_rejected() {
        let (signing_key, verifying_key): (SigningKey, VerifyingKey) = generate_keypair();
        let digest: [u8; 32] = [0xCDu8; 32];
        let signature: Signature = signing_key.sign(&digest);
        let bundle_sig: BundleSig = BundleSig {
            verifying_key,
            signature,
        };
        let mut buf: [u8; SIG_FILE_LEN] = bundle_sig.serialize();
        buf[0] = 0xFF;
        assert!(matches!(
            BundleSig::parse(&buf, "test_bundle"),
            Err(SigError::BadMagic { .. })
        ));
    }
}
