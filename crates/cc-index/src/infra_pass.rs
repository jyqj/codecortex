//! Independent infrastructure indexing pass.
//! Scans Dockerfile, docker-compose, K8s manifests — NOT part of the language parser pipeline.

use std::path::Path;

use cc_model::infra::{InfraEdge, InfraEdgeKind, InfraKind, InfraNode};
use cc_model::StableId;

/// Candidate infra files discovered by strong-feature filtering.
pub struct InfraCandidate {
    pub rel_path: String,
    pub abs_path: std::path::PathBuf,
    pub file_type: InfraFileType,
}

pub enum InfraFileType {
    Dockerfile,
    DockerCompose,
    K8sManifest,
    Kustomize,
    Terraform,
    CompileCommands,
}

/// Discover infra files using strong-feature filtering (not blanket *.yaml).
pub fn discover_infra_files(project_path: &Path) -> Vec<InfraCandidate> {
    let mut candidates = Vec::new();

    // Use ignore crate (same as Scanner) to respect .gitignore
    let walker = ignore::WalkBuilder::new(project_path)
        .hidden(true)
        .git_ignore(true)
        .build();

    for entry in walker.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let file_name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n,
            None => continue,
        };
        let rel_path = match path.strip_prefix(project_path) {
            Ok(r) => r.to_string_lossy().to_string(),
            Err(_) => continue,
        };

        let file_type = if file_name.starts_with("Dockerfile") {
            Some(InfraFileType::Dockerfile)
        } else if file_name.starts_with("docker-compose")
            && (file_name.ends_with(".yml") || file_name.ends_with(".yaml"))
        {
            Some(InfraFileType::DockerCompose)
        } else if file_name == "kustomization.yaml" || file_name == "kustomization.yml" {
            Some(InfraFileType::Kustomize)
        } else if file_name.ends_with(".tf") {
            Some(InfraFileType::Terraform)
        } else if file_name == "compile_commands.json" {
            Some(InfraFileType::CompileCommands)
        } else if (file_name.ends_with(".yaml") || file_name.ends_with(".yml"))
            && is_k8s_manifest(path)
        {
            // Generic YAML with apiVersion + kind — likely K8s manifest
            Some(InfraFileType::K8sManifest)
        } else {
            None
        };

        if let Some(ft) = file_type {
            candidates.push(InfraCandidate {
                rel_path,
                abs_path: path.to_path_buf(),
                file_type: ft,
            });
        }
    }
    candidates
}

/// Quick heuristic: does this YAML file contain both "apiVersion" and "kind:"?
fn is_k8s_manifest(path: &Path) -> bool {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return false,
    };
    // Only check first 50 lines for performance
    let head: String = content.lines().take(50).collect::<Vec<_>>().join("\n");
    head.contains("apiVersion") && head.contains("kind:")
}

/// Parse a Dockerfile into InfraNodes + InfraEdges.
///
/// Handles multi-stage builds (`FROM ... AS name`), `COPY --from=stage`,
/// `EXPOSE` directives, and tracks `ENV` key names as stage metadata.
pub fn parse_dockerfile(rel_path: &str, content: &str) -> (Vec<InfraNode>, Vec<InfraEdge>) {
    let mut nodes = Vec::new();
    let mut edges = Vec::new();

    // State tracking for multi-stage builds
    let mut current_stage: Option<String> = None; // node_id of the current stage
    let mut stage_node_ids: std::collections::HashMap<String, String> =
        std::collections::HashMap::new(); // alias → node_id
    let mut stage_count: u32 = 0;
    let mut current_env_keys: Vec<String> = Vec::new();

    for (line_num, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        let line_1based = (line_num + 1) as u32;

        // Case-insensitive FROM detection
        let from_rest = trimmed
            .strip_prefix("FROM ")
            .or_else(|| trimmed.strip_prefix("from "));

        if let Some(rest) = from_rest {
            // Flush ENV keys from previous stage
            flush_env_keys(&mut nodes, &current_stage, &mut current_env_keys);

            stage_count += 1;

            // Parse: FROM image:tag [AS alias]
            let parts: Vec<&str> = rest.split_whitespace().collect();
            let image_name = parts.first().copied().unwrap_or(rest).trim();

            // Check for AS alias (case-insensitive)
            let alias = if parts.len() >= 3 && parts[1].eq_ignore_ascii_case("AS") {
                Some(parts[2].to_string())
            } else {
                None
            };

            // Create DockerImage node
            let img_node_id = StableId::edge_id("infra", rel_path, line_1based, 0);
            nodes.push(InfraNode {
                node_id: img_node_id.clone(),
                file_path: rel_path.to_string(),
                kind: InfraKind::DockerImage,
                name: image_name.to_string(),
                namespace: None,
                line: line_1based,
                end_line: None,
                properties: serde_json::json!({"raw_from": trimmed}),
                bound_symbol_uid: None,
                binding_confidence: None,
            });

            // Create DockerStage node and link stage → image
            let stage_name = alias
                .clone()
                .unwrap_or_else(|| format!("stage-{}", stage_count - 1));
            let stage_node_id = StableId::edge_id("infra_stage", rel_path, line_1based, 0);

            let mut stage_props = serde_json::json!({
                "stage_index": stage_count - 1,
                "base_image": image_name,
            });
            if alias.is_none() {
                stage_props["unnamed"] = serde_json::json!(true);
            }

            nodes.push(InfraNode {
                node_id: stage_node_id.clone(),
                file_path: rel_path.to_string(),
                kind: InfraKind::DockerStage,
                name: stage_name.clone(),
                namespace: None,
                line: line_1based,
                end_line: None,
                properties: stage_props,
                bound_symbol_uid: None,
                binding_confidence: None,
            });

            // Edge: stage → base image (DependsOn)
            edges.push(InfraEdge {
                edge_id: StableId::edge_id("infra_e_stage", rel_path, line_1based, 0),
                source_node_id: stage_node_id.clone(),
                target_node_id: img_node_id,
                kind: InfraEdgeKind::DependsOn,
                confidence: 1.0,
                properties: serde_json::json!({}),
            });

            if alias.is_some() {
                stage_node_ids.insert(stage_name, stage_node_id.clone());
            }
            current_stage = Some(stage_node_id);
            continue;
        }

        // COPY --from=<stage> detection (case-insensitive)
        let copy_from = if let Some(after) = trimmed.strip_prefix("COPY --from=") {
            after.split_whitespace().next()
        } else if let Some(after) = trimmed.strip_prefix("copy --from=") {
            after.split_whitespace().next()
        } else {
            None
        };

        if let Some(source_stage) = copy_from {
            if let Some(ref cur_stage_id) = current_stage {
                if let Some(src_stage_id) = stage_node_ids.get(source_stage) {
                    edges.push(InfraEdge {
                        edge_id: StableId::edge_id("infra_e_copy", rel_path, line_1based, 0),
                        source_node_id: cur_stage_id.clone(),
                        target_node_id: src_stage_id.clone(),
                        kind: InfraEdgeKind::DependsOn,
                        confidence: 1.0,
                        properties: serde_json::json!({"copy_from": source_stage}),
                    });
                }
            }
            continue;
        }

        // EXPOSE detection (case-insensitive)
        let expose_rest = trimmed
            .strip_prefix("EXPOSE ")
            .or_else(|| trimmed.strip_prefix("expose "));

        if let Some(ports_str) = expose_rest {
            // EXPOSE can list multiple ports: EXPOSE 80 443
            for (idx, port) in ports_str.split_whitespace().enumerate() {
                let port_clean = port.trim();
                if port_clean.is_empty() {
                    continue;
                }
                let expose_node_id =
                    StableId::edge_id("infra_expose", rel_path, line_1based, idx as u32);
                nodes.push(InfraNode {
                    node_id: expose_node_id.clone(),
                    file_path: rel_path.to_string(),
                    kind: InfraKind::DockerExpose,
                    name: port_clean.to_string(),
                    namespace: None,
                    line: line_1based,
                    end_line: None,
                    properties: serde_json::json!({"port": port_clean}),
                    bound_symbol_uid: None,
                    binding_confidence: None,
                });

                // Link EXPOSE to current stage
                if let Some(ref stage_id) = current_stage {
                    edges.push(InfraEdge {
                        edge_id: StableId::edge_id(
                            "infra_e_expose",
                            rel_path,
                            line_1based,
                            idx as u32,
                        ),
                        source_node_id: stage_id.clone(),
                        target_node_id: expose_node_id,
                        kind: InfraEdgeKind::ExposesPort,
                        confidence: 1.0,
                        properties: serde_json::json!({}),
                    });
                }
            }
            continue;
        }

        // ENV detection (case-insensitive) — track key names as metadata
        let env_rest = trimmed
            .strip_prefix("ENV ")
            .or_else(|| trimmed.strip_prefix("env "));

        if let Some(env_str) = env_rest {
            // ENV KEY=value or ENV KEY value
            let key = if let Some(eq_pos) = env_str.find('=') {
                env_str[..eq_pos].trim().to_string()
            } else {
                env_str.split_whitespace().next().unwrap_or("").to_string()
            };
            if !key.is_empty() {
                current_env_keys.push(key);
            }
        }
    }

    // Flush trailing ENV keys for the last stage
    flush_env_keys(&mut nodes, &current_stage, &mut current_env_keys);

    (nodes, edges)
}

