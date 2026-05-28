//! Terraform and compile_commands.json parsing for the infrastructure pass.

use cc_model::infra::{InfraEdge, InfraEdgeKind, InfraKind, InfraNode};
use cc_model::StableId;
use std::sync::LazyLock;

static RE_RESOURCE_DATA: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r#"^(resource|data)\s+"(\w+)"\s+"(\w+)""#)
        .expect("valid Terraform resource/data regex")
});
static RE_VAR_OUTPUT: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r#"^(variable|output)\s+"(\w+)""#)
        .expect("valid Terraform variable/output regex")
});
static RE_MODULE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r#"^module\s+"(\w+)""#).expect("valid Terraform module regex")
});
static RE_SOURCE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r#"source\s*=\s*"([^"]+)""#).expect("valid Terraform source regex")
});
static RE_VAR_REF: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r#"var\.(\w+)"#).expect("valid Terraform variable reference regex")
});

/// Parse Terraform `.tf` files into InfraNodes + InfraEdges.
///
/// Supports: resource, data, variable, output, module blocks.
/// Also extracts `var.X` references and creates `References` edges.
pub fn parse_terraform(file_path: &str, content: &str) -> (Vec<InfraNode>, Vec<InfraEdge>) {
    let mut nodes = Vec::new();
    let mut edges = Vec::new();

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
        if let Some(caps) = RE_RESOURCE_DATA.captures(trimmed) {
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
        if let Some(caps) = RE_VAR_OUTPUT.captures(trimmed) {
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
        if let Some(caps) = RE_MODULE.captures(trimmed) {
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
            if let Some(caps) = RE_SOURCE.captures(trimmed) {
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
            for caps in RE_VAR_REF.captures_iter(trimmed) {
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

#[cfg(test)]
mod tests {
    use super::*;

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
