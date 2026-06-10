use core::str::FromStr;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;

use petgraph::algo;
use petgraph::graph::DiGraph;
use petgraph::graph::NodeIndex;
use polyplug_abi::types::Version;
use polyplug_utils::GuestContractId;

use crate::compatibility::contract_capability::ContractCapability;
use crate::compatibility::{bundle_node::BundleNode, dependency_edge::DependencyEdge};
use crate::error::GraphError;
use crate::loader::ManifestData;
use crate::loader::ManifestDependency;

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

impl Default for CapabilityGraph {
    fn default() -> CapabilityGraph {
        CapabilityGraph::new()
    }
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
    //  (matched by `contract_id`, which encodes name + major version) and add a
    //  directed edge: requirer → provider.
    //
    //  Version POLICY (minor/patch minimums, Strict/Relaxed/Yolo handling) is NOT
    //  enforced here — it is owned by `validate_bundle_compatibility`, which has the
    //  Compatibility mode in context and does the real `is_compatible_with` check.
    //  This layer only resolves the structural edge: matching on `contract_id`
    //  (name + major) is the correct major-version gate, since a different required
    //  major yields a different contract_id and therefore "no provider".
    //
    //  Returns Err(UnsatisfiedCapability) if any requirement has no provider.
    pub fn build_edges(&mut self) -> Result<(), GraphError> {
        // Collect provider map: contract_id → (provider_name, NodeIndex)
        let mut provider_map: HashMap<GuestContractId, (String, NodeIndex)> = HashMap::new();
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
                            _contract_name: req.contract_name.clone(),
                            _contract_id: req.contract_id,
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

    /// Build a `CapabilityGraph` from a set of discovered manifests.
    ///
    /// Validates all ByBundle dependencies against the discovered bundle set.
    /// Returns `Err(GraphError::UnsatisfiedCapability)` if any ByBundle dep is missing
    /// or if a bundle does not provide the required contract.
    /// The caller should then call `graph.topological_order()` for load ordering.
    pub fn from_manifests(
        manifests: &[(PathBuf, ManifestData)],
    ) -> Result<CapabilityGraph, GraphError> {
        let mut graph: CapabilityGraph = CapabilityGraph::new();

        // Build set of all discovered bundle names for ByBundle validation
        let discovered_bundles: HashSet<String> = manifests
            .iter()
            .map(|(_path, manifest): &(PathBuf, ManifestData)| manifest.name.clone())
            .collect::<HashSet<String>>();

        // Build provides_map: bundle_name -> Vec<String> (contract names provided)
        let mut provides_map: HashMap<String, Vec<String>> = HashMap::new();
        for (_path, manifest) in manifests {
            provides_map.insert(manifest.name.clone(), manifest.provides.clone());
        }

        for (_path, manifest) in manifests {
            // The bundle's own version is the fallback provided-version when a
            // provides entry does not pin one inline (via `name@version`).
            //
            // When neither an inline version nor a parseable bundle version is
            // available, fall back to 1.0.0 (the documented default major). This
            // keeps the derived contract_id (name + major) aligned with how requires
            // resolve their contract_id — a versionless provider is treated as v1,
            // matching `GuestContractId::new(name, 1)`.
            let default_version: Version = Version {
                major: 1,
                minor: 0,
                patch: 0,
            };
            let bundle_version: Version =
                Version::from_str(&manifest.version).unwrap_or(default_version);

            // Build provides capabilities with their real versions.
            //
            // A provides entry may be `contract` or `contract@version`. The inline
            // `@version` (when present) is the authoritative provided version; otherwise
            // fall back to the bundle version. `ContractCapability::new` derives the
            // contract_id from the bare name and the version's major component, matching
            // how requires resolve their contract_id.
            let provides_caps: Vec<ContractCapability> = manifest
                .provides
                .iter()
                .map(|spec: &String| {
                    let (bare_name, provided_version): (&str, Version) = match spec.split_once('@')
                    {
                        Some((name, version_str)) => (
                            name,
                            Version::from_str(version_str).unwrap_or(bundle_version),
                        ),
                        None => (spec.as_str(), bundle_version),
                    };
                    ContractCapability::new(bare_name.to_owned(), provided_version)
                })
                .collect::<Vec<ContractCapability>>();

            // Build requires capabilities from resolved dependencies
            let resolved: Vec<ManifestDependency> = manifest.resolved_dependencies();
            let mut requires_caps: Vec<ContractCapability> = Vec::new();
            for dep in &resolved {
                match dep {
                    ManifestDependency::ByContract {
                        contract,
                        contract_id,
                        min_version,
                    } => {
                        // Real required version: parse the dependency's declared
                        // min_version. An empty/unparseable spec means "any version"
                        // (0.0.0), preserving prior permissive behaviour for those.
                        let required_version: Version =
                            Version::from_str(min_version).unwrap_or(Version {
                                major: 0,
                                minor: 0,
                                patch: 0,
                            });
                        let cap: ContractCapability = ContractCapability {
                            contract_name: contract.clone(),
                            contract_id: *contract_id,
                            version: required_version,
                        };
                        requires_caps.push(cap);
                    }
                    ManifestDependency::ByBundle {
                        bundle,
                        bundle_id: _,
                        contract,
                        contract_id,
                        min_version,
                    } => {
                        // Validate bundle is in discovered set
                        if !discovered_bundles.contains(bundle) {
                            return Err(GraphError::UnsatisfiedCapability {
                                requester: manifest.name.clone(),
                                capability: format!("{} (from bundle {})", contract, bundle),
                            });
                        }
                        // Validate bundle provides the required contract. A provides
                        // entry may be `name` or `name@version`; the dependency names
                        // the bare contract. Compare bare-vs-bare by stripping any
                        // `@version` suffix from each provides entry so a versioned
                        // provides still satisfies a bare-named ByBundle dependency.
                        let provides: bool = provides_map
                            .get(bundle)
                            .map(|p: &Vec<String>| {
                                p.iter().any(|spec: &String| {
                                    let bare: &str = match spec.split_once('@') {
                                        Some((name, _)) => name,
                                        None => spec.as_str(),
                                    };
                                    bare == contract.as_str()
                                })
                            })
                            .unwrap_or(false);
                        if !provides {
                            return Err(GraphError::UnsatisfiedCapability {
                                requester: manifest.name.clone(),
                                capability: format!(
                                    "{} not provided by bundle {}",
                                    contract, bundle
                                ),
                            });
                        }
                        // Real required version, same parsing rule as ByContract.
                        let required_version: Version =
                            Version::from_str(min_version).unwrap_or(Version {
                                major: 0,
                                minor: 0,
                                patch: 0,
                            });
                        let cap: ContractCapability = ContractCapability {
                            contract_name: contract.clone(),
                            contract_id: *contract_id,
                            version: required_version,
                        };
                        requires_caps.push(cap);
                    }
                }
            }

            graph.add_bundle(BundleNode {
                name: manifest.name.clone(),
                provides: provides_caps,
                requires: requires_caps,
            });
        }

        graph.build_edges()?;
        graph.detect_cycles()?;
        Ok(graph)
    }
}
