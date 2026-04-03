use polyplug_abi::types::Version;
use polyplug_utils::GuestContractId;

/// A contract capability (either provided or required).
#[derive(Debug, Clone)]
pub struct ContractCapability {
    pub contract_name: String,
    pub contract_id: GuestContractId,
    pub version: Version,
}

impl ContractCapability {
    /// Construct from a contract name and version.
    pub fn new(name: String, version: Version) -> ContractCapability {
        let contract_id = GuestContractId::new(&name, version.major);
        ContractCapability {
            contract_name: name,
            contract_id,
            version,
        }
    }
}