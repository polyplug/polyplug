use crate::compatibility::contract_capability::ContractCapability;

/// A bundle node in the capability graph.
#[derive(Debug)]
pub struct BundleNode {
    pub name: String,
    /// Contracts this bundle provides.
    pub provides: Vec<ContractCapability>,
    /// Contracts this bundle requires.
    pub requires: Vec<ContractCapability>,
}
