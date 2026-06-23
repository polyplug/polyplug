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

/// Build a `VerifyingKey` from raw 32-byte Ed25519 public-key encoding.
///
/// Returns [`SigError::InvalidKeyData`] if the bytes are not a valid compressed
/// Edwards point. This is the entry point used to turn host-supplied trusted-key
/// bytes (`Ed25519PublicKey`) into verifying keys for [`PinnedKeyVerifier`].
pub fn verifying_key_from_bytes(bytes: &[u8; 32]) -> Result<VerifyingKey, SigError> {
    VerifyingKey::from_bytes(bytes).map_err(|e: ed25519_dalek::SignatureError| {
        SigError::InvalidKeyData {
            reason: e.to_string(),
        }
    })
}

/// Key-pinning verifier: performs the normal Ed25519 verification, then requires
/// the bundle's embedded verifying key to be a member of a host-trusted set.
///
/// # Empty set = reject-all
/// A `PinnedKeyVerifier` with an empty trusted set rejects EVERY bundle (no key
/// can be a member), so [`PinnedKeyVerifier::new`] is documented to be called
/// only when at least one key exists. The runtime constructs this verifier
/// exclusively when the host's allowlist is non-empty; an empty allowlist stays
/// on the TOFU [`Ed25519Verifier`] path instead.
pub struct PinnedKeyVerifier {
    trusted: Vec<VerifyingKey>,
}

impl PinnedKeyVerifier {
    /// Create a key-pinning verifier from a set of trusted verifying keys.
    ///
    /// An empty `keys` vector yields a verifier that rejects every bundle (see
    /// the type-level docs) — callers must supply at least one key.
    pub fn new(keys: Vec<VerifyingKey>) -> PinnedKeyVerifier {
        PinnedKeyVerifier { trusted: keys }
    }
}

impl BundleVerifier for PinnedKeyVerifier {
    fn verify(&self, bundle_dir: &Path) -> Result<VerifiedBundle, SigError> {
        // First run the full TOFU verification: this proves the bundle is
        // internally consistent and untampered against its embedded key.
        let verified: VerifiedBundle = Ed25519Verifier.verify(bundle_dir)?;

        // Then enforce authenticity: the embedded key must be in the allowlist.
        let is_trusted: bool = self
            .trusted
            .iter()
            .any(|trusted: &VerifyingKey| trusted.as_bytes() == verified.verifying_key.as_bytes());

        if is_trusted {
            Ok(verified)
        } else {
            Err(SigError::UntrustedKey {
                bundle: bundle_dir.display().to_string(),
            })
        }
    }
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
    fn tampered_nested_subdirectory_file_fails_verification() {
        let tmp: TempDir = TempDir::new().expect("tmp dir");
        write_test_bundle(tmp.path());

        // Add a file in a nested subdirectory, then sign.
        let nested: std::path::PathBuf = tmp.path().join("lib").join("inner");
        fs::create_dir_all(&nested).expect("create nested dirs");
        fs::write(nested.join("deep.so"), b"deep bytes").expect("write deep");

        let (signing_key, _): (SigningKey, VerifyingKey) = generate_keypair();
        sign_bundle(tmp.path(), &signing_key).expect("sign");

        // The sign→verify roundtrip with the new digest format must still pass.
        verify_bundle(tmp.path()).expect("verify after sign with nested file");

        // Tamper the nested file AFTER signing → verification must fail.
        fs::write(nested.join("deep.so"), b"DEEP TAMPERED").expect("rewrite deep");

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
    fn pinned_verifier_accepts_bundle_whose_key_is_in_the_set() {
        let tmp: TempDir = TempDir::new().expect("tmp dir");
        write_test_bundle(tmp.path());

        let (signing_key, verifying_key): (SigningKey, VerifyingKey) = generate_keypair();
        sign_bundle(tmp.path(), &signing_key).expect("sign");

        // The signing key's public key IS in the trusted set → accept.
        let verifier: PinnedKeyVerifier = PinnedKeyVerifier::new(vec![verifying_key]);
        let result: VerifiedBundle = verifier.verify(tmp.path()).expect("pinned verify accepts");
        assert_eq!(result.bundle_dir, tmp.path());
    }

    #[test]
    fn pinned_verifier_rejects_bundle_whose_key_is_not_in_the_set() {
        let tmp: TempDir = TempDir::new().expect("tmp dir");
        write_test_bundle(tmp.path());

        // Sign with key A, but pin only an unrelated key B.
        let (signing_key_a, _): (SigningKey, VerifyingKey) = generate_keypair();
        sign_bundle(tmp.path(), &signing_key_a).expect("sign");
        let (_, verifying_key_b): (SigningKey, VerifyingKey) = generate_keypair();

        let verifier: PinnedKeyVerifier = PinnedKeyVerifier::new(vec![verifying_key_b]);
        assert!(matches!(
            verifier.verify(tmp.path()),
            Err(SigError::UntrustedKey { .. })
        ));
    }

    #[test]
    fn pinned_verifier_rejects_a_normally_valid_tofu_bundle_when_key_excluded() {
        let tmp: TempDir = TempDir::new().expect("tmp dir");
        write_test_bundle(tmp.path());

        let (signing_key, _): (SigningKey, VerifyingKey) = generate_keypair();
        sign_bundle(tmp.path(), &signing_key).expect("sign");

        // The bundle passes the default TOFU verifier...
        verify_bundle(tmp.path()).expect("tofu verify accepts");

        // ...but a pinned verifier whose set excludes its key rejects it.
        let (_, unrelated): (SigningKey, VerifyingKey) = generate_keypair();
        let verifier: PinnedKeyVerifier = PinnedKeyVerifier::new(vec![unrelated]);
        assert!(matches!(
            verifier.verify(tmp.path()),
            Err(SigError::UntrustedKey { .. })
        ));
    }

    #[test]
    fn verifying_key_from_bytes_roundtrips_a_real_key() {
        let (_, verifying_key): (SigningKey, VerifyingKey) = generate_keypair();
        let rebuilt: VerifyingKey =
            verifying_key_from_bytes(verifying_key.as_bytes()).expect("rebuild key");
        assert_eq!(rebuilt.as_bytes(), verifying_key.as_bytes());
    }

    #[test]
    fn verifying_key_from_bytes_rejects_invalid_point() {
        // A 32-byte y-coordinate whose Edwards point cannot be decompressed: the
        // x-recovery has no solution, so dalek's `from_bytes` rejects it. (dalek
        // accepts most arbitrary 32-byte inputs lazily, so this is a verified
        // non-decompressible encoding rather than a "random bytes" guess.)
        let bad: [u8; 32] = [
            19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40,
            41, 42, 43, 44, 45, 46, 47, 48, 49, 50,
        ];
        assert!(matches!(
            verifying_key_from_bytes(&bad),
            Err(SigError::InvalidKeyData { .. })
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