/// Flush accumulated ENV keys into the current stage node's properties.
fn flush_env_keys(
    nodes: &mut [InfraNode],
    current_stage: &Option<String>,
    env_keys: &mut Vec<String>,
) {
    if env_keys.is_empty() {
        return;
    }
    if let Some(ref stage_nid) = current_stage {
        for node in nodes.iter_mut() {
            if node.node_id == *stage_nid {
                node.properties["env_keys"] = serde_json::json!(*env_keys);
                break;
            }
        }
    }
    env_keys.clear();
}

/// Parse docker-compose YAML into InfraNodes + InfraEdges.
pub fn parse_docker_compose(rel_path: &str, content: &str) -> (Vec<InfraNode>, Vec<InfraEdge>) {
    let mut nodes = Vec::new();
    let mut edges = Vec::new();

    // Simple line-based YAML parsing (not full YAML parser).
    // Look for top-level "services:" section, then each service name at indent level 2.
    let mut in_services = false;
    let mut current_service: Option<(String, u32, String)> = None; // (name, line, node_id)
    let mut services_indent = 0usize;

    for (line_num, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        let indent = line.len() - line.trim_start().len();

        if trimmed == "services:" || trimmed.starts_with("services:") {
            in_services = true;
            services_indent = indent;
            continue;
        }

        // Left a top-level section
        if in_services
            && indent <= services_indent
            && !trimmed.is_empty()
            && !trimmed.starts_with('#')
        {
            in_services = false;
            current_service = None;
        }

        if !in_services {
            continue;
        }

        // Service name at indent == services_indent + 2
        if indent == services_indent + 2
            && trimmed.ends_with(':')
            && !trimmed.starts_with('#')
            && !trimmed.starts_with('-')
        {
            let svc_name = trimmed.trim_end_matches(':').trim();
            let node_id = StableId::edge_id("infra_svc", rel_path, (line_num + 1) as u32, 0);
            nodes.push(InfraNode {
                node_id: node_id.clone(),
                file_path: rel_path.to_string(),
                kind: InfraKind::ComposeService,
                name: svc_name.to_string(),
                namespace: None,
                line: (line_num + 1) as u32,
                end_line: None,
                properties: serde_json::json!({}),
                bound_symbol_uid: None,
                binding_confidence: None,
            });
            current_service = Some((svc_name.to_string(), (line_num + 1) as u32, node_id));
            continue;
        }

        // Inside a service block — look for image: and depends_on entries
        if let Some((ref _svc_name, _, ref svc_node_id)) = current_service {
            if indent > services_indent + 2 {
                let key_val = trimmed.trim_start_matches("- ");

                // image: xxx
                if let Some(img) = key_val.strip_prefix("image:") {
                    let img_name = img.trim().trim_matches('"').trim_matches('\'');
                    let img_node_id =
                        StableId::edge_id("infra_img", rel_path, (line_num + 1) as u32, 0);
                    nodes.push(InfraNode {
                        node_id: img_node_id.clone(),
                        file_path: rel_path.to_string(),
                        kind: InfraKind::DockerImage,
                        name: img_name.to_string(),
                        namespace: None,
                        line: (line_num + 1) as u32,
                        end_line: None,
                        properties: serde_json::json!({}),
                        bound_symbol_uid: None,
                        binding_confidence: None,
                    });
                    edges.push(InfraEdge {
                        edge_id: StableId::edge_id("infra_e", rel_path, (line_num + 1) as u32, 0),
                        source_node_id: svc_node_id.clone(),
                        target_node_id: img_node_id,
                        kind: InfraEdgeKind::UsesImage,
                        confidence: 0.9,
                        properties: serde_json::json!({}),
                    });
                }
            }
        }
    }
    (nodes, edges)
}

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

