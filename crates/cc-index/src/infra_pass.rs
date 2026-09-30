//! Independent infrastructure indexing pass.
//! Scans Dockerfile, docker-compose, K8s manifests — NOT part of the language parser pipeline.

use std::path::Path;

use cc_model::infra::{InfraEdge, InfraEdgeKind, InfraKind, InfraNode};
use cc_model::StableId;

// Re-export parser functions from sub-modules so the public API is unchanged.
pub use crate::infra_docker::{parse_docker_compose, parse_dockerfile};
pub use crate::infra_k8s::{parse_k8s_manifest, parse_kustomize};
pub use crate::infra_terraform::{parse_compile_commands, parse_terraform};

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

/// Name-only superset of [`classify_infra_file`]: `true` whenever a file of
/// this NAME could classify as an infra candidate under any content. Used by
/// scoped-build gating (`ScopeSignatureHints`) to prove an event set cannot
/// change the infra candidate set without the content sniff or a walk. Must
/// stay a superset: `classify_infra_file(name, _) == Some(_)` ⇒ `true`.
pub(crate) fn may_be_infra_candidate_name(file_name: &str) -> bool {
    file_name.starts_with("Dockerfile")
        || file_name.ends_with(".tf")
        || file_name == "compile_commands.json"
        || file_name.ends_with(".yaml")
        || file_name.ends_with(".yml")
}

/// Name-based strong-feature classification shared by the walk and manifest
/// discovery paths. `path` is only read for the K8s content sniff.
fn classify_infra_file(file_name: &str, path: &Path) -> Option<InfraFileType> {
    if file_name.starts_with("Dockerfile") {
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
    } else if (file_name.ends_with(".yaml") || file_name.ends_with(".yml")) && is_k8s_manifest(path)
    {
        // Generic YAML with apiVersion + kind — likely K8s manifest
        Some(InfraFileType::K8sManifest)
    } else {
        None
    }
}

/// Discover infra files using strong-feature filtering (not blanket *.yaml).
///
/// Fallback path for builds without a shared walk manifest; manifest-carrying
/// builds use [`discover_infra_files_from_manifest`] and skip this walk.
pub fn discover_infra_files(project_path: &Path) -> Vec<InfraCandidate> {
    let mut candidates = Vec::new();

    // Use ignore crate (same as Scanner) to respect .gitignore
    let walker = ignore::WalkBuilder::new(project_path)
        .hidden(true)
        .git_ignore(true)
        .git_global(false)
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
            Ok(r) => r.to_string_lossy().replace('\\', "/"),
            Err(_) => continue,
        };

        if let Some(ft) = classify_infra_file(file_name, path) {
            candidates.push(InfraCandidate {
                rel_path,
                abs_path: path.to_path_buf(),
                file_type: ft,
            });
        }
    }
    candidates
}

/// Discover infra files from the shared walk manifest (no additional walk).
/// Hidden paths are excluded, matching the walk-based discovery's
/// `hidden(true)`. Candidates are sorted by path (the signature contract).
pub fn discover_infra_files_from_manifest(
    project_path: &Path,
    manifest: &crate::scanner::WalkManifest,
) -> Vec<InfraCandidate> {
    let mut candidates = Vec::new();
    for file in &manifest.files {
        if file.hidden {
            continue;
        }
        let file_name = file.rel_path.rsplit('/').next().unwrap_or(&file.rel_path);
        let abs_path = project_path.join(&file.rel_path);
        if let Some(ft) = classify_infra_file(file_name, &abs_path) {
            candidates.push(InfraCandidate {
                rel_path: file.rel_path.clone(),
                abs_path,
                file_type: ft,
            });
        }
    }
    candidates.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
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

/// Deterministic signature over the infrastructure file inputs: the sorted
/// set of discovered infra candidate paths, each tagged with its mtime and
/// size. Adding, removing, or modifying any infra file (Dockerfile, compose,
/// K8s manifest, terraform, compile_commands) changes the signature and
/// forces the infra pass to recompute.
///
/// This only walks the tree and stats files (the `discover_infra_files`
/// strong-feature filter), so it is strictly cheaper than the full pass
/// (read + parse + symbol bind + route match + `replace_infra_data`) it
/// gates. mtime + size matches the change-detection contract used by the
/// scan/diff fast path.
pub(crate) fn infra_signature(project_path: &Path) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut candidates = discover_infra_files(project_path);
    candidates.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));

    let mut hasher = DefaultHasher::new();
    candidates.len().hash(&mut hasher);
    for candidate in &candidates {
        candidate.rel_path.hash(&mut hasher);
        if let Ok(metadata) = std::fs::metadata(&candidate.abs_path) {
            metadata.len().hash(&mut hasher);
            let mtime = metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            mtime.hash(&mut hasher);
        }
    }
    hasher.finish()
}

