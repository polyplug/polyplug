//! SignaturePolicy — bundle signature enforcement modes.

/// How strictly bundle signature verification is enforced at load time.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SignaturePolicy {
    /// Signature verification is skipped entirely. Unsigned bundles load normally.
    #[default]
    Off = 0,
    /// If a bundle is unsigned or the signature is invalid, emit a warning and
    /// continue loading. Bundles without a valid signature are NOT rejected.
    WarnOnly = 1,
    /// Bundles MUST carry a valid `bundle.sig`. Missing or invalid signatures
    /// cause the load to fail with a `LoaderError`.
    Required = 2,
}

#[cfg(test)]
mod tests {
    use super::SignaturePolicy;

    #[test]
    fn signature_policy_default_is_off() {
        assert_eq!(SignaturePolicy::default(), SignaturePolicy::Off);
    }

    #[test]
    fn signature_policy_repr_u32() {
        assert_eq!(SignaturePolicy::Off as u32, 0);
        assert_eq!(SignaturePolicy::WarnOnly as u32, 1);
        assert_eq!(SignaturePolicy::Required as u32, 2);
    }
}
