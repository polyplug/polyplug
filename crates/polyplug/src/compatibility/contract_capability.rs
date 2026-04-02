use polyplug_abi::types::Version;
use polyplug_utils::PluginContractId;

/// A contract capability (either provided or required).
#[derive(Debug, Clone)]
pub struct ContractCapability {
    pub contract_name: String,
    pub contract_id: PluginContractId,
    pub version: Version,
}

impl ContractCapability {
    /// Construct from a contract name and version.
    pub fn new(name: String, version: Version) -> ContractCapability {
        ContractCapability {
            contract_name: name,
            contract_id: PluginContractId::new(&name, version.major),
            version,
        }
    }
}