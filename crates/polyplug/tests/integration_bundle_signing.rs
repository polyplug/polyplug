#![allow(clippy::expect_used)]

//! Integration tests: bundle signature enforcement via `SignaturePolicy`.
//!
//! Uses a `NoopLoader` (identical pattern to `integration_version.rs`) so the
//! native artifact is never dlopened. Signature checks run before the loader,
//! so all policy paths are exercised without real native libraries.

use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

use ed25519_dalek::SigningKey;
use ed25519_dalek::VerifyingKey;
use polyplug::error::LoaderError;
use polyplug::error::RuntimeError;
use polyplug::loader::BundleLoader;
use polyplug::loader::BundleSource;
use polyplug::loader::ManifestData;
use polyplug::runtime::Runtime;
use polyplug_abi::SupportedLanguage;
use polyplug_abi::runtime::SignaturePolicy;
use polyplug_abi::types::LogLevel;
use polyplug_signing::{generate_keypair, sign_bundle};
use polyplug_utils::bundle_id as compute_bundle_id;
use tempfile::TempDir;

// ─── NoopLoader ──────────────────────────────────────────────────────────────

struct NoopLoader;

impl BundleLoader for NoopLoader {
    fn loader_name(&self) -> &'static str {
        "noop"
    }

    fn loader_language(&self) -> SupportedLanguage {
        SupportedLanguage::Rust
    }

    fn supports_hot_reload(&self) -> bool {
        false
    }

    fn load(
        &self,
        _manifest: &ManifestData,
        _source: &BundleSource,
        _runtime: &Runtime,
    ) -> Result<(), polyplug::error::LoaderError> {
        Ok(())
    }

    fn reload(
        &self,
        _manifest: &ManifestData,
        _runtime: &Runtime,
    ) -> Result<(), polyplug::error::LoaderError> {
        Err(polyplug::error::LoaderError::HotReloadUnsupported {
            loader_name: self.loader_name().to_owned(),
        })
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Write a minimal stub bundle accepted by the runtime's manifest validator.
fn write_stub_bundle(root: &Path, name: &str) -> PathBuf {
    let bundle_dir: PathBuf = root.join(name);
    fs::create_dir_all(&bundle_dir).expect("create bundle dir");

    let artifact_name: String = format!("{name}.so");
    fs::write(bundle_dir.join(&artifact_name), b"\x7fELF stub").expect("write stub artifact");

    let manifest_toml: String = format!(
        "id = {}\nname = \"{name}\"\nloader = \"noop\"\nfile = \"{artifact_name}\"\nversion = \"1.0.0\"\n",
        compute_bundle_id(name),
    );
    fs::write(bundle_dir.join("manifest.toml"), manifest_toml).expect("write manifest.toml");

    bundle_dir
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[test]
fn policy_off_unsigned_bundle_loads_ok() {
    let tmp: TempDir = TempDir::new().expect("tmp dir");
    write_stub_bundle(tmp.path(), "unsigned_bundle");

    let result: Result<Arc<Runtime>, RuntimeError> = Runtime::builder()
        .loader(NoopLoader)
        .plugin_dir(tmp.path().to_path_buf())
        .signature_policy(SignaturePolicy::Off)
        .build();

    assert!(
        result.is_ok(),
        "Off policy: unsigned bundle must load OK; got: {:?}",
        result.err()
    );
}

#[test]
fn policy_required_valid_signed_bundle_loads_ok() {
    let tmp: TempDir = TempDir::new().expect("tmp dir");
    let bundle_dir: PathBuf = write_stub_bundle(tmp.path(), "signed_bundle");

    let (signing_key, _): (SigningKey, VerifyingKey) = generate_keypair();
    sign_bundle(&bundle_dir, &signing_key).expect("sign bundle");

    let result: Result<Arc<Runtime>, RuntimeError> = Runtime::builder()
        .loader(NoopLoader)
        .plugin_dir(tmp.path().to_path_buf())
        .signature_policy(SignaturePolicy::Required)
        .build();

    assert!(
        result.is_ok(),
        "Required policy: validly signed bundle must load OK; got: {:?}",
        result.err()
    );
}

#[test]
fn policy_required_unsigned_bundle_returns_unsigned_bundle_error() {
    let tmp: TempDir = TempDir::new().expect("tmp dir");
    write_stub_bundle(tmp.path(), "unsigned_bundle");

    let result: Result<Arc<Runtime>, RuntimeError> = Runtime::builder()
        .loader(NoopLoader)
        .plugin_dir(tmp.path().to_path_buf())
        .signature_policy(SignaturePolicy::Required)
        .build();

    assert!(
        matches!(
            result,
            Err(RuntimeError::Loader(LoaderError::UnsignedBundle { .. }))
        ),
        "Required policy: unsigned bundle must fail with UnsignedBundle; got: {:?}",
        result.err()
    );
}

#[test]
fn policy_required_tampered_artifact_returns_verification_failed_error() {
    let tmp: TempDir = TempDir::new().expect("tmp dir");
    let bundle_dir: PathBuf = write_stub_bundle(tmp.path(), "tampered_bundle");

    let (signing_key, _): (SigningKey, VerifyingKey) = generate_keypair();
    sign_bundle(&bundle_dir, &signing_key).expect("sign bundle");

    // Tamper the artifact AFTER signing so the digest no longer matches.
    fs::write(bundle_dir.join("tampered_bundle.so"), b"TAMPERED BYTES")
        .expect("overwrite artifact");

    let result: Result<Arc<Runtime>, RuntimeError> = Runtime::builder()
        .loader(NoopLoader)
        .plugin_dir(tmp.path().to_path_buf())
        .signature_policy(SignaturePolicy::Required)
        .build();

    assert!(
        matches!(
            result,
            Err(RuntimeError::Loader(
                LoaderError::SignatureVerificationFailed { .. }
            ))
        ),
        "Required policy: tampered bundle must fail with SignatureVerificationFailed; got: {:?}",
        result.err()
    );
}

#[test]
fn policy_warn_only_unsigned_bundle_loads_ok_and_emits_warning() {
    let tmp: TempDir = TempDir::new().expect("tmp dir");
    write_stub_bundle(tmp.path(), "unsigned_bundle");

    let warnings: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let warnings_clone: Arc<Mutex<Vec<String>>> = Arc::clone(&warnings);

    let result: Result<Arc<Runtime>, RuntimeError> = Runtime::builder()
        .loader(NoopLoader)
        .plugin_dir(tmp.path().to_path_buf())
        .signature_policy(SignaturePolicy::WarnOnly)
        .logger(move |level: LogLevel, _scope: &str, msg: &str| {
            if level == LogLevel::Warn {
                warnings_clone
                    .lock()
                    .expect("warnings lock")
                    .push(msg.to_owned());
            }
        })
        .build();

    assert!(
        result.is_ok(),
        "WarnOnly policy: unsigned bundle must load OK; got: {:?}",
        result.err()
    );

    let captured: Vec<String> = warnings.lock().expect("warnings lock").clone();
    assert!(
        captured
            .iter()
            .any(|msg: &String| msg.contains("signature check failed")),
        "WarnOnly policy: expected a warning about missing/invalid signature; got: {captured:?}"
    );
}
