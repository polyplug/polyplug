//! Key generation and serialization for Ed25519 signing keys.
//!
//! # Key file format
//!
//! Both signing and verifying key files share the same 39-byte layout:
//!
//! ```text
//! Offset  Size  Description
//! 0       6     Magic: b"PPKEY\0"
//! 6       1     Key type: 0x01 = signing key, 0x02 = verifying key
//! 7       32    Key bytes (signing key seed or verifying key point)
//! Total:  39 bytes
//! ```

use ed25519_dalek::{SigningKey, VerifyingKey};
use rand_core::OsRng;
#[cfg(unix)]
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use crate::SigError;

/// Magic prefix for all polyplug key files.
const KEY_MAGIC: &[u8; 6] = b"PPKEY\0";

/// Byte length of a serialized key file.
const KEY_FILE_LEN: usize = 6 + 1 + 32; // magic + type + key bytes = 39

/// Discriminator values for the key type byte.
pub(crate) enum KeyType {}

impl KeyType {
    pub(crate) const SIGNING: u8 = 0x01;
    pub(crate) const VERIFYING: u8 = 0x02;
}

/// Generate a fresh Ed25519 keypair using the OS random source.
///
/// Returns `(signing_key, verifying_key)`.
pub fn generate_keypair() -> (SigningKey, VerifyingKey) {
    let signing_key: SigningKey = SigningKey::generate(&mut OsRng);
    let verifying_key: VerifyingKey = signing_key.verifying_key();
    (signing_key, verifying_key)
}

/// Serialize an Ed25519 signing key to the documented 39-byte key file format.
pub fn serialize_signing_key(signing_key: &SigningKey) -> [u8; KEY_FILE_LEN] {
    let mut buf: [u8; KEY_FILE_LEN] = [0u8; KEY_FILE_LEN];
    buf[..6].copy_from_slice(KEY_MAGIC);
    buf[6] = KeyType::SIGNING;
    buf[7..].copy_from_slice(&signing_key.to_bytes());
    buf
}

/// Serialize an Ed25519 verifying key to the documented 39-byte key file format.
pub fn serialize_verifying_key(verifying_key: &VerifyingKey) -> [u8; KEY_FILE_LEN] {
    let mut buf: [u8; KEY_FILE_LEN] = [0u8; KEY_FILE_LEN];
    buf[..6].copy_from_slice(KEY_MAGIC);
    buf[6] = KeyType::VERIFYING;
    buf[7..].copy_from_slice(verifying_key.as_bytes());
    buf
}

/// Parse a signing key from the 39-byte key file format.
pub fn parse_signing_key(data: &[u8]) -> Result<SigningKey, SigError> {
    if data.len() != KEY_FILE_LEN {
        return Err(SigError::MalformedKeyLength {
            expected: KEY_FILE_LEN,
            found: data.len(),
        });
    }
    if &data[..6] != KEY_MAGIC {
        return Err(SigError::BadKeyMagic);
    }
    if data[6] != KeyType::SIGNING {
        return Err(SigError::BadKeyType { key_type: data[6] });
    }
    let key_bytes: [u8; 32] =
        data[7..39]
            .try_into()
            .map_err(
                |_: core::array::TryFromSliceError| SigError::InvalidKeyData {
                    reason: "key bytes slice has wrong length".to_owned(),
                },
            )?;
    Ok(SigningKey::from_bytes(&key_bytes))
}

/// Parse a verifying key from the 39-byte key file format.
pub fn parse_verifying_key(data: &[u8]) -> Result<VerifyingKey, SigError> {
    if data.len() != KEY_FILE_LEN {
        return Err(SigError::MalformedKeyLength {
            expected: KEY_FILE_LEN,
            found: data.len(),
        });
    }
    if &data[..6] != KEY_MAGIC {
        return Err(SigError::BadKeyMagic);
    }
    if data[6] != KeyType::VERIFYING {
        return Err(SigError::BadKeyType { key_type: data[6] });
    }
    let key_bytes: [u8; 32] =
        data[7..39]
            .try_into()
            .map_err(
                |_: core::array::TryFromSliceError| SigError::InvalidKeyData {
                    reason: "key bytes slice has wrong length".to_owned(),
                },
            )?;
    VerifyingKey::from_bytes(&key_bytes).map_err(|e: ed25519_dalek::SignatureError| {
        SigError::InvalidKeyData {
            reason: e.to_string(),
        }
    })
}

/// Write a signing key to `path` in the documented key file format.
///
/// The caller is responsible for creating `path` with restrictive permissions
/// (e.g., `0o600` on Unix) before distributing or using the file.
#[cfg(unix)]
pub fn save_signing_key(path: &std::path::Path, signing_key: &SigningKey) -> Result<(), SigError> {
    let buf: [u8; KEY_FILE_LEN] = serialize_signing_key(signing_key);

    // Open with mode 0o600 so a freshly-created file is never world-readable.
    let mut file: std::fs::File = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .map_err(|e: std::io::Error| SigError::Io {
            path: path.display().to_string(),
            source: e,
        })?;

    // `.mode(0o600)` only applies on creation; a pre-existing file keeps its old
    // (possibly 0o644) mode. Force 0o600 on the open handle BEFORE writing any
    // secret bytes, so the seed never exists on disk with broader-than-0600 perms.
    let permissions: std::fs::Permissions = std::fs::Permissions::from_mode(0o600);
    file.set_permissions(permissions)
        .map_err(|e: std::io::Error| SigError::Io {
            path: path.display().to_string(),
            source: e,
        })?;

    file.write_all(&buf)
        .map_err(|e: std::io::Error| SigError::Io {
            path: path.display().to_string(),
            source: e,
        })?;

    Ok(())
}