/// Parse Terraform `.tf` files into InfraNodes + InfraEdges.
///
/// Supports: resource, data, variable, output, module blocks.
/// Also extracts `var.X` references and creates `References` edges.
pub fn parse_terraform(file_path: &str, content: &str) -> (Vec<InfraNode>, Vec<InfraEdge>) {
    let mut nodes = Vec::new();
    let mut edges = Vec::new();

    let re_resource_data = regex::Regex::new(r#"^(resource|data)\s+"(\w+)"\s+"(\w+)""#).unwrap();
    let re_var_output = regex::Regex::new(r#"^(variable|output)\s+"(\w+)""#).unwrap();
    let re_module = regex::Regex::new(r#"^module\s+"(\w+)""#).unwrap();
    let re_source = regex::Regex::new(r#"source\s*=\s*"([^"]+)""#).unwrap();
    let re_var_ref = regex::Regex::new(r#"var\.(\w+)"#).unwrap();

    // Track variable node IDs for var.X reference edges
    let mut var_node_ids: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    // Track all var.X references per resource/data/output/module block
    struct PendingBlock {
        node_id: String,
        var_refs: Vec<String>,
    }
    let mut pending_blocks: Vec<PendingBlock> = Vec::new();

    // State for tracking current module block (to find its source)
    let mut current_module: Option<(String, String, u32)> = None; // (name, node_id, line)

    for (line_num, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        let line_1based = (line_num + 1) as u32;

        // resource "type" "name" {
        if let Some(caps) = re_resource_data.captures(trimmed) {
            let block_type = caps.get(1).unwrap().as_str();
            let type_name = caps.get(2).unwrap().as_str();
            let local_name = caps.get(3).unwrap().as_str();
            let kind = if block_type == "resource" {
                InfraKind::TerraformResource
            } else {
                InfraKind::TerraformDataSource
            };
            let name = format!("{}.{}", type_name, local_name);
            let node_id = StableId::edge_id("infra_tf", file_path, line_1based, 0);
            nodes.push(InfraNode {
                node_id: node_id.clone(),
                file_path: file_path.to_string(),
                kind,
                name,
                namespace: None,
                line: line_1based,
                end_line: None,
                properties: serde_json::json!({
                    "block_type": block_type,
                    "resource_type": type_name,
                    "local_name": local_name,
                }),
                bound_symbol_uid: None,
                binding_confidence: None,
            });
            pending_blocks.push(PendingBlock {
                node_id,
                var_refs: Vec::new(),
            });
            continue;
        }

        // variable "name" { / output "name" {
        if let Some(caps) = re_var_output.captures(trimmed) {
            let block_type = caps.get(1).unwrap().as_str();
            let var_name = caps.get(2).unwrap().as_str();
            let kind = if block_type == "variable" {
                InfraKind::TerraformVariable
            } else {
                InfraKind::TerraformOutput
            };
            let node_id = StableId::edge_id("infra_tf", file_path, line_1based, 0);
            nodes.push(InfraNode {
                node_id: node_id.clone(),
                file_path: file_path.to_string(),
                kind,
                name: var_name.to_string(),
                namespace: None,
                line: line_1based,
                end_line: None,
                properties: serde_json::json!({"block_type": block_type}),
                bound_symbol_uid: None,
                binding_confidence: None,
            });
            if block_type == "variable" {
                var_node_ids.insert(var_name.to_string(), node_id.clone());
            }
            if block_type == "output" {
                pending_blocks.push(PendingBlock {
                    node_id,
                    var_refs: Vec::new(),
                });
            }
            continue;
        }

        // module "name" {
        if let Some(caps) = re_module.captures(trimmed) {
            let mod_name = caps.get(1).unwrap().as_str();
            let node_id = StableId::edge_id("infra_tf", file_path, line_1based, 0);
            nodes.push(InfraNode {
                node_id: node_id.clone(),
                file_path: file_path.to_string(),
                kind: InfraKind::TerraformModule,
                name: mod_name.to_string(),
                namespace: None,
                line: line_1based,
                end_line: None,
                properties: serde_json::json!({}),
                bound_symbol_uid: None,
                binding_confidence: None,
            });
            current_module = Some((mod_name.to_string(), node_id.clone(), line_1based));
            pending_blocks.push(PendingBlock {
                node_id,
                var_refs: Vec::new(),
            });
            continue;
        }

        // source = "..." inside a module block
        if let Some((ref _mod_name, ref mod_node_id, mod_line)) = current_module {
            if let Some(caps) = re_source.captures(trimmed) {
                let source_path = caps.get(1).unwrap().as_str();
                let edge_id = StableId::edge_id("infra_tf_mod", file_path, mod_line, 0);
                edges.push(InfraEdge {
                    edge_id,
                    source_node_id: mod_node_id.clone(),
                    target_node_id: format!("tf_module_source:{}", source_path),
                    kind: InfraEdgeKind::UsesModule,
                    confidence: 0.9,
                    properties: serde_json::json!({"source": source_path}),
                });
                // Update module node properties with source
                if let Some(mod_node) = nodes.iter_mut().find(|n| n.node_id == *mod_node_id) {
                    mod_node.properties["source"] = serde_json::json!(source_path);
                }
            }
        }

        // Reset current_module when we hit a closing brace at column 0
        if trimmed == "}" && current_module.is_some() {
            let indent = line.len() - line.trim_start().len();
            if indent == 0 {
                current_module = None;
            }
        }

        // Collect var.X references in the current block
        if let Some(block) = pending_blocks.last_mut() {
            for caps in re_var_ref.captures_iter(trimmed) {
                let var_name = caps.get(1).unwrap().as_str().to_string();
                if !block.var_refs.contains(&var_name) {
                    block.var_refs.push(var_name);
                }
            }
        }
    }

    // Create References edges for var.X usages
    for block in &pending_blocks {
        for var_name in &block.var_refs {
            if let Some(var_node_id) = var_node_ids.get(var_name) {
                let edge_id = StableId::edge_id("infra_tf_ref", file_path, 0, edges.len() as u32);
                edges.push(InfraEdge {
                    edge_id,
                    source_node_id: block.node_id.clone(),
                    target_node_id: var_node_id.clone(),
                    kind: InfraEdgeKind::References,
                    confidence: 0.85,
                    properties: serde_json::json!({"var_name": var_name}),
                });
            }
        }
    }

    (nodes, edges)
}

/// Parse `compile_commands.json` into InfraNodes.
///
/// Each compilation entry becomes a `CompileTarget` node with extracted
/// include directories (`-I`) and defines (`-D`) stored in properties.
pub fn parse_compile_commands(file_path: &str, content: &str) -> (Vec<InfraNode>, Vec<InfraEdge>) {
    let mut nodes = Vec::new();
    let edges = Vec::new();

    // compile_commands.json is an array of objects
    let entries: Vec<serde_json::Value> = match serde_json::from_str(content) {
        Ok(v) => v,
        Err(_) => return (nodes, edges),
    };

    for (idx, entry) in entries.iter().enumerate() {
        let file = match entry.get("file").and_then(|v| v.as_str()) {
            Some(f) => f,
            None => continue,
        };
        let directory = entry
            .get("directory")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        // Extract command string — could be "command" or "arguments"
        let command_str = entry
            .get("command")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| {
                entry
                    .get("arguments")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|a| a.as_str())
                            .collect::<Vec<_>>()
                            .join(" ")
                    })
            })
            .unwrap_or_default();

        // Extract -I include dirs and -D defines from the command
        let mut include_dirs: Vec<String> = Vec::new();
        let mut defines: Vec<String> = Vec::new();

        let tokens: Vec<&str> = command_str.split_whitespace().collect();
        let mut i = 0;
        while i < tokens.len() {
            let token = tokens[i];
            if token == "-I" {
                // -I <dir>
                if i + 1 < tokens.len() {
                    include_dirs.push(tokens[i + 1].to_string());
                    i += 1;
                }
            } else if let Some(dir) = token.strip_prefix("-I") {
                // -I<dir>
                include_dirs.push(dir.to_string());
            } else if token == "-D" {
                // -D <define>
                if i + 1 < tokens.len() {
                    defines.push(tokens[i + 1].to_string());
                    i += 1;
                }
            } else if let Some(def) = token.strip_prefix("-D") {
                // -D<define>
                defines.push(def.to_string());
            }
            i += 1;
        }

        let node_id = StableId::edge_id("infra_cc", file_path, (idx + 1) as u32, 0);
        nodes.push(InfraNode {
            node_id,
            file_path: file_path.to_string(),
            kind: InfraKind::CompileTarget,
            name: file.to_string(),
            namespace: None,
            line: (idx + 1) as u32,
            end_line: None,
            properties: serde_json::json!({
                "directory": directory,
                "include_dirs": include_dirs,
                "defines": defines,
            }),
            bound_symbol_uid: None,
            binding_confidence: None,
        });
    }

    (nodes, edges)
}

/// Bind infra nodes to code symbols by name matching.
///
/// Strategy:
/// 1. Normalize infra node name: "user-api" -> "user_api"
/// 2. Exact match against symbol names (case-insensitive)
/// 3. Prefix match: "user_api" matches "user_api_service"
/// 4. Contains match: "user_api" found inside "big_user_api_handler"
/// 5. Best match wins, with confidence score
pub fn bind_infra_to_symbols(
    infra_nodes: &mut [InfraNode],
    symbols: &[cc_model::symbol::SymbolRecord],
) {
    for node in infra_nodes.iter_mut() {
        let normalized = node.name.to_lowercase().replace(['-', '.'], "_");
        if normalized.is_empty() {
            continue;
        }

        let mut best_uid: Option<String> = None;
        let mut best_confidence = 0.0_f64;

        for sym in symbols {
            let sym_lower = sym.name.to_lowercase();

            // Exact match
            if sym_lower == normalized {
                best_uid = sym.symbol_uid.clone();
                best_confidence = 1.0;
                break;
            }

            // Prefix match
            if sym_lower.starts_with(&normalized) && best_confidence < 0.85 {
                best_uid = sym.symbol_uid.clone();
                best_confidence = 0.85;
            }

            // Contains match
            if sym_lower.contains(&normalized) && best_confidence < 0.7 {
                best_uid = sym.symbol_uid.clone();
                best_confidence = 0.7;
            }
        }

        if best_confidence >= 0.7 {
            node.bound_symbol_uid = best_uid;
            node.binding_confidence = Some(best_confidence);
        }
    }
}

/// Run the full infra pass: discover -> parse -> return nodes + edges.
pub fn run_infra_pass(project_path: &Path) -> (Vec<InfraNode>, Vec<InfraEdge>) {
    let candidates = discover_infra_files(project_path);
    let mut all_nodes = Vec::new();
    let mut all_edges = Vec::new();

    for candidate in &candidates {
        let content = match std::fs::read_to_string(&candidate.abs_path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let (nodes, edges) = match candidate.file_type {
            InfraFileType::Dockerfile => parse_dockerfile(&candidate.rel_path, &content),
            InfraFileType::DockerCompose => parse_docker_compose(&candidate.rel_path, &content),
            InfraFileType::K8sManifest => parse_k8s_manifest(&candidate.rel_path, &content),
            InfraFileType::Kustomize => parse_kustomize(&candidate.rel_path, &content),
            InfraFileType::Terraform => parse_terraform(&candidate.rel_path, &content),
            InfraFileType::CompileCommands => parse_compile_commands(&candidate.rel_path, &content),
        };

        all_nodes.extend(nodes);
        all_edges.extend(edges);
    }

    // Scan all YAML/JSON config files for topic/queue → endpoint bindings
    extract_infra_bindings(project_path, &mut all_nodes, &mut all_edges);

    (all_nodes, all_edges)
}

// ---------------------------------------------------------------------------
// Topic / Queue binding extraction
// ---------------------------------------------------------------------------

/// Known YAML keys that identify a topic or queue name.
const SOURCE_KEYS: &[&str] = &[
    "topic",
    "queue",
    "queue_name",
    "subscription",
    "subject",
    "channel",
    "stream",
];

/// Known YAML keys that identify an endpoint URL.
const TARGET_KEYS: &[&str] = &[
    "push_endpoint",
    "uri",
    "url",
    "endpoint",
    "http_target",
    "target_url",
    "webhook_url",
    "callback_url",
];

/// A raw binding extracted from a YAML config file.
#[derive(Debug)]
struct RawBinding {
    /// Topic or queue name
    source_name: String,
    /// Target endpoint URL
    target_url: String,
    /// Inferred broker type (pubsub, kafka, sqs, ...)
    broker: String,
    /// Line number where the source key was found (1-based)
    source_line: u32,
}

/// Scan YAML config files for topic/queue → endpoint bindings and produce
/// `InfraNode` + `InfraEdge` entries.
fn extract_infra_bindings(
    project_path: &Path,
    infra_nodes: &mut Vec<InfraNode>,
    infra_edges: &mut Vec<InfraEdge>,
) {
    let walker = ignore::WalkBuilder::new(project_path)
        .hidden(true)
        .git_ignore(true)
        .build();

    for entry in walker.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !matches!(ext, "yaml" | "yml" | "json") {
            continue;
        }
        // Skip directories that are unlikely to contain IaC config
        let path_str = path.to_string_lossy();
        if path_str.contains("node_modules")
            || path_str.contains("vendor")
            || path_str.contains(".git/")
        {
            continue;
        }
        let rel_path = match path.strip_prefix(project_path) {
            Ok(r) => r.to_string_lossy().to_string(),
            Err(_) => continue,
        };
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let bindings = scan_yaml_for_bindings(&content, &rel_path);
        for binding in bindings {
            let kind = if binding.broker == "sqs"
                || binding.broker == "cloud_tasks"
                || binding.source_name.to_lowercase().contains("queue")
            {
                InfraKind::MessageQueue
            } else {
                InfraKind::MessageTopic
            };

            let edge_kind = if kind == InfraKind::MessageQueue {
                InfraEdgeKind::ConsumesQueue
            } else {
                InfraEdgeKind::BindsTopic
            };

            let node_id = StableId::edge_id("infra_msg", &rel_path, binding.source_line, 0);

            infra_nodes.push(InfraNode {
                node_id: node_id.clone(),
                file_path: rel_path.clone(),
                kind,
                name: binding.source_name.clone(),
                namespace: None,
                line: binding.source_line,
                end_line: None,
                properties: serde_json::json!({
                    "broker": binding.broker,
                    "target_url": binding.target_url,
                }),
                bound_symbol_uid: None,
                binding_confidence: None,
            });

            // Create an edge: topic/queue → target endpoint
            // The target_node_id is a placeholder that can be resolved to a
            // route_node later in the indexer.
            let edge_id = StableId::edge_id("infra_bind", &rel_path, binding.source_line, 0);

            infra_edges.push(InfraEdge {
                edge_id,
                source_node_id: node_id,
                target_node_id: format!("endpoint:{}", binding.target_url),
                kind: edge_kind,
                confidence: 0.8,
                properties: serde_json::json!({
                    "source_name": binding.source_name,
                    "target_url": binding.target_url,
                    "broker": binding.broker,
                }),
            });
        }
    }
}

/// Simple line-based YAML scanner for infra bindings.
///
/// Finds source keys (topic, queue, ...) and target keys (push_endpoint, url, ...)
/// that appear within the same indentation block, and emits a binding for each pair.
fn scan_yaml_for_bindings(content: &str, file_path: &str) -> Vec<RawBinding> {
    let mut bindings = Vec::new();
    let mut current_source: Option<(String, u32)> = None; // (value, line)
    let mut current_target: Option<String> = None;
    // The indentation level at which the current source/target keys live.
    // A line with indent strictly less than this means a new parent block started.
    let mut block_indent: Option<usize> = None;

    for (line_idx, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let indent = line.len() - line.trim_start().len();

        // When indent drops strictly below the block level, we left the
        // current block — flush any accumulated source+target pair.
        if let Some(bi) = block_indent {
            if indent < bi {
                if let (Some((src, src_line)), Some(tgt)) =
                    (current_source.take(), current_target.take())
                {
                    bindings.push(RawBinding {
                        broker: infer_broker(file_path, &src),
                        source_name: src,
                        target_url: tgt,
                        source_line: src_line,
                    });
                }
                current_source = None;
                current_target = None;
                block_indent = None;
            }
        }

        if let Some((key, value)) = parse_yaml_kv(trimmed) {
            let key_lower = key.to_lowercase();

            let is_source = SOURCE_KEYS.contains(&key_lower.as_str()) && !value.is_empty();
            let is_target = TARGET_KEYS.contains(&key_lower.as_str())
                && (value.contains("://") || value.starts_with('/'));

            // If we hit a new source key while we already have a pending
            // source (possibly with a target), flush the previous pair first.
            if is_source && current_source.is_some() {
                if let (Some((src, src_line)), Some(tgt)) =
                    (current_source.take(), current_target.take())
                {
                    bindings.push(RawBinding {
                        broker: infer_broker(file_path, &src),
                        source_name: src,
                        target_url: tgt,
                        source_line: src_line,
                    });
                }
                current_source = None;
                current_target = None;
            }

            if is_source {
                current_source = Some((strip_quotes(&value), (line_idx + 1) as u32));
                block_indent = Some(indent);
            }
            if is_target {
                current_target = Some(strip_quotes(&value));
                if block_indent.is_none() {
                    block_indent = Some(indent);
                }
            }
        }
    }

    // Flush trailing binding
    if let (Some((src, src_line)), Some(tgt)) = (current_source, current_target) {
        bindings.push(RawBinding {
            broker: infer_broker(file_path, &src),
            source_name: src,
            target_url: tgt,
            source_line: src_line,
        });
    }

    bindings
}

/// Parse a YAML-like `key: value` line. Returns `None` for non-KV lines.
fn parse_yaml_kv(trimmed: &str) -> Option<(String, String)> {
    // Skip list items for key extraction (but allow "- key: value")
    let line = if let Some(rest) = trimmed.strip_prefix("- ") {
        rest.trim()
    } else {
        trimmed
    };
    let colon_pos = line.find(':')?;
    let key = line[..colon_pos].trim().to_string();
    if key.is_empty() || key.contains(' ') {
        return None;
    }
    let value = line[colon_pos + 1..].trim().to_string();
    Some((key, value))
}

/// Strip surrounding single or double quotes from a YAML value.
fn strip_quotes(value: &str) -> String {
    let trimmed = value.trim();
    if (trimmed.starts_with('"') && trimmed.ends_with('"'))
        || (trimmed.starts_with('\'') && trimmed.ends_with('\''))
    {
        trimmed[1..trimmed.len() - 1].to_string()
    } else {
        trimmed.to_string()
    }
}

/// Infer the message broker from the file path and source key name.
fn infer_broker(file_path: &str, source_key: &str) -> String {
    let fp = file_path.to_lowercase();
    if fp.contains("pubsub") {
        return "pubsub".into();
    }
    if fp.contains("kafka") || source_key == "stream" {
        return "kafka".into();
    }
    if fp.contains("sns") {
        return "sns".into();
    }
    if fp.contains("sqs") {
        return "sqs".into();
    }
    if fp.contains("scheduler") {
        return "cloud_scheduler".into();
    }
    if source_key == "queue" || fp.contains("task") {
        return "cloud_tasks".into();
    }
    "async".into()
}

/// Try to match infra binding target URLs to known route nodes.
/// Updates `InfraEdge.target_node_id` when a match is found.
pub fn match_bindings_to_routes(
    infra_edges: &mut [InfraEdge],
    route_nodes: &[cc_model::edge::RouteNodeRecord],
) {
    use cc_model::route_normalize::normalize_route_path;

    if route_nodes.is_empty() {
        return;
    }

    // Build a lookup: normalized_path → route_id
    let route_lookup: std::collections::HashMap<String, &str> = route_nodes
        .iter()
        .filter_map(|r| {
            let norm = r.normalized_path.as_deref().unwrap_or(
                // Should not happen — normalized_path is always set during indexing
                "",
            );
            if norm.is_empty() {
                None
            } else {
                Some((norm.to_string(), r.route_id.as_str()))
            }
        })
        .collect();

    for edge in infra_edges.iter_mut() {
        if !matches!(
            edge.kind,
            InfraEdgeKind::BindsTopic | InfraEdgeKind::ConsumesQueue
        ) {
            continue;
        }
        let target_url = match edge.properties.get("target_url").and_then(|v| v.as_str()) {
            Some(u) => u.to_string(),
            None => continue,
        };
        let normalized = normalize_route_path(&target_url);
        if let Some(&route_id) = route_lookup.get(&normalized) {
            edge.target_node_id = route_id.to_string();
            edge.confidence = 0.9; // upgraded confidence when matched
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scan_yaml_for_bindings_pubsub() {
        let yaml = "\
subscription:
  topic: orders-topic
  push_endpoint: https://api.example.com/webhooks/orders
";
        let bindings = scan_yaml_for_bindings(yaml, "config/pubsub.yaml");
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].source_name, "orders-topic");
        assert_eq!(
            bindings[0].target_url,
            "https://api.example.com/webhooks/orders"
        );
        assert_eq!(bindings[0].broker, "pubsub");
    }

    #[test]
    fn test_scan_yaml_for_bindings_kafka() {
        let yaml = "\
consumer:
  stream: events-stream
  endpoint: https://internal/v1/events/handle
";
        let bindings = scan_yaml_for_bindings(yaml, "deploy/kafka.yml");
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].source_name, "events-stream");
        assert_eq!(bindings[0].broker, "kafka");
    }

    #[test]
    fn test_scan_yaml_for_bindings_multiple_blocks() {
        let yaml = "\
subscriptions:
  - topic: topic-a
    push_endpoint: https://svc/a
  - topic: topic-b
    push_endpoint: https://svc/b
";
        let bindings = scan_yaml_for_bindings(yaml, "config/pubsub.yaml");
        assert_eq!(bindings.len(), 2);
        assert_eq!(bindings[0].source_name, "topic-a");
        assert_eq!(bindings[1].source_name, "topic-b");
    }

    #[test]
    fn test_scan_yaml_for_bindings_quoted_values() {
        let yaml = "\
config:
  topic: \"my-topic\"
  url: 'https://svc/handler'
";
        let bindings = scan_yaml_for_bindings(yaml, "infra/config.yaml");
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].source_name, "my-topic");
        assert_eq!(bindings[0].target_url, "https://svc/handler");
    }

    #[test]
    fn test_scan_yaml_no_target_no_binding() {
        let yaml = "\
config:
  topic: orphan-topic
  retries: 3
";
        let bindings = scan_yaml_for_bindings(yaml, "config/misc.yaml");
        assert_eq!(bindings.len(), 0);
    }

    #[test]
    fn test_infer_broker_from_filepath() {
        assert_eq!(infer_broker("infra/sns-config.yaml", "topic"), "sns");
        assert_eq!(infer_broker("infra/sqs.tf", "queue"), "sqs");
        assert_eq!(infer_broker("config/pubsub.yaml", "topic"), "pubsub");
        assert_eq!(infer_broker("deploy/kafka.yml", "stream"), "kafka");
        assert_eq!(infer_broker("config/tasks.yaml", "queue"), "cloud_tasks");
        assert_eq!(
            infer_broker("config/scheduler.yaml", "topic"),
            "cloud_scheduler"
        );
        assert_eq!(infer_broker("config/generic.yaml", "topic"), "async");
    }

    #[test]
    fn test_infer_broker_sns_not_sqs() {
        // SNS 文件不应返回 "sqs"
        assert_eq!(infer_broker("infra/sns-notifications.yaml", "topic"), "sns");
        assert_ne!(infer_broker("infra/sns-notifications.yaml", "topic"), "sqs");
        // SQS 文件返回 "sqs"
        assert_eq!(infer_broker("deploy/sqs-worker.tf", "queue"), "sqs");
    }

    #[test]
    fn test_parse_yaml_kv() {
        assert_eq!(
            parse_yaml_kv("topic: my-topic"),
            Some(("topic".into(), "my-topic".into()))
        );
        assert_eq!(
            parse_yaml_kv("push_endpoint: https://x"),
            Some(("push_endpoint".into(), "https://x".into()))
        );
        assert_eq!(parse_yaml_kv("# comment"), None);
        assert_eq!(parse_yaml_kv("just a line"), None);
        // list item
        assert_eq!(
            parse_yaml_kv("- topic: orders"),
            Some(("topic".into(), "orders".into()))
        );
    }

    #[test]
    fn test_strip_quotes() {
        assert_eq!(strip_quotes("\"hello\""), "hello");
        assert_eq!(strip_quotes("'world'"), "world");
        assert_eq!(strip_quotes("plain"), "plain");
    }

    #[test]
    fn test_scan_yaml_path_based_target() {
        let yaml = "\
webhook:
  topic: notifications
  push_endpoint: /api/v1/notifications/webhook
";
        let bindings = scan_yaml_for_bindings(yaml, "config/pubsub.yaml");
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].target_url, "/api/v1/notifications/webhook");
    }

    #[test]
    fn test_dockerfile_multistage() {
        let content = r#"FROM node:18 AS builder
WORKDIR /app
COPY . .
RUN npm run build
ENV NODE_ENV=production

FROM nginx:alpine AS runtime
COPY --from=builder /app/dist /usr/share/nginx/html
EXPOSE 80
ENV PORT=80
"#;
        let (nodes, edges) = parse_dockerfile("Dockerfile", content);

        // DockerImage nodes for "node:18" and "nginx:alpine"
        let image_nodes: Vec<_> = nodes
            .iter()
            .filter(|n| n.kind == InfraKind::DockerImage)
            .collect();
        assert_eq!(image_nodes.len(), 2);
        assert_eq!(image_nodes[0].name, "node:18");
        assert_eq!(image_nodes[1].name, "nginx:alpine");

        // DockerStage nodes for "builder" and "runtime"
        let stage_nodes: Vec<_> = nodes
            .iter()
            .filter(|n| n.kind == InfraKind::DockerStage)
            .collect();
        assert_eq!(stage_nodes.len(), 2);
        assert_eq!(stage_nodes[0].name, "builder");
        assert_eq!(stage_nodes[1].name, "runtime");

        // DockerExpose node for "80"
        let expose_nodes: Vec<_> = nodes
            .iter()
            .filter(|n| n.kind == InfraKind::DockerExpose)
            .collect();
        assert_eq!(expose_nodes.len(), 1);
        assert_eq!(expose_nodes[0].name, "80");

        // Edges: stage→image (DependsOn) x2 + copy-from (DependsOn) x1 + expose (ExposesPort) x1
        let depends_edges: Vec<_> = edges
            .iter()
            .filter(|e| e.kind == InfraEdgeKind::DependsOn)
            .collect();
        assert_eq!(depends_edges.len(), 3); // 2 stage→image + 1 copy-from

        // Verify COPY --from=builder creates edge from runtime → builder
        let copy_edge = edges
            .iter()
            .find(|e| e.properties.get("copy_from").and_then(|v| v.as_str()) == Some("builder"))
            .expect("should have copy-from edge");
        assert_eq!(copy_edge.source_node_id, stage_nodes[1].node_id);
        assert_eq!(copy_edge.target_node_id, stage_nodes[0].node_id);

        // Verify ExposesPort edge
        let expose_edges: Vec<_> = edges
            .iter()
            .filter(|e| e.kind == InfraEdgeKind::ExposesPort)
            .collect();
        assert_eq!(expose_edges.len(), 1);
        assert_eq!(expose_edges[0].source_node_id, stage_nodes[1].node_id);
        assert_eq!(expose_edges[0].target_node_id, expose_nodes[0].node_id);

        // Verify ENV keys stored as metadata on stage nodes
        let builder_env = stage_nodes[0]
            .properties
            .get("env_keys")
            .and_then(|v| v.as_array())
            .expect("builder should have env_keys");
        assert_eq!(builder_env.len(), 1);
        assert_eq!(builder_env[0].as_str().unwrap(), "NODE_ENV");

        let runtime_env = stage_nodes[1]
            .properties
            .get("env_keys")
            .and_then(|v| v.as_array())
            .expect("runtime should have env_keys");
        assert_eq!(runtime_env.len(), 1);
        assert_eq!(runtime_env[0].as_str().unwrap(), "PORT");
    }

    #[test]
    fn test_dockerfile_single_stage_no_alias() {
        let content = "FROM python:3.11\nRUN pip install flask\nEXPOSE 5000\n";
        let (nodes, edges) = parse_dockerfile("Dockerfile", content);

        let image_nodes: Vec<_> = nodes
            .iter()
            .filter(|n| n.kind == InfraKind::DockerImage)
            .collect();
        assert_eq!(image_nodes.len(), 1);
        assert_eq!(image_nodes[0].name, "python:3.11");

        // Unnamed stage
        let stage_nodes: Vec<_> = nodes
            .iter()
            .filter(|n| n.kind == InfraKind::DockerStage)
            .collect();
        assert_eq!(stage_nodes.len(), 1);
        assert_eq!(stage_nodes[0].name, "stage-0");
        assert_eq!(
            stage_nodes[0]
                .properties
                .get("unnamed")
                .and_then(|v| v.as_bool()),
            Some(true)
        );

        // EXPOSE 5000
        let expose_nodes: Vec<_> = nodes
            .iter()
            .filter(|n| n.kind == InfraKind::DockerExpose)
            .collect();
        assert_eq!(expose_nodes.len(), 1);
        assert_eq!(expose_nodes[0].name, "5000");

        // Edges: 1 stage→image + 1 expose
        assert_eq!(edges.len(), 2);
    }

    #[test]
    fn test_dockerfile_expose_multiple_ports() {
        let content = "FROM nginx:latest\nEXPOSE 80 443\n";
        let (nodes, _edges) = parse_dockerfile("Dockerfile", content);

        let expose_nodes: Vec<_> = nodes
            .iter()
            .filter(|n| n.kind == InfraKind::DockerExpose)
            .collect();
        assert_eq!(expose_nodes.len(), 2);
        assert_eq!(expose_nodes[0].name, "80");
        assert_eq!(expose_nodes[1].name, "443");
    }

    #[test]
    fn test_dockerfile_expose_with_protocol() {
        let content = "FROM redis:7\nEXPOSE 6379/tcp\n";
        let (nodes, _edges) = parse_dockerfile("Dockerfile", content);

        let expose_nodes: Vec<_> = nodes
            .iter()
            .filter(|n| n.kind == InfraKind::DockerExpose)
            .collect();
        assert_eq!(expose_nodes.len(), 1);
        assert_eq!(expose_nodes[0].name, "6379/tcp");
    }

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

    #[test]
    fn test_parse_terraform_basic() {
        let tf = r#"
variable "instance_type" {
  default = "t3.micro"
}

variable "region" {
  default = "us-east-1"
}

resource "aws_instance" "main" {
  ami           = "ami-12345"
  instance_type = var.instance_type
}

data "aws_ami" "ubuntu" {
  most_recent = true
}

output "instance_ip" {
  value = aws_instance.main.public_ip
}
"#;
        let (nodes, edges) = parse_terraform("infra/main.tf", tf);

        // 2 variables + 1 resource + 1 data + 1 output = 5 nodes
        assert_eq!(nodes.len(), 5);

        // Check resource
        let resource = nodes
            .iter()
            .find(|n| n.kind == InfraKind::TerraformResource)
            .unwrap();
        assert_eq!(resource.name, "aws_instance.main");

        // Check data source
        let data = nodes
            .iter()
            .find(|n| n.kind == InfraKind::TerraformDataSource)
            .unwrap();
        assert_eq!(data.name, "aws_ami.ubuntu");

        // Check variables
        let vars: Vec<_> = nodes
            .iter()
            .filter(|n| n.kind == InfraKind::TerraformVariable)
            .collect();
        assert_eq!(vars.len(), 2);

        // Check output
        let output = nodes
            .iter()
            .find(|n| n.kind == InfraKind::TerraformOutput)
            .unwrap();
        assert_eq!(output.name, "instance_ip");

        // The resource block references var.instance_type → References edge
        let ref_edges: Vec<_> = edges
            .iter()
            .filter(|e| e.kind == InfraEdgeKind::References)
            .collect();
        assert!(
            ref_edges.iter().any(|e| {
                e.properties.get("var_name").and_then(|v| v.as_str()) == Some("instance_type")
            }),
            "resource should reference var.instance_type"
        );
    }

    #[test]
    fn test_parse_terraform_module() {
        let tf = r#"
module "vpc" {
  source = "./modules/vpc"
  cidr   = "10.0.0.0/16"
}

module "eks" {
  source  = "terraform-aws-modules/eks/aws"
  version = "19.0"
}
"#;
        let (nodes, edges) = parse_terraform("infra/main.tf", tf);

        // 2 module nodes
        let modules: Vec<_> = nodes
            .iter()
            .filter(|n| n.kind == InfraKind::TerraformModule)
            .collect();
        assert_eq!(modules.len(), 2);
        assert_eq!(modules[0].name, "vpc");
        assert_eq!(modules[1].name, "eks");

        // 2 UsesModule edges
        let mod_edges: Vec<_> = edges
            .iter()
            .filter(|e| e.kind == InfraEdgeKind::UsesModule)
            .collect();
        assert_eq!(mod_edges.len(), 2);

        // Check source paths
        let sources: Vec<&str> = mod_edges
            .iter()
            .filter_map(|e| e.properties.get("source").and_then(|v| v.as_str()))
            .collect();
        assert!(sources.contains(&"./modules/vpc"));
        assert!(sources.contains(&"terraform-aws-modules/eks/aws"));

        // vpc module node should have source in properties
        let vpc_node = modules.iter().find(|n| n.name == "vpc").unwrap();
        assert_eq!(
            vpc_node.properties.get("source").and_then(|v| v.as_str()),
            Some("./modules/vpc")
        );
    }

    #[test]
    fn test_parse_terraform_variable_reference() {
        let tf = r#"
variable "env" {
  default = "staging"
}

variable "region" {
  default = "us-west-2"
}

resource "aws_s3_bucket" "logs" {
  bucket = "logs-${var.env}-${var.region}"
  tags = {
    Environment = var.env
  }
}

output "bucket_name" {
  value = aws_s3_bucket.logs.id
}
"#;
        let (nodes, edges) = parse_terraform("infra/vars.tf", tf);

        // 2 variables + 1 resource + 1 output = 4 nodes
        assert_eq!(nodes.len(), 4, "expected 4 nodes, got {}", nodes.len());

        // The resource block references both var.env and var.region
        let ref_edges: Vec<_> = edges
            .iter()
            .filter(|e| e.kind == InfraEdgeKind::References)
            .collect();

        let resource_node = nodes
            .iter()
            .find(|n| n.kind == InfraKind::TerraformResource)
            .unwrap();

        let resource_refs: Vec<_> = ref_edges
            .iter()
            .filter(|e| e.source_node_id == resource_node.node_id)
            .collect();
        assert_eq!(
            resource_refs.len(),
            2,
            "resource should reference 2 variables (env, region), got {}",
            resource_refs.len()
        );

        let ref_var_names: Vec<&str> = resource_refs
            .iter()
            .filter_map(|e| e.properties.get("var_name").and_then(|v| v.as_str()))
            .collect();
        assert!(
            ref_var_names.contains(&"env"),
            "missing reference to var.env"
        );
        assert!(
            ref_var_names.contains(&"region"),
            "missing reference to var.region"
        );

        // All References edges should point to actual variable node IDs
        for re in &resource_refs {
            assert!(
                nodes.iter().any(|n| n.node_id == re.target_node_id),
                "References edge target should match a variable node"
            );
        }

        // The output block should NOT reference any vars (no var.X usage)
        let output_node = nodes
            .iter()
            .find(|n| n.kind == InfraKind::TerraformOutput)
            .unwrap();
        let output_refs: Vec<_> = ref_edges
            .iter()
            .filter(|e| e.source_node_id == output_node.node_id)
            .collect();
        assert!(
            output_refs.is_empty(),
            "output block without var.X should have no References edges"
        );
    }

    #[test]
    fn test_parse_compile_commands_basic() {
        let json = r#"[
  {
    "directory": "/home/user/project/build",
    "command": "gcc -I/usr/include -I../include -DDEBUG -DVERSION=2 -o main.o -c main.c",
    "file": "main.c"
  },
  {
    "directory": "/home/user/project/build",
    "arguments": ["g++", "-I", "/usr/local/include", "-DNDEBUG", "-c", "util.cpp"],
    "file": "util.cpp"
  }
]"#;
        let (nodes, edges) = parse_compile_commands("compile_commands.json", json);

        assert_eq!(nodes.len(), 2);
        assert!(edges.is_empty());

        // First target: main.c
        assert_eq!(nodes[0].kind, InfraKind::CompileTarget);
        assert_eq!(nodes[0].name, "main.c");
        let inc0 = nodes[0]
            .properties
            .get("include_dirs")
            .unwrap()
            .as_array()
            .unwrap();
        assert_eq!(inc0.len(), 2);
        assert_eq!(inc0[0].as_str().unwrap(), "/usr/include");
        assert_eq!(inc0[1].as_str().unwrap(), "../include");
        let def0 = nodes[0]
            .properties
            .get("defines")
            .unwrap()
            .as_array()
            .unwrap();
        assert_eq!(def0.len(), 2);
        assert_eq!(def0[0].as_str().unwrap(), "DEBUG");
        assert_eq!(def0[1].as_str().unwrap(), "VERSION=2");

        // Second target: util.cpp (uses "arguments" form with -I <dir>)
        assert_eq!(nodes[1].name, "util.cpp");
        let inc1 = nodes[1]
            .properties
            .get("include_dirs")
            .unwrap()
            .as_array()
            .unwrap();
        assert_eq!(inc1.len(), 1);
        assert_eq!(inc1[0].as_str().unwrap(), "/usr/local/include");
        let def1 = nodes[1]
            .properties
            .get("defines")
            .unwrap()
            .as_array()
            .unwrap();
        assert_eq!(def1.len(), 1);
        assert_eq!(def1[0].as_str().unwrap(), "NDEBUG");
    }
}
