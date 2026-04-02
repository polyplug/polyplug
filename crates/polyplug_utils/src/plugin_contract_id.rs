use crate::contract_id;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PluginContractId(u64);

impl PluginContractId {
    /// Calculate plugin contract ID from name and major version.
    ///
    /// Uses a distinct prefix `"plugin_contract:"` to avoid collisions with host contract IDs.
    ///
    /// # Example
    ///
    /// ```
    /// use polyplug_utils::PluginContractId;
    ///
    /// let contract_id = PluginContractId::new("logger", 1);
    /// assert_ne!(contract_id.id(), 0);
    /// ```
    pub fn new(name: &str, major_version: u32) -> Self {
        Self(contract_id("plugin_contract:", name, major_version))
    }
}

impl PluginContractId {
    #[inline(always)]
    pub fn id(&self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use crate::{PluginContractId, fnv1a_64};

    #[test]
    fn plugin_contract_id_format() {
        // plugin_contract_id("logger", 1) should equal fnv1a_64(b"plugin_contract:logger@1")
        assert_eq!(
            PluginContractId::new("logger", 1).id(),
            fnv1a_64(b"plugin_contract:logger@1")
        );
    }

    #[test]
    fn contract_id_collision() {
        // Both must be deterministic
        assert_eq!(
            PluginContractId::new("logger", 1),
            PluginContractId::new("logger", 1)
        );

        // Different names produce different IDs within same category
        assert_ne!(
            PluginContractId::new("logger", 1),
            PluginContractId::new("metrics", 1)
        );

        // Different major versions produce different IDs within same category
        assert_ne!(
            PluginContractId::new("logger", 1),
            PluginContractId::new("logger", 2)
        );
    }
}
