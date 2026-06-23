//! Ed25519 public-key bytes for the host-configured trusted-key allowlist.
//!
//! This is a plain 32-byte value carried in `RuntimeConfig::trusted_keys`. It is
//! the raw Ed25519 verifying-key encoding (the 32-byte compressed Edwards point);
//! the runtime decodes it into a real verifying key during bundle signature
//! enforcement. Empty allowlist = TOFU (trust the key embedded in `bundle.sig`);
//! non-empty allowlist = pin (reject any bundle whose embedded key is not a
//! member).

/// Raw Ed25519 public-key bytes (the 32-byte compressed Edwards point encoding).
///
/// # Layout
/// `#[repr(C)]`, 32 bytes, align 1 — a bare byte array with no padding.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ed25519PublicKey {
    /// The 32 raw bytes of the Ed25519 verifying key.
    pub bytes: [u8; 32],
}

#[cfg(test)]
mod tests {
    use core::mem::{align_of, size_of};

    use crate::types::ed25519_public_key::Ed25519PublicKey;

    #[test]
    fn layout_ed25519_public_key() {
        assert_eq!(size_of::<Ed25519PublicKey>(), 32);
        assert_eq!(align_of::<Ed25519PublicKey>(), 1);
    }
}
