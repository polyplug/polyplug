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
use polyplug::runtime::Runtime;
use polyplug_abi::Array;
use polyplug_abi::RuntimeConfig;
use polyplug_abi::SupportedLanguage;
use polyplug_abi::runtime::SignaturePolicy;
use polyplug_abi::types::Ed25519PublicKey;
use polyplug_abi::types::LogLevel;
use polyplug_common::ManifestData;
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
    ) -> Result<(), LoaderError> {
        Ok(())
    }

    fn reload(&self, _manifest: &ManifestData, _runtime: &Runtime) -> Result<(), LoaderError> {
        Err(LoaderError::HotReloadUnsupported {
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

/// A bundle whose artifact is a symlink must be rejected under Required policy:
/// the canonical digest refuses symlinks (a symlinked artifact would be loaded
/// but never covered by the signature), so verification fails.
#[test]
#[cfg(unix)]
fn policy_required_symlinked_artifact_returns_verification_failed_error() {
    use std::os::unix::fs::symlink;

    let tmp: TempDir = TempDir::new().expect("tmp dir");
    let bundle_dir: PathBuf = tmp.path().join("symlink_bundle");
    fs::create_dir_all(&bundle_dir).expect("create bundle dir");

    // A real DLL inside the bundle plus a symlink artifact pointing at it.
    fs::write(bundle_dir.join("real.so"), b"\x7fELF stub").expect("write real artifact");
    symlink(
        bundle_dir.join("real.so"),
        bundle_dir.join("symlink_bundle.so"),
    )
    .expect("create symlink artifact");

    let manifest_toml: String = format!(
        "id = {}\nname = \"symlink_bundle\"\nloader = \"noop\"\nfile = \"symlink_bundle.so\"\nversion = \"1.0.0\"\n",
        compute_bundle_id("symlink_bundle"),
    );
    fs::write(bundle_dir.join("manifest.toml"), manifest_toml).expect("write manifest.toml");

    // Sign BEFORE the symlink exists is impossible (the digest refuses it); so we
    // attempt to sign — signing must itself fail on the symlink. Either way the
    // bundle cannot be validly signed, and Required verification rejects it.
    let (signing_key, _): (SigningKey, VerifyingKey) = generate_keypair();
    let _ = sign_bundle(&bundle_dir, &signing_key);

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
            )) | Err(RuntimeError::Loader(LoaderError::UnsignedBundle { .. }))
        ),
        "Required policy: symlinked artifact must be rejected; got: {:?}",
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

// ─── Key pinning (RuntimeConfig::trusted_keys) ─────────────────────────────────

#[test]
fn policy_required_pinned_key_accepts_bundle_signed_with_trusted_key() {
    let tmp: TempDir = TempDir::new().expect("tmp dir");
    let bundle_dir: PathBuf = write_stub_bundle(tmp.path(), "pinned_ok");

    let (signing_key, verifying_key): (SigningKey, VerifyingKey) = generate_keypair();
    sign_bundle(&bundle_dir, &signing_key).expect("sign bundle");

    // Pin exactly the key that signed the bundle → accept.
    let result: Result<Arc<Runtime>, RuntimeError> = Runtime::builder()
        .loader(NoopLoader)
        .plugin_dir(tmp.path().to_path_buf())
        .signature_policy(SignaturePolicy::Required)
        .trusted_keys(&[verifying_key])
        .build();

    assert!(
        result.is_ok(),
        "Required+pinned: bundle signed by a trusted key must load OK; got: {:?}",
        result.err()
    );
}

#[test]
fn policy_required_pinned_key_rejects_bundle_signed_with_untrusted_key() {
    let tmp: TempDir = TempDir::new().expect("tmp dir");
    let bundle_dir: PathBuf = write_stub_bundle(tmp.path(), "pinned_untrusted");

    // Sign with key A but pin only an unrelated key B.
    let (signing_key_a, _): (SigningKey, VerifyingKey) = generate_keypair();
    sign_bundle(&bundle_dir, &signing_key_a).expect("sign bundle");
    let (_, verifying_key_b): (SigningKey, VerifyingKey) = generate_keypair();

    let result: Result<Arc<Runtime>, RuntimeError> = Runtime::builder()
        .loader(NoopLoader)
        .plugin_dir(tmp.path().to_path_buf())
        .signature_policy(SignaturePolicy::Required)
        .trusted_keys(&[verifying_key_b])
        .build();

    assert!(
        matches!(
            result,
            Err(RuntimeError::Loader(
                LoaderError::UntrustedSigningKey { .. }
            ))
        ),
        "Required+pinned: an untrusted signing key must fail with UntrustedSigningKey; got: {:?}",
        result.err()
    );
}

#[test]
fn policy_warn_only_pinned_key_loads_untrusted_bundle_and_warns() {
    let tmp: TempDir = TempDir::new().expect("tmp dir");
    let bundle_dir: PathBuf = write_stub_bundle(tmp.path(), "pinned_warn");

    let (signing_key_a, _): (SigningKey, VerifyingKey) = generate_keypair();
    sign_bundle(&bundle_dir, &signing_key_a).expect("sign bundle");
    let (_, verifying_key_b): (SigningKey, VerifyingKey) = generate_keypair();

    let warnings: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let warnings_clone: Arc<Mutex<Vec<String>>> = Arc::clone(&warnings);

    let result: Result<Arc<Runtime>, RuntimeError> = Runtime::builder()
        .loader(NoopLoader)
        .plugin_dir(tmp.path().to_path_buf())
        .signature_policy(SignaturePolicy::WarnOnly)
        .trusted_keys(&[verifying_key_b])
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
        "WarnOnly+pinned: an untrusted bundle must still load; got: {:?}",
        result.err()
    );

    let captured: Vec<String> = warnings.lock().expect("warnings lock").clone();
    assert!(
        captured
            .iter()
            .any(|msg: &String| msg.contains("signature check failed")),
        "WarnOnly+pinned: expected a warning about the untrusted key; got: {captured:?}"
    );
}

