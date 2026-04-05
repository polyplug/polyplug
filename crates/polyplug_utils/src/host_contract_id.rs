use crate::contract_id;

#[repr(transparent)]
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct HostContractId(u64);

impl HostContractId {
    /// Calculate host contract ID from name and major version.
    ///
    /// Uses a distinct prefix `"host_contract:"` to avoid collisions with plugin contract IDs.
    ///
    /// # Example
    ///
    /// ```
    /// use polyplug_utils::HostContractId;
    ///
    /// let contract_id = HostContractId::new("logger", 1);
    /// assert_ne!(contract_id.id(), 0);
    /// ```
    pub fn new(name: &str, major_version: u32) -> Self {
        Self(contract_id("host_contract:", name, major_version))
    }
}

impl HostContractId {
    #[inline(always)]
    pub fn id(&self) -> u64 {
        self.0
    }
}

impl From<u64> for HostContractId {
    /// Create a HostContractId from a raw u64 value.
    ///
    /// Use this when you have a pre-computed contract ID (e.g., from code generation).
    #[inline(always)]
    fn from(value: u64) -> Self {
        Self(value)
    }
}

#[cfg(test)]
mod tests {
    use crate::{HostContractId, fnv1a_64};


    #[test]
    fn contract_id_collision() {
        // Both must be deterministic
        assert_eq!(
            HostContractId::new("logger", 1),
            HostContractId::new("logger", 1)
        );

        // Different names produce different IDs within same category
        assert_ne!(
            HostContractId::new("logger", 1),
            HostContractId::new("metrics", 1)
        );

        // Different major versions produce different IDs within same category
        assert_ne!(
            HostContractId::new("logger", 1),
            HostContractId::new("logger", 2)
        );
    }

    #[test]
    fn host_contract_id_format() {
        // host_contract_id("logger", 1) should equal fnv1a_64(b"host_contract:logger@1")
        assert_eq!(
            HostContractId::new("logger", 1).id(),
            fnv1a_64(b"host_contract:logger@1")
        );
    }
}
