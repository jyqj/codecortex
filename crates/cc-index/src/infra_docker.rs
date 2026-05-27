//! Docker / docker-compose parsing for the infrastructure pass.

use cc_model::infra::{InfraEdge, InfraEdgeKind, InfraKind, InfraNode};
use cc_model::StableId;

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
