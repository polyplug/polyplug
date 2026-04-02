use polyplug_utils::PluginContractId;

/// A directed edge in the capability graph.
#[derive(Debug, Clone)]
pub struct DependencyEdge {
    /// Contract that creates the dependency (e.g. "image.decode@1.0").
    pub contract_name: String,
    pub contract_id: PluginContractId,
}
