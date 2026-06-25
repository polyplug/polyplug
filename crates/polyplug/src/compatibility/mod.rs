//! Compatibility module.

mod bundle_node;
pub mod capability_graph;
mod contract_capability;
mod dependency_edge;

pub use capability_graph::CapabilityGraph;

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use std::collections::HashMap;
    use std::path::PathBuf;

    use polyplug_abi::types::Version;
    use polyplug_utils::{BundleId, GuestContractId};

    use crate::compatibility::bundle_node::BundleNode;
    use crate::compatibility::capability_graph::CapabilityGraph;
    use crate::compatibility::contract_capability::ContractCapability;
    use crate::error::GraphError;
    use crate::loader::{ManifestData, RawManifestDependency};

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

    #[test]
    fn from_manifests_chain_order() {
        let cid_x = GuestContractId::new("contract.X", 1);
        let cid_y = GuestContractId::new("contract.Y", 1);

        let dep_b: RawManifestDependency = RawManifestDependency {
            kind: "contract".to_owned(),
            contract: "contract.X".to_owned(),
            min_version: "1.0".to_owned(),
            bundle: None,
            contract_id: cid_x,
            bundle_id: None,
        };

        let dep_c: RawManifestDependency = RawManifestDependency {
            kind: "contract".to_owned(),
            contract: "contract.Y".to_owned(),
            min_version: "1.0".to_owned(),
            bundle: None,
            contract_id: cid_y,
            bundle_id: None,
        };

        let manifest_a: ManifestData = ManifestData {
            loader: "native".to_owned(),
            name: "bundle_a".to_owned(),
            dependencies: Vec::new(),
            id: 0,
            version: String::new(),
            file: String::new(),
            provides: vec!["contract.X".to_owned()],
            function_count: HashMap::new(),
            needs_reinit_on_dep_reload: false,
            bundle_dependencies: Vec::new(),
            path: PathBuf::new(),
        };

        let manifest_b: ManifestData = ManifestData {
            loader: "native".to_owned(),
            name: "bundle_b".to_owned(),
            dependencies: vec![dep_b],
            id: 0,
            version: String::new(),
            file: String::new(),
            provides: vec!["contract.Y".to_owned()],
            function_count: HashMap::new(),
            needs_reinit_on_dep_reload: false,
            bundle_dependencies: Vec::new(),
            path: PathBuf::new(),
        };

        let manifest_c: ManifestData = ManifestData {
            loader: "native".to_owned(),
            name: "bundle_c".to_owned(),
            dependencies: vec![dep_c],
            id: 0,
            version: String::new(),
            file: String::new(),
            provides: Vec::new(),
            function_count: HashMap::new(),
            needs_reinit_on_dep_reload: false,
            bundle_dependencies: Vec::new(),
            path: PathBuf::new(),
        };

        let manifests: Vec<(PathBuf, ManifestData)> = vec![
            (PathBuf::from("bundle_a.so"), manifest_a),
            (PathBuf::from("bundle_b.so"), manifest_b),
            (PathBuf::from("bundle_c.so"), manifest_c),
        ];

        let graph: CapabilityGraph =
            CapabilityGraph::from_manifests(&manifests).expect("from_manifests should succeed");

        let order: Vec<String> = graph.topological_order().expect("topo order");

        let pos_a: usize = order
            .iter()
            .position(|n| n == "bundle_a")
            .expect("bundle_a in order");
        let pos_b: usize = order
            .iter()
            .position(|n| n == "bundle_b")
            .expect("bundle_b in order");
        let pos_c: usize = order
            .iter()
            .position(|n| n == "bundle_c")
            .expect("bundle_c in order");

        assert!(pos_a < pos_b, "bundle_a must load before bundle_b");
        assert!(pos_b < pos_c, "bundle_b must load before bundle_c");
    }

    #[test]
    fn from_manifests_bybundle_missing_fails() {
        let cid_x = GuestContractId::new("contract.X", 1);

        let dep_b: RawManifestDependency = RawManifestDependency {
            kind: "bundle".to_owned(),
            contract: "contract.X".to_owned(),
            min_version: "1.0".to_owned(),
            bundle: Some("missing_bundle".to_owned()),
            contract_id: cid_x,
            bundle_id: Some(BundleId::new("bundle_b")),
        };

        let manifest_b: ManifestData = ManifestData {
            loader: "native".to_owned(),
            name: "bundle_b".to_owned(),
            dependencies: vec![dep_b],
            id: 0,
            version: String::new(),
            file: String::new(),
            provides: Vec::new(),
            function_count: HashMap::new(),
            needs_reinit_on_dep_reload: false,
            bundle_dependencies: Vec::new(),
            path: PathBuf::new(),
        };

        let manifests: Vec<(PathBuf, ManifestData)> =
            vec![(PathBuf::from("bundle_b.so"), manifest_b)];

        let result: Result<CapabilityGraph, GraphError> =
            CapabilityGraph::from_manifests(&manifests);
        assert!(
            matches!(result, Err(GraphError::UnsatisfiedCapability { .. })),
            "expected UnsatisfiedCapability when ByBundle dep is missing"
        );
    }

    /// Build a `ManifestData` with one provides entry and (optionally) one ByContract
    /// dependency, used by the version-gating tests below.
    fn version_manifest(
        name: &str,
        version: &str,
        provides: Vec<&str>,
        dep: Option<RawManifestDependency>,
    ) -> ManifestData {
        ManifestData {
            loader: "native".to_owned(),
            name: name.to_owned(),
            dependencies: dep.into_iter().collect(),
            id: 0,
            version: version.to_owned(),
            file: String::new(),
            provides: provides.into_iter().map(str::to_owned).collect(),
            function_count: HashMap::new(),
            needs_reinit_on_dep_reload: false,
            bundle_dependencies: Vec::new(),
            path: PathBuf::new(),
        }
    }

    /// Item 6: the graph now derives a provider's `contract_id` from its REAL major
    /// version (parsed from the `provides` `name@version` / bundle version), not a
    /// hardcoded major 1. A requirement for a DIFFERENT major therefore has no
    /// structural provider and fails with UnsatisfiedCapability.
    ///
    /// (Minor/patch version POLICY is enforced separately by
    /// `validate_bundle_compatibility`, which is Compatibility-mode aware.)
    #[test]
    fn graph_rejects_major_version_mismatch() {
        // Provider offers svc.api@2.0 → contract_id(name, major=2).
        // Requirer needs svc.api at major 1 → contract_id(name, major=1): different id.
        let req_cid: GuestContractId = GuestContractId::new("svc.api", 1);
        let provider: ManifestData = version_manifest("provider", "2.0", vec!["svc.api@2.0"], None);
        let requirer: ManifestData = version_manifest(
            "requirer",
            "1.0",
            vec![],
            Some(RawManifestDependency {
                kind: "contract".to_owned(),
                contract: "svc.api".to_owned(),
                min_version: "1.0".to_owned(),
                bundle: None,
                contract_id: req_cid,
                bundle_id: None,
            }),
        );

        let manifests: Vec<(PathBuf, ManifestData)> = vec![
            (PathBuf::from("provider.so"), provider),
            (PathBuf::from("requirer.so"), requirer),
        ];
        let result: Result<CapabilityGraph, GraphError> =
            CapabilityGraph::from_manifests(&manifests);
        assert!(
            matches!(result, Err(GraphError::UnsatisfiedCapability { .. })),
            "a provider at major 2 must NOT satisfy a major-1 requirement, got ok={}",
            result.is_ok()
        );
    }

    /// Item 6: when the provider's major version matches the requirement's, the graph
    /// resolves the structural edge and builds successfully.
    #[test]
    fn graph_accepts_matching_major_version() {
        // Provider offers svc.api@2.3 → contract_id(name, major=2).
        // Requirer needs the same major (its contract_id uses major 2).
        let cid: GuestContractId = GuestContractId::new("svc.api", 2);
        let provider: ManifestData = version_manifest("provider", "2.3", vec!["svc.api@2.3"], None);
        let requirer: ManifestData = version_manifest(
            "requirer",
            "1.0",
            vec![],
            Some(RawManifestDependency {
                kind: "contract".to_owned(),
                contract: "svc.api".to_owned(),
                min_version: "2.0".to_owned(),
                bundle: None,
                contract_id: cid,
                bundle_id: None,
            }),
        );

        let manifests: Vec<(PathBuf, ManifestData)> = vec![
            (PathBuf::from("provider.so"), provider),
            (PathBuf::from("requirer.so"), requirer),
        ];
        let result: Result<CapabilityGraph, GraphError> =
            CapabilityGraph::from_manifests(&manifests);
        assert!(
            result.is_ok(),
            "a provider at major 2 must satisfy a major-2 requirement, got error: {:?}",
            result.err()
        );
    }
}