/// Write a signing key to `path` in the documented key file format.
///
/// The caller is responsible for creating `path` with restrictive permissions
/// before distributing or using the file.
///
/// On non-Unix platforms this OS provides no file-mode restriction here: the
/// secret seed is written with the platform's default permissions and the caller
/// must rely on directory ACLs or other OS-specific mechanisms to protect it.
#[cfg(not(unix))]
pub fn save_signing_key(path: &std::path::Path, signing_key: &SigningKey) -> Result<(), SigError> {
    let buf: [u8; KEY_FILE_LEN] = serialize_signing_key(signing_key);
    std::fs::write(path, buf).map_err(|e: std::io::Error| SigError::Io {
        path: path.display().to_string(),
        source: e,
    })
}

/// Write a verifying key to `path` in the documented key file format.
pub fn save_verifying_key(
    path: &std::path::Path,
    verifying_key: &VerifyingKey,
) -> Result<(), SigError> {
    let buf: [u8; KEY_FILE_LEN] = serialize_verifying_key(verifying_key);
    std::fs::write(path, buf).map_err(|e: std::io::Error| SigError::Io {
        path: path.display().to_string(),
        source: e,
    })
}

/// Read and parse a signing key from `path`.
pub fn load_signing_key(path: &std::path::Path) -> Result<SigningKey, SigError> {
    let data: Vec<u8> = std::fs::read(path).map_err(|e: std::io::Error| SigError::Io {
        path: path.display().to_string(),
        source: e,
    })?;
    parse_signing_key(&data)
}

/// Read and parse a verifying key from `path`.
pub fn load_verifying_key(path: &std::path::Path) -> Result<VerifyingKey, SigError> {
    let data: Vec<u8> = std::fs::read(path).map_err(|e: std::io::Error| SigError::Io {
        path: path.display().to_string(),
        source: e,
    })?;
    parse_verifying_key(&data)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use super::*;

    #[test]
    fn keygen_roundtrip_signing_key() {
        let (signing_key, _): (SigningKey, VerifyingKey) = generate_keypair();
        let serialized: [u8; KEY_FILE_LEN] = serialize_signing_key(&signing_key);
        let parsed: SigningKey = parse_signing_key(&serialized).expect("parse signing key");
        assert_eq!(signing_key.to_bytes(), parsed.to_bytes());
    }

    #[test]
    fn keygen_roundtrip_verifying_key() {
        let (_, verifying_key): (SigningKey, VerifyingKey) = generate_keypair();
        let serialized: [u8; KEY_FILE_LEN] = serialize_verifying_key(&verifying_key);
        let parsed: VerifyingKey = parse_verifying_key(&serialized).expect("parse verifying key");
        assert_eq!(verifying_key.as_bytes(), parsed.as_bytes());
    }

    #[test]
    fn wrong_key_type_byte_is_rejected() {
        let (signing_key, _): (SigningKey, VerifyingKey) = generate_keypair();
        let mut buf: [u8; KEY_FILE_LEN] = serialize_signing_key(&signing_key);
        // Flip the type byte to verifying — parsing as signing key must fail.
        buf[6] = KeyType::VERIFYING;
        assert!(matches!(
            parse_signing_key(&buf),
            Err(SigError::BadKeyType { .. })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn save_signing_key_creates_file_with_0o600_mode() {
        use std::os::unix::fs::PermissionsExt;

        let tmp: tempfile::TempDir = tempfile::TempDir::new().expect("tmp dir");
        let path: std::path::PathBuf = tmp.path().join("signing.key");
        let (signing_key, _): (SigningKey, VerifyingKey) = generate_keypair();

        save_signing_key(&path, &signing_key).expect("save signing key");

        let mode: u32 = std::fs::metadata(&path)
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(
            mode & 0o777,
            0o600,
            "private key file must be exactly 0o600, got {:o}",
            mode & 0o777
        );
    }

    #[cfg(unix)]
    #[test]
    fn save_signing_key_tightens_preexisting_world_readable_file() {
        use std::os::unix::fs::PermissionsExt;

        let tmp: tempfile::TempDir = tempfile::TempDir::new().expect("tmp dir");
        let path: std::path::PathBuf = tmp.path().join("signing.key");

        // Pre-create a 0o644 file at the target path.
        std::fs::write(&path, b"stale").expect("pre-create file");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).expect("set 0o644");

        let (signing_key, _): (SigningKey, VerifyingKey) = generate_keypair();
        save_signing_key(&path, &signing_key).expect("overwrite signing key");

        let mode: u32 = std::fs::metadata(&path)
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(
            mode & 0o777,
            0o600,
            "overwriting a 0o644 file must tighten it to 0o600, got {:o}",
            mode & 0o777
        );
    }

    #[test]
    fn bad_magic_is_rejected() {
        let (signing_key, _): (SigningKey, VerifyingKey) = generate_keypair();
        let mut buf: [u8; KEY_FILE_LEN] = serialize_signing_key(&signing_key);
        buf[0] = 0xFF;
        assert!(matches!(
            parse_signing_key(&buf),
            Err(SigError::BadKeyMagic)
        ));
    }
}