#[test]
fn policy_off_ignores_pinned_keys() {
    let tmp: TempDir = TempDir::new().expect("tmp dir");
    write_stub_bundle(tmp.path(), "off_pinned");

    // An unrelated pinned key is irrelevant under Off: no verification runs.
    let (_, unrelated): (SigningKey, VerifyingKey) = generate_keypair();

    let result: Result<Arc<Runtime>, RuntimeError> = Runtime::builder()
        .loader(NoopLoader)
        .plugin_dir(tmp.path().to_path_buf())
        .signature_policy(SignaturePolicy::Off)
        .trusted_keys(&[unrelated])
        .build();

    assert!(
        result.is_ok(),
        "Off policy: an unsigned bundle must load even with pinned keys set; got: {:?}",
        result.err()
    );
}

// ─── Key pinning via the FFI / `config()` path (non-Rust hosts) ─────────────────
//
// Non-Rust hosts (cpp/csharp/python/lua/js) do not call the Rust `trusted_keys()`
// builder API — they populate `RuntimeConfig.trusted_keys` directly and hand the
// config to `polyplug_runtime_create`, which forwards it through
// `RuntimeBuilder::config()`. These tests exercise that exact path to prove the
// host-supplied `Array` survives `build()` (it is NOT overwritten by the empty
// Rust-API key set) and that pinning is enforced. The backing `Vec` is kept alive
// for the runtime's whole lifetime per the `RuntimeConfig.trusted_keys` ownership
// contract.

/// Drive the runtime through the `config()` path with a host-owned trusted-key
/// buffer — the same path the FFI `polyplug_runtime_create` takes.
fn build_via_config_with_trusted_keys(
    plugin_dir: PathBuf,
    policy: SignaturePolicy,
    keys: &[Ed25519PublicKey],
) -> Result<Arc<Runtime>, RuntimeError> {
    let trusted_keys: Array<Ed25519PublicKey> = if keys.is_empty() {
        Array::empty()
    } else {
        Array::new(keys.as_ptr() as *mut Ed25519PublicKey, keys.len())
    };
    let cfg: RuntimeConfig = RuntimeConfig {
        signature_policy: policy,
        trusted_keys,
        ..Default::default()
    };
    Runtime::builder()
        .loader(NoopLoader)
        .plugin_dir(plugin_dir)
        .config(cfg)
        .build()
}

#[test]
fn config_path_pinned_key_accepts_bundle_signed_with_trusted_key() {
    let tmp: TempDir = TempDir::new().expect("tmp dir");
    let bundle_dir: PathBuf = write_stub_bundle(tmp.path(), "cfg_pinned_ok");

    let (signing_key, verifying_key): (SigningKey, VerifyingKey) = generate_keypair();
    sign_bundle(&bundle_dir, &signing_key).expect("sign bundle");

    // Host-owned key buffer; must outlive the runtime returned below.
    let keys: Vec<Ed25519PublicKey> = vec![Ed25519PublicKey {
        bytes: *verifying_key.as_bytes(),
    }];

    let result: Result<Arc<Runtime>, RuntimeError> = build_via_config_with_trusted_keys(
        tmp.path().to_path_buf(),
        SignaturePolicy::Required,
        &keys,
    );

    assert!(
        result.is_ok(),
        "config()+pinned: bundle signed by a host-trusted key must load OK; got: {:?}",
        result.err()
    );
}

