//! Kubernetes manifest and Kustomize parsing for the infrastructure pass.

use std::path::Path;

use cc_model::infra::{InfraEdge, InfraEdgeKind, InfraKind, InfraNode};
use cc_model::StableId;

use crate::infra_pass::{parse_yaml_kv, strip_quotes};

/// Parse K8s manifest into InfraNodes + InfraEdges.
///
/// Supports multi-document YAML (separated by `---`).
/// Extracts cross-reference edges between resources based on selectors,
/// configMapRef/secretRef, and volume mounts.
pub fn parse_k8s_manifest(rel_path: &str, content: &str) -> (Vec<InfraNode>, Vec<InfraEdge>) {
    let mut nodes = Vec::new();
    let mut edges = Vec::new();

    // Split on `---` to handle multi-document YAML
    let documents: Vec<&str> = content.split("\n---").collect();

    // Per-document metadata for cross-referencing
    struct K8sDoc {
        node_id: String,
        kind: String,
        name: String,
        labels: Vec<(String, String)>,
        config_map_refs: Vec<String>,
        secret_refs: Vec<String>,
        pvc_refs: Vec<String>,
        selector_labels: Vec<(String, String)>,
    }
    let mut docs: Vec<K8sDoc> = Vec::new();

    for doc in &documents {
        let mut kind_str = None;
        let mut name_str = None;
        let mut namespace_str = None;
        let mut in_metadata = false;
        let mut in_labels = false;
        let mut labels: Vec<(String, String)> = Vec::new();
        let mut metadata_indent = 0usize;
        let mut labels_indent = 0usize;

        // Cross-reference tracking
        let mut selector_labels: Vec<(String, String)> = Vec::new();
        let mut in_selector = false;
        let mut in_match_labels = false;
        let mut selector_indent = 0usize;
        let mut match_labels_indent = 0usize;
        let mut config_map_refs: Vec<String> = Vec::new();
        let mut secret_refs: Vec<String> = Vec::new();
        let mut pvc_refs: Vec<String> = Vec::new();

        for (line_num, line) in doc.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let indent = line.len() - line.trim_start().len();

            // Top-level kind (indent == 0)
            if indent == 0 {
                if let Some(k) = trimmed.strip_prefix("kind:") {
                    kind_str = Some((k.trim().to_string(), line_num + 1));
                }
            }

            // metadata block
            if trimmed == "metadata:" && indent == 0 {
                in_metadata = true;
                metadata_indent = indent;
                in_labels = false;
                continue;
            }
            if in_metadata
                && indent <= metadata_indent
                && !trimmed.is_empty()
                && trimmed != "metadata:"
            {
                in_metadata = false;
                in_labels = false;
            }
            if in_metadata {
                if trimmed == "labels:" {
                    in_labels = true;
                    labels_indent = indent;
                    continue;
                }
                if in_labels {
                    if indent <= labels_indent && !trimmed.is_empty() {
                        in_labels = false;
                    } else if let Some((k, v)) = parse_yaml_kv(trimmed) {
                        labels.push((k, strip_quotes(&v)));
                    }
                }
                if let Some(n) = trimmed.strip_prefix("name:") {
                    name_str = Some(n.trim().trim_matches('"').trim_matches('\'').to_string());
                }
                if let Some(ns) = trimmed.strip_prefix("namespace:") {
                    namespace_str =
                        Some(ns.trim().trim_matches('"').trim_matches('\'').to_string());
                }
            }

            // selector / matchLabels block (for Service -> Deployment linking)
            if trimmed == "selector:" || trimmed.starts_with("selector:") {
                in_selector = true;
                selector_indent = indent;
                in_match_labels = false;
                continue;
            }
            if in_selector && indent <= selector_indent && !trimmed.is_empty() {
                in_selector = false;
                in_match_labels = false;
            }
            if in_selector {
                if trimmed == "matchLabels:" {
                    in_match_labels = true;
                    match_labels_indent = indent;
                    continue;
                }
                if in_match_labels {
                    if indent <= match_labels_indent && !trimmed.is_empty() {
                        in_match_labels = false;
                    } else if let Some((k, v)) = parse_yaml_kv(trimmed) {
                        selector_labels.push((k, strip_quotes(&v)));
                    }
                }
                // Service selector without matchLabels (flat key-value pairs)
                if !in_match_labels && indent > selector_indent {
                    if let Some((k, v)) = parse_yaml_kv(trimmed) {
                        if k != "matchLabels" && k != "matchExpressions" {
                            selector_labels.push((k, strip_quotes(&v)));
                        }
                    }
                }
            }

            // configMapRef / secretRef references
            if let Some(rest) = trimmed.strip_prefix("configMapRef:") {
                let n = rest.trim();
                if !n.is_empty() {
                    config_map_refs.push(strip_quotes(n));
                }
            }
            if trimmed.starts_with("configMap:") {
                if let Some(rest) = trimmed.strip_prefix("configMap:") {
                    let r = rest.trim();
                    if !r.is_empty() && !r.starts_with('{') {
                        config_map_refs.push(strip_quotes(r));
                    }
                }
            }
            if let Some(rest) = trimmed.strip_prefix("secretRef:") {
                let r = rest.trim();
                if !r.is_empty() {
                    secret_refs.push(strip_quotes(r));
                }
            }
            if trimmed.starts_with("secret:") {
                if let Some(rest) = trimmed.strip_prefix("secret:") {
                    let r = rest.trim();
                    if !r.is_empty() && !r.starts_with('{') {
                        secret_refs.push(strip_quotes(r));
                    }
                }
            }
            if let Some(rest) = trimmed.strip_prefix("claimName:") {
                pvc_refs.push(strip_quotes(rest.trim()));
            }
        }

        if let (Some((kind, line)), Some(name)) = (kind_str, name_str) {
            let infra_kind = match kind.as_str() {
                "Deployment" => Some(InfraKind::K8sDeployment),
                "Service" => Some(InfraKind::K8sService),
                "Ingress" | "IngressRoute" => Some(InfraKind::K8sIngress),
                "ConfigMap" => Some(InfraKind::K8sConfigMap),
                "Secret" => Some(InfraKind::K8sSecret),
                "StatefulSet" => Some(InfraKind::K8sStatefulSet),
                "DaemonSet" => Some(InfraKind::K8sDaemonSet),
                "Job" => Some(InfraKind::K8sJob),
                "CronJob" => Some(InfraKind::K8sCronJob),
                "PersistentVolumeClaim" => Some(InfraKind::K8sPvc),
                "ServiceAccount" => Some(InfraKind::K8sServiceAccount),
                "Namespace" => Some(InfraKind::K8sNamespace),
                _ => None,
            };

            if let Some(ik) = infra_kind {
                let node_id = StableId::edge_id("infra_k8s", rel_path, line as u32, 0);
                nodes.push(InfraNode {
                    node_id: node_id.clone(),
                    file_path: rel_path.to_string(),
                    kind: ik,
                    name: name.clone(),
                    namespace: namespace_str,
                    line: line as u32,
                    end_line: None,
                    properties: serde_json::json!({"k8s_kind": kind}),
                    bound_symbol_uid: None,
                    binding_confidence: None,
                });

                docs.push(K8sDoc {
                    node_id,
                    kind: kind.clone(),
                    name,
                    labels,
                    config_map_refs,
                    secret_refs,
                    pvc_refs,
                    selector_labels,
                });
            }
        }
    }

    // Phase 2: Build cross-reference edges between documents
    for doc in &docs {
        // Service -> Deployment/StatefulSet by selector label matching
        if doc.kind == "Service" && !doc.selector_labels.is_empty() {
            for target in &docs {
                if !matches!(
                    target.kind.as_str(),
                    "Deployment" | "StatefulSet" | "DaemonSet"
                ) {
                    continue;
                }
                let all_match = doc
                    .selector_labels
                    .iter()
                    .all(|(sk, sv)| target.labels.iter().any(|(tk, tv)| tk == sk && tv == sv));
                if all_match {
                    let edge_id = StableId::edge_id("infra_xref", rel_path, 0, edges.len() as u32);
                    edges.push(InfraEdge {
                        edge_id,
                        source_node_id: doc.node_id.clone(),
                        target_node_id: target.node_id.clone(),
                        kind: InfraEdgeKind::RoutesTo,
                        confidence: 0.85,
                        properties: serde_json::json!({"via": "selector_match"}),
                    });
                }
            }
        }

        // Workload -> ConfigMap / Secret / PVC by ref
        if matches!(
            doc.kind.as_str(),
            "Deployment" | "StatefulSet" | "DaemonSet" | "Job" | "CronJob"
        ) {
            for cm_name in &doc.config_map_refs {
                if let Some(target) = docs
                    .iter()
                    .find(|d| d.kind == "ConfigMap" && d.name == *cm_name)
                {
                    let edge_id = StableId::edge_id("infra_xref", rel_path, 0, edges.len() as u32);
                    edges.push(InfraEdge {
                        edge_id,
                        source_node_id: doc.node_id.clone(),
                        target_node_id: target.node_id.clone(),
                        kind: InfraEdgeKind::DependsOn,
                        confidence: 0.9,
                        properties: serde_json::json!({"via": "configMapRef"}),
                    });
                }
            }
            for sec_name in &doc.secret_refs {
                if let Some(target) = docs
                    .iter()
                    .find(|d| d.kind == "Secret" && d.name == *sec_name)
                {
                    let edge_id = StableId::edge_id("infra_xref", rel_path, 0, edges.len() as u32);
                    edges.push(InfraEdge {
                        edge_id,
                        source_node_id: doc.node_id.clone(),
                        target_node_id: target.node_id.clone(),
                        kind: InfraEdgeKind::DependsOn,
                        confidence: 0.9,
                        properties: serde_json::json!({"via": "secretRef"}),
                    });
                }
            }
            for pvc_name in &doc.pvc_refs {
                if let Some(target) = docs
                    .iter()
                    .find(|d| d.kind == "PersistentVolumeClaim" && d.name == *pvc_name)
                {
                    let edge_id = StableId::edge_id("infra_xref", rel_path, 0, edges.len() as u32);
                    edges.push(InfraEdge {
                        edge_id,
                        source_node_id: doc.node_id.clone(),
                        target_node_id: target.node_id.clone(),
                        kind: InfraEdgeKind::DependsOn,
                        confidence: 0.9,
                        properties: serde_json::json!({"via": "persistentVolumeClaim"}),
                    });
                }
            }
        }
    }

    (nodes, edges)
}

