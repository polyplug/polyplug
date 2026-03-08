//! Graph — capability dependency graph, topological sort, and cycle detection.
//!
//! Uses `petgraph::DiGraph` where nodes are bundles and edges represent
//! dependency relationships ("bundle A requires something provided by bundle B").

use std::collections::HashMap;

use petgraph::algo;
use petgraph::graph::DiGraph;
use petgraph::graph::NodeIndex;

use crate::abi::contract_id as compute_contract_id;
use crate::error::GraphError;

/// Version with major.minor.patch components.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

/// A contract capability (either provided or required).
#[derive(Debug, Clone)]
pub struct ContractCapability {
    pub contract_name: String,
    /// Precomputed FNV-1a hash of "name@major".
    pub contract_id: u64,
    pub version: Version,
}

impl ContractCapability {
    /// Construct from a contract name and version.
    pub fn new(name: String, version: Version) -> ContractCapability {
        let id: u64 = compute_contract_id(&name, version.major);
        ContractCapability {
            contract_name: name,
            contract_id: id,
            version,
        }
    }
}

/// A directed edge in the capability graph.
#[derive(Debug, Clone)]
pub struct DependencyEdge {
    /// Contract that creates the dependency (e.g. "image.decode@1.0").
    pub contract_name: String,
    pub contract_id: u64,
}

/// A bundle node in the capability graph.
#[derive(Debug)]
pub struct BundleNode {
    pub name: String,
    /// Contracts this bundle provides.
    pub provides: Vec<ContractCapability>,
    /// Contracts this bundle requires.
    pub requires: Vec<ContractCapability>,
}

/// The capability dependency graph for all bundles.
//
//  Nodes = bundles, edges = dependency relationships.
//  Only used during the initialization phase (single-threaded).
//  Discarded after init — not stored in the runtime.
pub struct CapabilityGraph {
    graph: DiGraph<BundleNode, DependencyEdge>,
    /// Maps bundle name → NodeIndex.
    node_map: HashMap<String, NodeIndex>,
}

impl CapabilityGraph {
    /// Create an empty capability graph.
    pub fn new() -> CapabilityGraph {
        CapabilityGraph {
            graph: DiGraph::new(),
            node_map: HashMap::new(),
        }
    }

    /// Add a bundle to the graph.
    pub fn add_bundle(&mut self, node: BundleNode) {
        let name: String = node.name.clone();
        let idx: NodeIndex = self.graph.add_node(node);
        self.node_map.insert(name, idx);
    }

    /// Build dependency edges between bundles.
    //
    //  For each bundle's `requires`, find which bundle `provides` that contract
    //  and add a directed edge: requirer → provider.
    //
    //  Returns Err(UnsatisfiedCapability) if any requirement has no provider.
    pub fn build_edges(&mut self) -> Result<(), GraphError> {
        // Collect provider map: contract_id → (provider_name, NodeIndex)
        let mut provider_map: HashMap<u64, (String, NodeIndex)> = HashMap::new();
        for idx in self.graph.node_indices() {
            let bundle_name: String = self.graph[idx].name.clone();
            for cap in &self.graph[idx].provides {
                provider_map.insert(cap.contract_id, (bundle_name.clone(), idx));
            }
        }

        // Build edges: for each require, add edge requirer → provider
        let mut edges_to_add: Vec<(NodeIndex, NodeIndex, DependencyEdge)> = Vec::new();
        for requirer_idx in self.graph.node_indices() {
            let requirer_name: String = self.graph[requirer_idx].name.clone();
            let requires: Vec<ContractCapability> = self.graph[requirer_idx].requires.clone();
            for req in requires {
                match provider_map.get(&req.contract_id) {
                    Some((_, provider_idx)) => {
                        let edge: DependencyEdge = DependencyEdge {
                            contract_name: req.contract_name.clone(),
                            contract_id: req.contract_id,
                        };
                        edges_to_add.push((requirer_idx, *provider_idx, edge));
                    }
                    None => {
                        return Err(GraphError::UnsatisfiedCapability {
                            requester: requirer_name,
                            capability: req.contract_name,
                        });
                    }
                }
            }
        }

        for (from, to, edge) in edges_to_add {
            self.graph.add_edge(from, to, edge);
        }
        Ok(())
    }

