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
            Some(InfraFileType::K8sManifest)
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
pub fn parse_dockerfile(rel_path: &str, content: &str) -> (Vec<InfraNode>, Vec<InfraEdge>) {
    let mut nodes = Vec::new();
    let edges = Vec::new();

    for (line_num, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("FROM ") {
            // FROM image:tag [AS alias]
            let image_clean = rest
                .split_whitespace()
                .next()
                .unwrap_or(rest)
                .split(" AS ")
                .next()
                .unwrap_or(rest)
                .trim();

            let node_id = StableId::edge_id("infra", rel_path, (line_num + 1) as u32, 0);
            nodes.push(InfraNode {
                node_id,
                file_path: rel_path.to_string(),
                kind: InfraKind::DockerImage,
                name: image_clean.to_string(),
                namespace: None,
                line: (line_num + 1) as u32,
                end_line: None,
                properties: serde_json::json!({"raw_from": trimmed}),
                bound_symbol_uid: None,
                binding_confidence: None,
            });
        }
    }
    (nodes, edges)
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
pub fn parse_k8s_manifest(rel_path: &str, content: &str) -> (Vec<InfraNode>, Vec<InfraEdge>) {
    let mut nodes = Vec::new();
    let edges = Vec::new();

    let mut kind_str = None;
    let mut name_str = None;
    let mut namespace_str = None;
    let mut in_metadata = false;

    for (line_num, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        let indent = line.len() - line.trim_start().len();

        if let Some(k) = trimmed.strip_prefix("kind:") {
            kind_str = Some((k.trim().to_string(), line_num + 1));
        }
        if trimmed == "metadata:" {
            in_metadata = true;
            continue;
        }
        if in_metadata && indent == 0 && !trimmed.is_empty() {
            in_metadata = false;
        }
        if in_metadata {
            if let Some(n) = trimmed.strip_prefix("name:") {
                name_str = Some(n.trim().trim_matches('"').trim_matches('\'').to_string());
            }
            if let Some(ns) = trimmed.strip_prefix("namespace:") {
                namespace_str = Some(ns.trim().trim_matches('"').trim_matches('\'').to_string());
            }
        }
    }

    if let (Some((kind, line)), Some(name)) = (kind_str, name_str) {
        let infra_kind = match kind.as_str() {
            "Deployment" => Some(InfraKind::K8sDeployment),
            "Service" => Some(InfraKind::K8sService),
            _ => None,
        };

        if let Some(ik) = infra_kind {
            let node_id = StableId::edge_id("infra_k8s", rel_path, line as u32, 0);
            nodes.push(InfraNode {
                node_id,
                file_path: rel_path.to_string(),
                kind: ik,
                name,
                namespace: namespace_str,
                line: line as u32,
                end_line: None,
                properties: serde_json::json!({"k8s_kind": kind}),
                bound_symbol_uid: None,
                binding_confidence: None,
            });
        }
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
        let normalized = node.name.to_lowercase().replace('-', "_").replace('.', "_");
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
            let norm = r.normalized_path.as_deref().unwrap_or_else(|| {
                // Should not happen — normalized_path is always set during indexing
                ""
            });
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
}
