//! BundleVerifier trait and Ed25519Verifier implementation.

use std::path::Path;

use ed25519_dalek::VerifyingKey;

use crate::SigError;
use crate::digest::{SIG_FILE_NAME, canonical_digest};
use crate::sig::BundleSig;

/// The verifying key and name of a successfully verified bundle.
#[derive(Debug, Clone)]
pub struct VerifiedBundle {
    /// The verifying key embedded in the bundle's `bundle.sig`.
    ///
    /// A future key-pinning layer can inspect this to enforce an allowlist.
    pub verifying_key: VerifyingKey,
    /// The canonical bundle directory path that was verified.
    pub bundle_dir: std::path::PathBuf,
}

/// Trait that abstracts bundle verification so alternative policies (e.g.,
/// key-pinning, allowlists) can be added later without changing call sites.
pub trait BundleVerifier: Send + Sync {
    /// Verify that `bundle_dir` contains a valid, untampered `bundle.sig`.
    ///
    /// On success, returns a `VerifiedBundle` containing the embedded verifying
    /// key so the caller can apply additional trust decisions.
    fn verify(&self, bundle_dir: &Path) -> Result<VerifiedBundle, SigError>;
}

/// Default verifier: reads `bundle.sig`, verifies the embedded Ed25519 signature
/// against the canonical bundle digest. Does NOT require a pre-known key — the
/// public key travels with the bundle (TOFU / self-signed model).
pub struct Ed25519Verifier;

impl BundleVerifier for Ed25519Verifier {
    fn verify(&self, bundle_dir: &Path) -> Result<VerifiedBundle, SigError> {
        let bundle_name: String = bundle_dir.display().to_string();

        let sig_path: std::path::PathBuf = bundle_dir.join(SIG_FILE_NAME);
        if !sig_path.exists() {
            return Err(SigError::MissingSignature {
                bundle: bundle_name,
            });
        }

        let sig_data: Vec<u8> =
            std::fs::read(&sig_path).map_err(|e: std::io::Error| SigError::Io {
                path: sig_path.display().to_string(),
                source: e,
            })?;

        let bundle_sig: BundleSig = BundleSig::parse(&sig_data, &bundle_name)?;

        let digest: [u8; 32] = canonical_digest(bundle_dir)?;

        bundle_sig
            .verifying_key
            .verify_strict(&digest, &bundle_sig.signature)
            .map_err(
                |e: ed25519_dalek::SignatureError| SigError::SignatureMismatch {
                    bundle: bundle_name.clone(),
                    reason: e.to_string(),
                },
            )?;

        Ok(VerifiedBundle {
            verifying_key: bundle_sig.verifying_key,
            bundle_dir: bundle_dir.to_path_buf(),
        })
    }
}

/// Verify a bundle using the default `Ed25519Verifier`.
///
/// Convenience wrapper — equivalent to `Ed25519Verifier.verify(bundle_dir)`.
pub fn verify_bundle(bundle_dir: &Path) -> Result<VerifiedBundle, SigError> {
    Ed25519Verifier.verify(bundle_dir)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use std::fs;

    use ed25519_dalek::Signer;
    use ed25519_dalek::SigningKey;
    use tempfile::TempDir;

    use super::*;
    use crate::key::generate_keypair;
    use crate::sig::sign_bundle;

    fn write_test_bundle(dir: &Path) {
        fs::write(dir.join("manifest.toml"), b"[bundle]\nname = \"test\"\n")
            .expect("write manifest");
        fs::write(dir.join("artifact.so"), b"\x7fELF stub bytes").expect("write artifact");
    }

    #[test]
    fn sign_then_verify_roundtrip() {
        let tmp: TempDir = TempDir::new().expect("tmp dir");
        write_test_bundle(tmp.path());

        let (signing_key, _): (SigningKey, VerifyingKey) = generate_keypair();
        sign_bundle(tmp.path(), &signing_key).expect("sign");

        let result: VerifiedBundle = verify_bundle(tmp.path()).expect("verify");
        assert_eq!(result.bundle_dir, tmp.path());
    }

    #[test]
    fn tampered_artifact_fails_verification() {
        let tmp: TempDir = TempDir::new().expect("tmp dir");
        write_test_bundle(tmp.path());

        let (signing_key, _): (SigningKey, VerifyingKey) = generate_keypair();
        sign_bundle(tmp.path(), &signing_key).expect("sign");

        // Flip a byte in the artifact AFTER signing.
        fs::write(tmp.path().join("artifact.so"), b"TAMPERED DATA").expect("overwrite artifact");

        assert!(matches!(
            verify_bundle(tmp.path()),
            Err(SigError::SignatureMismatch { .. })
        ));
    }

    #[test]
    fn tampered_manifest_fails_verification() {
        let tmp: TempDir = TempDir::new().expect("tmp dir");
        write_test_bundle(tmp.path());

        let (signing_key, _): (SigningKey, VerifyingKey) = generate_keypair();
        sign_bundle(tmp.path(), &signing_key).expect("sign");

        fs::write(
            tmp.path().join("manifest.toml"),
            b"[bundle]\nname = \"EVIL\"\n",
        )
        .expect("overwrite manifest");

        assert!(matches!(
            verify_bundle(tmp.path()),
            Err(SigError::SignatureMismatch { .. })
        ));
    }

    #[test]
    fn missing_sig_file_returns_missing_signature_error() {
        let tmp: TempDir = TempDir::new().expect("tmp dir");
        write_test_bundle(tmp.path());

        assert!(matches!(
            verify_bundle(tmp.path()),
            Err(SigError::MissingSignature { .. })
        ));
    }

    #[test]
    fn wrong_key_fails_verification() {
        let tmp: TempDir = TempDir::new().expect("tmp dir");
        write_test_bundle(tmp.path());

        // Sign the digest with key B but embed key A's verifying key in bundle.sig.
        // The signature is well-formed yet cannot validate under the embedded key.
        let (signing_key_b, _): (SigningKey, VerifyingKey) = generate_keypair();
        let (_, verifying_key_a): (SigningKey, VerifyingKey) = generate_keypair();

        let digest: [u8; 32] = canonical_digest(tmp.path()).expect("digest");
        let mismatched: BundleSig = BundleSig {
            verifying_key: verifying_key_a,
            signature: signing_key_b.sign(&digest),
        };
        let sig_path: std::path::PathBuf = tmp.path().join(SIG_FILE_NAME);
        fs::write(&sig_path, mismatched.serialize()).expect("write mismatched sig");

        assert!(matches!(
            verify_bundle(tmp.path()),
            Err(SigError::SignatureMismatch { .. })
        ));
    }

    #[test]
    fn corrupt_sig_magic_returns_bad_magic_error() {
        let tmp: TempDir = TempDir::new().expect("tmp dir");
        write_test_bundle(tmp.path());

        let (signing_key, _): (SigningKey, VerifyingKey) = generate_keypair();
        sign_bundle(tmp.path(), &signing_key).expect("sign");

        let sig_path: std::path::PathBuf = tmp.path().join(SIG_FILE_NAME);
        let mut bytes: Vec<u8> = fs::read(&sig_path).expect("read sig");
        bytes[0] = 0xFF;
        fs::write(&sig_path, &bytes).expect("write corrupted sig");

        assert!(matches!(
            verify_bundle(tmp.path()),
            Err(SigError::BadMagic { .. })
        ));
    }
}