    /// Detect cycles using Tarjan's SCC algorithm.
    //
    //  Any SCC with size > 1 is a cycle.
    //  Reports ALL participants in the cycle.
    pub fn detect_cycles(&self) -> Result<(), GraphError> {
        let sccs: Vec<Vec<NodeIndex>> = algo::tarjan_scc(&self.graph);
        for scc in sccs {
            if scc.len() > 1 {
                let participants: Vec<String> = scc
                    .iter()
                    .map(|&idx| self.graph[idx].name.clone())
                    .collect();
                return Err(GraphError::DependencyCycle { participants });
            }
        }
        Ok(())
    }

    /// Produce a topological initialization order.
    //
    //  Bundles with no dependencies load first.
    //  Returns Err(DependencyCycle) if the graph has cycles (should be detected first).
    pub fn topological_order(&self) -> Result<Vec<String>, GraphError> {
        match algo::toposort(&self.graph, None) {
            Ok(order) => {
                let names: Vec<String> = order
                    .iter()
                    .rev() // toposort returns reverse dependency order
                    .map(|&idx| self.graph[idx].name.clone())
                    .collect();
                Ok(names)
            }
            Err(cycle) => {
                let participant: String = self.graph[cycle.node_id()].name.clone();
                Err(GraphError::DependencyCycle {
                    participants: vec![participant],
                })
            }
        }
    }
}

impl Default for CapabilityGraph {
    fn default() -> CapabilityGraph {
        CapabilityGraph::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_capability(name: &str, major: u32, minor: u32) -> ContractCapability {
        ContractCapability::new(
            name.to_owned(),
            Version {
                major,
                minor,
                patch: 0,
            },
        )
    }

    #[test]
    fn topological_sort_no_deps() {
        let mut graph: CapabilityGraph = CapabilityGraph::new();
        graph.add_bundle(BundleNode {
            name: "bundle_a".to_owned(),
            provides: vec![make_capability("image.decode", 1, 0)],
            requires: vec![],
        });
        graph.add_bundle(BundleNode {
            name: "bundle_b".to_owned(),
            provides: vec![make_capability("audio.decode", 1, 0)],
            requires: vec![],
        });
        graph.build_edges().expect("no edges needed");
        graph.detect_cycles().expect("no cycles");
        let order: Vec<String> = graph.topological_order().expect("order");
        assert_eq!(order.len(), 2);
    }

    #[test]
    fn topological_sort_with_dependency() {
        let mut graph: CapabilityGraph = CapabilityGraph::new();
        let image_cap: ContractCapability = make_capability("image.decode", 1, 0);
        graph.add_bundle(BundleNode {
            name: "decoder".to_owned(),
            provides: vec![image_cap.clone()],
            requires: vec![],
        });
        graph.add_bundle(BundleNode {
            name: "processor".to_owned(),
            provides: vec![],
            requires: vec![image_cap],
        });
        graph.build_edges().expect("edges built");
        graph.detect_cycles().expect("no cycles");
        let order: Vec<String> = graph.topological_order().expect("order");
        // decoder must come before processor
        let decoder_pos: usize = order.iter().position(|n| n == "decoder").expect("decoder");
        let processor_pos: usize = order
            .iter()
            .position(|n| n == "processor")
            .expect("processor");
        assert!(
            decoder_pos < processor_pos,
            "decoder must load before processor"
        );
    }

    #[test]
    fn cycle_detection() {
        let mut graph: CapabilityGraph = CapabilityGraph::new();
        let cap_a: ContractCapability = make_capability("contract.a", 1, 0);
        let cap_b: ContractCapability = make_capability("contract.b", 1, 0);
        graph.add_bundle(BundleNode {
            name: "bundle_a".to_owned(),
            provides: vec![cap_a.clone()],
            requires: vec![cap_b.clone()],
        });
        graph.add_bundle(BundleNode {
            name: "bundle_b".to_owned(),
            provides: vec![cap_b],
            requires: vec![cap_a],
        });
        graph.build_edges().expect("edges built");
        let result: Result<(), GraphError> = graph.detect_cycles();
        assert!(
            matches!(result, Err(GraphError::DependencyCycle { .. })),
            "expected DependencyCycle error"
        );
    }

    #[test]
    fn unsatisfied_capability_error() {
        let mut graph: CapabilityGraph = CapabilityGraph::new();
        let missing_cap: ContractCapability = make_capability("missing.contract", 1, 0);
        graph.add_bundle(BundleNode {
            name: "bundle_a".to_owned(),
            provides: vec![],
            requires: vec![missing_cap],
        });
        let result: Result<(), GraphError> = graph.build_edges();
        assert!(
            matches!(result, Err(GraphError::UnsatisfiedCapability { .. })),
            "expected UnsatisfiedCapability error"
        );
    }
}
