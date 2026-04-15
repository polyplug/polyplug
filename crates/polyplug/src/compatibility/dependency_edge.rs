use polyplug_utils::GuestContractId;

/// A directed edge in the capability graph.
#[derive(Debug, Clone)]
pub struct DependencyEdge {
    /// Contract that creates the dependency (e.g. "image.decode@1.0").
    pub _contract_name: String,
    pub _contract_id: GuestContractId,
}