/// Parse a kustomization.yaml into InfraNodes + InfraEdges.
///
/// Creates a `KustomizeOverlay` node for the directory and `DependsOn` edges
/// to referenced resources, bases, patches, and components.
pub fn parse_kustomize(rel_path: &str, content: &str) -> (Vec<InfraNode>, Vec<InfraEdge>) {
    let mut nodes = Vec::new();
    let mut edges = Vec::new();

    // Derive overlay name from the parent directory
    let overlay_name = Path::new(rel_path)
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("root");

    let node_id = StableId::edge_id("infra_kustomize", rel_path, 1, 0);
    nodes.push(InfraNode {
        node_id: node_id.clone(),
        file_path: rel_path.to_string(),
        kind: InfraKind::KustomizeOverlay,
        name: overlay_name.to_string(),
        namespace: None,
        line: 1,
        end_line: None,
        properties: serde_json::json!({}),
        bound_symbol_uid: None,
        binding_confidence: None,
    });

    // Keys whose list items are referenced paths
    let import_keys: &[&str] = &[
        "resources:",
        "bases:",
        "patches:",
        "patchesStrategicMerge:",
        "patchesJson6902:",
        "components:",
    ];

    let mut in_import_section = false;
    let mut section_indent = 0usize;

    for (line_num, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let indent = line.len() - line.trim_start().len();

        // Check if this line starts an import section
        let is_import_key = import_keys
            .iter()
            .any(|k| trimmed == *k || trimmed.starts_with(k));

        if is_import_key {
            in_import_section = true;
            section_indent = indent;
            continue;
        }

        // A new top-level key ends the current section
        if in_import_section && indent <= section_indent && !trimmed.starts_with('-') {
            in_import_section = false;
        }

        if in_import_section && trimmed.starts_with("- ") {
            let ref_path = trimmed.strip_prefix("- ").unwrap_or("").trim();
            // Skip complex patch entries (those with sub-keys like `path:`)
            if ref_path.is_empty() || ref_path.contains(':') {
                continue;
            }
            let ref_clean = strip_quotes(ref_path);
            if ref_clean.is_empty() {
                continue;
            }

            // Resolve relative path against kustomization file's directory
            let base_dir = Path::new(rel_path).parent().unwrap_or(Path::new(""));
            let resolved = base_dir.join(&ref_clean).to_string_lossy().to_string();

            let edge_id =
                StableId::edge_id("infra_kustomize_dep", rel_path, (line_num + 1) as u32, 0);
            edges.push(InfraEdge {
                edge_id,
                source_node_id: node_id.clone(),
                target_node_id: format!("kustomize_ref:{}", resolved),
                kind: InfraEdgeKind::DependsOn,
                confidence: 0.9,
                properties: serde_json::json!({
                    "raw_ref": ref_clean,
                    "resolved_path": resolved,
                }),
            });
        }
    }

    (nodes, edges)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_k8s_multi_resource_extraction() {
        let yaml = "\
apiVersion: apps/v1
kind: Deployment
metadata:
  name: web-app
  labels:
    app: web
spec:
  selector:
    matchLabels:
      app: web
  template:
    spec:
      containers:
      - name: web
        image: nginx:1.25
      volumes:
      - name: config-vol
        configMap: app-config
      - name: secret-vol
        secret: db-credentials
      - name: data-vol
        persistentVolumeClaim:
          claimName: app-data
---
apiVersion: v1
kind: Service
metadata:
  name: web-svc
spec:
  selector:
    app: web
  ports:
  - port: 80
---
apiVersion: v1
kind: ConfigMap
metadata:
  name: app-config
  labels:
    app: web
data:
  key: value
---
apiVersion: v1
kind: Secret
metadata:
  name: db-credentials
type: Opaque
---
apiVersion: v1
kind: PersistentVolumeClaim
metadata:
  name: app-data
spec:
  accessModes:
  - ReadWriteOnce
---
apiVersion: v1
kind: Namespace
metadata:
  name: production
---
apiVersion: networking.k8s.io/v1
kind: Ingress
metadata:
  name: web-ingress
spec:
  rules:
  - host: example.com
---
apiVersion: v1
kind: ServiceAccount
metadata:
  name: web-sa
---
apiVersion: apps/v1
kind: StatefulSet
metadata:
  name: db
  labels:
    app: db
spec:
  selector:
    matchLabels:
      app: db
---
apiVersion: apps/v1
kind: DaemonSet
metadata:
  name: log-agent
  labels:
    app: logs
---
apiVersion: batch/v1
kind: Job
metadata:
  name: db-migrate
---
apiVersion: batch/v1
kind: CronJob
metadata:
  name: cleanup
";
        let (nodes, edges) = parse_k8s_manifest("k8s/app.yaml", yaml);

        // Verify all resource types are extracted
        assert!(nodes
            .iter()
            .any(|n| n.kind == InfraKind::K8sDeployment && n.name == "web-app"));
        assert!(nodes
            .iter()
            .any(|n| n.kind == InfraKind::K8sService && n.name == "web-svc"));
        assert!(nodes
            .iter()
            .any(|n| n.kind == InfraKind::K8sConfigMap && n.name == "app-config"));
        assert!(nodes
            .iter()
            .any(|n| n.kind == InfraKind::K8sSecret && n.name == "db-credentials"));
        assert!(nodes
            .iter()
            .any(|n| n.kind == InfraKind::K8sPvc && n.name == "app-data"));
        assert!(nodes
            .iter()
            .any(|n| n.kind == InfraKind::K8sNamespace && n.name == "production"));
        assert!(nodes
            .iter()
            .any(|n| n.kind == InfraKind::K8sIngress && n.name == "web-ingress"));
        assert!(nodes
            .iter()
            .any(|n| n.kind == InfraKind::K8sServiceAccount && n.name == "web-sa"));
        assert!(nodes
            .iter()
            .any(|n| n.kind == InfraKind::K8sStatefulSet && n.name == "db"));
        assert!(nodes
            .iter()
            .any(|n| n.kind == InfraKind::K8sDaemonSet && n.name == "log-agent"));
        assert!(nodes
            .iter()
            .any(|n| n.kind == InfraKind::K8sJob && n.name == "db-migrate"));
        assert!(nodes
            .iter()
            .any(|n| n.kind == InfraKind::K8sCronJob && n.name == "cleanup"));
        assert_eq!(nodes.len(), 12);

        // Verify cross-reference edges
        // Service -> Deployment via selector match (app: web)
        let routes_to: Vec<_> = edges
            .iter()
            .filter(|e| e.kind == InfraEdgeKind::RoutesTo)
            .collect();
        assert_eq!(
            routes_to.len(),
            1,
            "Service should route to Deployment by label match"
        );

        // Deployment -> ConfigMap, Secret, PVC via refs
        let depends_on: Vec<_> = edges
            .iter()
            .filter(|e| e.kind == InfraEdgeKind::DependsOn)
            .collect();
        assert_eq!(
            depends_on.len(),
            3,
            "Deployment should depend on ConfigMap, Secret, and PVC"
        );
    }

    #[test]
    fn test_k8s_single_resource() {
        let yaml = "\
apiVersion: v1
kind: ConfigMap
metadata:
  name: my-config
  namespace: staging
data:
  DB_HOST: localhost
";
        let (nodes, edges) = parse_k8s_manifest("k8s/config.yaml", yaml);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].kind, InfraKind::K8sConfigMap);
        assert_eq!(nodes[0].name, "my-config");
        assert_eq!(nodes[0].namespace, Some("staging".to_string()));
        assert!(edges.is_empty());
    }

    #[test]
    fn test_k8s_unknown_kind_skipped() {
        let yaml = "\
apiVersion: v1
kind: NetworkPolicy
metadata:
  name: deny-all
";
        let (nodes, _edges) = parse_k8s_manifest("k8s/netpol.yaml", yaml);
        assert!(nodes.is_empty(), "Unknown kind should be skipped");
    }

    #[test]
    fn test_kustomize_parsing() {
        let yaml = "\
apiVersion: kustomize.config.k8s.io/v1beta1
kind: Kustomization

resources:
- deployment.yaml
- service.yaml
- ../base

bases:
- ../../common

patches:
- patch-replicas.yaml

patchesStrategicMerge:
- patch-env.yaml

components:
- ../../components/monitoring

namespace: production
";
        let (nodes, edges) = parse_kustomize("overlays/prod/kustomization.yaml", yaml);

        // Should have one KustomizeOverlay node
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].kind, InfraKind::KustomizeOverlay);
        assert_eq!(nodes[0].name, "prod");

        // Should have edges for all referenced paths:
        // resources: 3 (deployment.yaml, service.yaml, ../base)
        // bases: 1 (../../common)
        // patches: 1 (patch-replicas.yaml)
        // patchesStrategicMerge: 1 (patch-env.yaml)
        // components: 1 (../../components/monitoring)
        assert_eq!(edges.len(), 7);

        // Check all edges are DependsOn
        assert!(edges.iter().all(|e| e.kind == InfraEdgeKind::DependsOn));

        // Verify resolved paths
        let resolved_paths: Vec<String> = edges
            .iter()
            .filter_map(|e| {
                e.properties
                    .get("resolved_path")
                    .and_then(|v| v.as_str())
                    .map(String::from)
            })
            .collect();
        assert!(resolved_paths.contains(&"overlays/prod/deployment.yaml".to_string()));
        assert!(resolved_paths.contains(&"overlays/prod/service.yaml".to_string()));
        assert!(resolved_paths.contains(&"overlays/prod/../base".to_string()));
        assert!(resolved_paths.contains(&"overlays/prod/../../common".to_string()));
        assert!(resolved_paths.contains(&"overlays/prod/../../components/monitoring".to_string()));
    }

    #[test]
    fn test_kustomize_root_overlay() {
        let yaml = "\
resources:
- namespace.yaml
";
        let (nodes, edges) = parse_kustomize("kustomization.yaml", yaml);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].kind, InfraKind::KustomizeOverlay);
        // Root-level kustomization has no parent dir → "root"
        assert_eq!(nodes[0].name, "root");
        assert_eq!(edges.len(), 1);
    }
}