#[test]
fn config_path_pinned_key_rejects_bundle_signed_with_untrusted_key() {
    let tmp: TempDir = TempDir::new().expect("tmp dir");
    let bundle_dir: PathBuf = write_stub_bundle(tmp.path(), "cfg_pinned_untrusted");

    // Sign with key A but pin only an unrelated key B via the host config.
    let (signing_key_a, _): (SigningKey, VerifyingKey) = generate_keypair();
    sign_bundle(&bundle_dir, &signing_key_a).expect("sign bundle");
    let (_, verifying_key_b): (SigningKey, VerifyingKey) = generate_keypair();

    let keys: Vec<Ed25519PublicKey> = vec![Ed25519PublicKey {
        bytes: *verifying_key_b.as_bytes(),
    }];

    let result: Result<Arc<Runtime>, RuntimeError> = build_via_config_with_trusted_keys(
        tmp.path().to_path_buf(),
        SignaturePolicy::Required,
        &keys,
    );

    // The bug this guards: if build() wiped the host-supplied trusted_keys, this
    // would fall back to TOFU and load OK instead of rejecting the untrusted key.
    assert!(
        matches!(
            result,
            Err(RuntimeError::Loader(
                LoaderError::UntrustedSigningKey { .. }
            ))
        ),
        "config()+pinned: an untrusted signing key must fail with UntrustedSigningKey; got: {:?}",
        result.err()
    );
}

/// Proves the documented `RuntimeConfig.trusted_keys` ownership contract: the
/// runtime COPIES the host's keys during `create`, so the host buffer is only
/// borrowed for that call and may be freed/reused afterward. The test pins an
/// untrusted key B via a host-owned buffer, builds the runtime (no `plugin_dir`,
/// so nothing loads during `build`), then OVERWRITES the host buffer in place with
/// the real signer's key A and loads a bundle signed by A. If the runtime had
/// retained the host pointer (instead of copying), it would now observe A and
/// accept; because it copied B at create, it still rejects. This deterministically
/// distinguishes copy-at-create from pointer-retention without relying on
/// allocator reuse.
#[test]
fn config_path_trusted_keys_are_copied_at_create_not_retained() {
    let tmp: TempDir = TempDir::new().expect("tmp dir");
    let bundle_dir: PathBuf = write_stub_bundle(tmp.path(), "cfg_copy_proof");

    let (signing_key_a, verifying_key_a): (SigningKey, VerifyingKey) = generate_keypair();
    sign_bundle(&bundle_dir, &signing_key_a).expect("sign bundle");
    let (_, verifying_key_b): (SigningKey, VerifyingKey) = generate_keypair();

    // Host-owned key buffer pinned to the UNTRUSTED key B.
    let mut keys: Vec<Ed25519PublicKey> = vec![Ed25519PublicKey {
        bytes: *verifying_key_b.as_bytes(),
    }];

    let cfg: RuntimeConfig = RuntimeConfig {
        signature_policy: SignaturePolicy::Required,
        trusted_keys: Array::new(keys.as_mut_ptr(), keys.len()),
        ..Default::default()
    };

    // No plugin_dir → build() copies the keys at create but loads nothing yet.
    let rt: Arc<Runtime> = Runtime::builder()
        .loader(NoopLoader)
        .config(cfg)
        .build()
        .expect("build runtime");

    // Mutate the host buffer to the REAL signer (key A) AFTER create. A runtime
    // that retained the host pointer would now see A and accept; a runtime that
    // copied B at create still holds B and must reject.
    keys[0] = Ed25519PublicKey {
        bytes: *verifying_key_a.as_bytes(),
    };

    let result: Result<(), RuntimeError> = rt.load_bundle(&bundle_dir);

    assert!(
        matches!(
            result,
            Err(RuntimeError::Loader(
                LoaderError::UntrustedSigningKey { .. }
            ))
        ),
        "copy-at-create: post-create mutation of the host buffer must NOT affect the \
         runtime's pinned set; expected UntrustedSigningKey, got: {:?}",
        result.err()
    );
}
