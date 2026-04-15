use crate::contract_id;

#[repr(transparent)]
#[derive(
    Debug,
    Clone,
    Copy,
    Hash,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    serde::Deserialize,
    serde::Serialize,
    Default,
)]
pub struct GuestContractId(u64);

impl GuestContractId {
    /// Calculate guest contract ID from name and major version.
    ///
    /// Uses a distinct prefix `"guest_contract:"` to avoid collisions with host contract IDs.
    ///
    /// # Example
    ///
    /// ```
    /// use polyplug_utils::GuestContractId;
    ///
    /// let contract_id = GuestContractId::new("logger", 1);
    /// assert_ne!(contract_id.id(), 0);
    /// ```
    pub fn new(name: &str, major_version: u32) -> Self {
        Self(contract_id("guest_contract:", name, major_version))
    }

    /// Create a GuestContractId from a raw u64.
    ///
    /// This is used when receiving contract IDs from the ABI boundary.
    pub const fn from_u64(id: u64) -> Self {
        Self(id)
    }
}

impl GuestContractId {
    #[inline(always)]
    pub fn id(&self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use crate::{GuestContractId, fnv1a_64};

    #[test]
    fn guest_contract_id_format() {
        // guest_contract_id("logger", 1) should equal fnv1a_64(b"guest_contract:logger@1")
        assert_eq!(
            GuestContractId::new("logger", 1).id(),
            fnv1a_64(b"guest_contract:logger@1")
        );
    }

    #[test]
    fn contract_id_collision() {
        // Both must be deterministic
        assert_eq!(
            GuestContractId::new("logger", 1),
            GuestContractId::new("logger", 1)
        );

        // Different names produce different IDs within same category
        assert_ne!(
            GuestContractId::new("logger", 1),
            GuestContractId::new("metrics", 1)
        );

        // Different major versions produce different IDs within same category
        assert_ne!(
            GuestContractId::new("logger", 1),
            GuestContractId::new("logger", 2)
        );
    }
}
