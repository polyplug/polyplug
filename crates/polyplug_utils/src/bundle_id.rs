use crate::fnv1a_64;

#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct BundleId(u64);

impl BundleId {
    /// Compute a bundle ID from its name using FNV-1a 64-bit hash.
    ///
    /// The bundle ID is a stable identifier for a plugin bundle.
    /// Same bundle name always produces the same ID.
    ///
    /// # Example
    ///
    /// ```
    /// use polyplug_utils::BundleId;
    ///
    /// let bundle_id = BundleId::new("my-bundle");
    /// assert_eq!(bundle_id.id(), 0xfe6226876e3a35b2_u64);
    /// ```
    pub fn new(name: &str) -> Self {
        Self(fnv1a_64(name.as_bytes()))
    }
}

impl BundleId {
    #[inline(always)]
    pub fn id(&self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use crate::BundleId;

    #[test]
    fn bundle_id_stability() {
        // Same input always yields same output
        assert_eq!(BundleId::new("my-bundle"), BundleId::new("my-bundle"));

        // Golden: FNV-1a of "my-bundle"
        assert_eq!(BundleId::new("my-bundle").id(), 0xfe6226876e3a35b2_u64);

        // Golden: FNV-1a of "polyplug-core"
        assert_eq!(BundleId::new("polyplug-core").id(), 0x6ef4aee714f5f991_u64);

        // Different bundle names produce different IDs
        assert_ne!(BundleId::new("bundle-a"), BundleId::new("bundle-b"));
    }
}