/// Same signature contract as [`infra_signature`], computed from the shared
/// walk manifest — must stay value-equal for the same tree so the gate never
/// flaps between manifest-carrying and fallback builds.
pub(crate) fn infra_signature_from_manifest(
    project_path: &Path,
    manifest: &crate::scanner::WalkManifest,
) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let candidates = discover_infra_files_from_manifest(project_path, manifest);
    let stat_by_rel: std::collections::HashMap<&str, (u64, u64)> = manifest
        .files
        .iter()
        .map(|f| (f.rel_path.as_str(), (f.size, f.mtime_secs)))
        .collect();

    let mut hasher = DefaultHasher::new();
    candidates.len().hash(&mut hasher);
    for candidate in &candidates {
        candidate.rel_path.hash(&mut hasher);
        if let Some((size, mtime_secs)) = stat_by_rel.get(candidate.rel_path.as_str()) {
            size.hash(&mut hasher);
            mtime_secs.hash(&mut hasher);
        }
    }
    hasher.finish()
}

/// Run the full infra pass: discover -> parse -> return nodes + edges.
///
/// Fallback path for builds without a shared walk manifest.
pub fn run_infra_pass(project_path: &Path) -> (Vec<InfraNode>, Vec<InfraEdge>) {
    let candidates = discover_infra_files(project_path);
    let (mut all_nodes, mut all_edges) = parse_infra_candidates(&candidates);

    // Scan all YAML/JSON config files for topic/queue → endpoint bindings
    extract_infra_bindings(project_path, &mut all_nodes, &mut all_edges);

    (all_nodes, all_edges)
}

/// Run the full infra pass from the shared walk manifest (no additional
/// walks: candidates and binding-scan files both come from the manifest).
pub fn run_infra_pass_with_manifest(
    project_path: &Path,
    manifest: &crate::scanner::WalkManifest,
) -> (Vec<InfraNode>, Vec<InfraEdge>) {
    let candidates = discover_infra_files_from_manifest(project_path, manifest);
    let (mut all_nodes, mut all_edges) = parse_infra_candidates(&candidates);

    let binding_files = manifest.files.iter().filter_map(|f| {
        if f.hidden {
            return None;
        }
        binding_scan_rel_filter(&f.rel_path)
            .then(|| (f.rel_path.clone(), project_path.join(&f.rel_path)))
    });
    extract_infra_bindings_from_files(binding_files, &mut all_nodes, &mut all_edges);

    (all_nodes, all_edges)
}

/// Parse each discovered candidate into infra nodes/edges.
fn parse_infra_candidates(candidates: &[InfraCandidate]) -> (Vec<InfraNode>, Vec<InfraEdge>) {
    let mut all_nodes = Vec::new();
    let mut all_edges = Vec::new();

    for candidate in candidates {
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

/// Rel-path filter for the binding scan: YAML/JSON files outside directories
/// that are unlikely to contain IaC config.
fn binding_scan_rel_filter(rel_path: &str) -> bool {
    let ext = Path::new(rel_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    matches!(ext, "yaml" | "yml" | "json")
        && !rel_path.contains("node_modules")
        && !rel_path.contains("vendor")
        && !rel_path.contains(".git/")
}

/// Scan YAML config files for topic/queue → endpoint bindings and produce
/// `InfraNode` + `InfraEdge` entries.
///
/// Fallback path for builds without a shared walk manifest.
fn extract_infra_bindings(
    project_path: &Path,
    infra_nodes: &mut Vec<InfraNode>,
    infra_edges: &mut Vec<InfraEdge>,
) {
    let walker = ignore::WalkBuilder::new(project_path)
        .hidden(true)
        .git_ignore(true)
        .git_global(false)
        .build();

    let files = walker.flatten().filter_map(|entry| {
        let path = entry.path();
        if !path.is_file() {
            return None;
        }
        let rel_path = path
            .strip_prefix(project_path)
            .ok()?
            .to_string_lossy()
            .replace('\\', "/");
        binding_scan_rel_filter(&rel_path).then(|| (rel_path, path.to_path_buf()))
    });
    extract_infra_bindings_from_files(files, infra_nodes, infra_edges);
}

/// Shared binding-extraction core over pre-filtered candidate files.
fn extract_infra_bindings_from_files(
    files: impl Iterator<Item = (String, std::path::PathBuf)>,
    infra_nodes: &mut Vec<InfraNode>,
    infra_edges: &mut Vec<InfraEdge>,
) {
    for (rel_path, abs_path) in files {
        let content = match std::fs::read_to_string(&abs_path) {
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
///
/// Made `pub(crate)` so sub-modules (`infra_k8s`) can reuse it.
pub(crate) fn parse_yaml_kv(trimmed: &str) -> Option<(String, String)> {
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
///
/// Made `pub(crate)` so sub-modules (`infra_k8s`) can reuse it.
pub(crate) fn strip_quotes(value: &str) -> String {
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
}
