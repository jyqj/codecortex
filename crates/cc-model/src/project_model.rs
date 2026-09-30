//! Immutable, admitted-file project inputs. Resolution never consults the disk.
use crate::{repo_path, CcError, CcResult};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
pub const PROJECT_MODEL_VERSION: u32 = 3;
pub const PROJECT_INPUT_KEY: &str = "project_model_inputs_v1";
pub const MODULE_BLOCKED_BINDING: &str = "module_unavailable_binding";
/// Importers using JS/TS rules, including script blocks already extracted from SFCs.
/// This does not certify framework transforms or arbitrary component extensions.
pub fn is_jsts(path: &str) -> bool {
    path.rsplit('.').next().is_some_and(|e| {
        matches!(
            e,
            "ts" | "tsx" | "js" | "jsx" | "mts" | "cts" | "mjs" | "cjs" | "vue" | "svelte"
        )
    })
}
pub fn is_config_root(path: &str) -> bool {
    matches!(
        path.rsplit('/').next(),
        Some("tsconfig.json" | "jsconfig.json")
    )
}
/// Repository-relative lexical join. A directory may be the empty root.
pub fn join_relative(directory: &str, path: &str) -> Option<String> {
    if path.is_empty()
        || path.len() > 4096
        || path.starts_with('/')
        || path.contains(['\\', ':', '\0'])
    {
        return None;
    }
    let mut parts: Vec<&str> = directory.split('/').filter(|p| !p.is_empty()).collect();
    for p in path.split('/') {
        match p {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            _ => parts.push(p),
        }
    }
    if parts
        .iter()
        .any(|p| p.starts_with(".codecortex") || *p == ".git" || p.chars().any(char::is_control))
    {
        return None;
    }
    Some(parts.join("/"))
}
pub fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(p, _)| p)
}
#[derive(Debug, Clone, Serialize)]
pub struct FileCatalog {
    files: BTreeSet<String>,
}
impl FileCatalog {
    pub fn new(files: BTreeSet<String>) -> CcResult<Self> {
        if files.iter().any(|p| !repo_path::is_canonical_file(p)) {
            return Err(CcError::Config("non-canonical project file catalog".into()));
        }
        Ok(Self { files })
    }
    pub fn contains(&self, path: &str) -> bool {
        self.files.contains(path)
    }
    pub fn files(&self) -> &BTreeSet<String> {
        &self.files
    }
}
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawProjectConfig {
    pub extends: Vec<String>,
    pub options: BTreeMap<String, serde_json::Value>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigInput {
    pub digest: Option<String>,
    pub parsed: Option<crate::module_inputs::ConfigDocument>,
    pub error: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectInputs {
    pub version: u32,
    pub roots: BTreeSet<String>,
    pub configs: BTreeMap<String, ConfigInput>,
    #[serde(default)]
    pub rust_sources: BTreeMap<String, crate::module_inputs::RustSourceInput>,
    #[serde(default)]
    pub go_sources: BTreeMap<String, crate::go_project::GoSourceInput>,
    pub package_roots: BTreeMap<String, String>,
}
impl Default for ProjectInputs {
    fn default() -> Self {
        Self {
            version: PROJECT_MODEL_VERSION,
            roots: BTreeSet::new(),
            configs: BTreeMap::new(),
            rust_sources: BTreeMap::new(),
            go_sources: BTreeMap::new(),
            package_roots: BTreeMap::new(),
        }
    }
}
impl ProjectInputs {
    pub fn validate(&self) -> CcResult<()> {
        if self.version != PROJECT_MODEL_VERSION
            || self.configs.len() > 1024
            || self.roots.len() > 1024
            || self.package_roots.len() > 8192
            || self.go_sources.len() > 100000
            || self.go_sources.iter().any(|(p, s)| {
                !repo_path::is_canonical_file(p)
                    || !p.ends_with(".go")
                    || (s.digest.len() != 64 || !s.digest.bytes().all(|b| b.is_ascii_hexdigit()))
            })
            || self.rust_sources.len() > 100000
            || self.rust_sources.iter().any(|(p, s)| {
                !repo_path::is_canonical_file(p)
                    || !p.ends_with(".rs")
                    || (s.digest.len() != 64 || !s.digest.bytes().all(|b| b.is_ascii_hexdigit()))
            })
            || self
                .configs
                .keys()
                .chain(self.roots.iter())
                .chain(self.package_roots.keys())
                .any(|p| !repo_path::is_canonical_file(p))
            || self.configs.values().any(|c| {
                c.digest
                    .as_ref()
                    .is_some_and(|s| s.len() != 64 || !s.bytes().all(|b| b.is_ascii_hexdigit()))
                    || (c.parsed.is_some() && (c.digest.is_none() || c.error.is_some()))
            })
        {
            return Err(CcError::Config(
                "invalid project input snapshot; full rebuild required".into(),
            ));
        }
        Ok(())
    }
    pub fn payload(&self) -> CcResult<String> {
        self.validate()?;
        let text = serde_json::to_string(self)?;
        if text.len() > 16 * 1024 * 1024 {
            return Err(CcError::Config(
                "project input snapshot exceeds 16 MiB".into(),
            ));
        }
        Ok(text)
    }
    pub fn digest(&self) -> CcResult<String> {
        Ok(blake3::hash(self.payload()?.as_bytes())
            .to_hex()
            .to_string())
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ConfigDiagnostic {
    pub path: String,
    pub reason: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnchoredPaths {
    pub defined_in: String,
    pub patterns: BTreeMap<String, Vec<String>>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypeScriptConfig {
    pub path: String,
    pub dependencies: BTreeSet<String>,
    pub base_url: Option<String>,
    pub paths: Option<AnchoredPaths>,
    pub module_resolution: String,
    pub conditions: Vec<String>,
    pub resolve_json_module: bool,
    pub diagnostics: Vec<ConfigDiagnostic>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModuleStatus {
    Resolved,
    Unresolved,
    Unsupported,
    External,
    Ambiguous,
    Unknown,
}
/// A package is a set, never an arbitrarily chosen representative source file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedPackage {
    pub directory: String,
    pub name: String,
    pub files: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModuleResolution {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub request_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_package: Option<ResolvedPackage>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidates: Vec<String>,
    pub import_string: String,
    pub resolved_path: Option<String>,
    pub status: ModuleStatus,
    pub strategy: String,
    pub module_resolution: String,
    pub conditions: Vec<String>,
    pub config_dependencies: BTreeSet<String>,
    pub probes: Vec<String>,
    pub reason: Option<String>,
}
impl ModuleResolution {
    pub fn valid_target_shape(&self) -> bool {
        let file = self.resolved_path.is_some();
        let package = self.resolved_package.is_some();
        if (self.status == ModuleStatus::Resolved) != (file || package)
            || (file && package)
            || self.candidates.len() > 4096
        {
            return false;
        }
        if self.resolved_package.as_ref().is_some_and(|p| {
            p.name.is_empty()
                || p.files.is_empty()
                || p.files.len() > 4096
                || p.files.windows(2).any(|w| w[0] >= w[1])
                || p.files
                    .iter()
                    .any(|f| !repo_path::is_canonical_file(f) || parent(f) != p.directory)
        }) {
            return false;
        }
        true
    }
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectModelReport {
    #[serde(default)]
    pub rust_source_reads: usize,
    #[serde(default)]
    pub rust_fact_cache_hits: usize,
    #[serde(default)]
    pub go_source_reads: usize,
    #[serde(default)]
    pub go_fact_cache_hits: usize,
    pub version: u32,
    pub input_digest: String,
    pub catalog_files: usize,
    pub config_roots: usize,
    pub config_inputs: usize,
    pub config_reads: usize,
    pub config_root_probes: usize,
    pub config_parse_cache_hits: usize,
    pub inventory_source: String,
    pub changed_configs: Vec<String>,
    pub modes: BTreeSet<String>,
    pub diagnostics: Vec<ConfigDiagnostic>,
}
#[derive(Debug, Clone, Serialize)]
pub struct ProjectModel {
    manifests: BTreeMap<String, crate::module_inputs::ConfigDocument>,
    package_names: BTreeMap<String, Vec<String>>,
    workspace_groups: BTreeMap<String, BTreeSet<String>>,
    rust: crate::module_inputs::RustProject,
    python: crate::module_inputs::PythonProject,
    go: crate::go_project::GoProject,
    files: FileCatalog,
    typescript: BTreeMap<String, TypeScriptConfig>,
    package_roots: BTreeMap<String, String>,
    rust_aliases: std::collections::HashMap<String, String>,
}
impl ProjectModel {
    pub fn new(
        files: FileCatalog,
        typescript: BTreeMap<String, TypeScriptConfig>,
        package_roots: BTreeMap<String, String>,
        rust_aliases: std::collections::HashMap<String, String>,
    ) -> Self {
        Self {
            files,
            typescript,
            package_roots,
            rust_aliases,
            manifests: Default::default(),
            package_names: Default::default(),
            workspace_groups: Default::default(),
            rust: Default::default(),
            python: Default::default(),
            go: Default::default(),
        }
    }
    pub fn with_module_inputs(
        mut self,
        manifests: BTreeMap<String, crate::module_inputs::ConfigDocument>,
        rust: crate::module_inputs::RustProject,
        python: crate::module_inputs::PythonProject,
    ) -> Self {
        for (path, doc) in &manifests {
            if let crate::module_inputs::ConfigDocument::Package(p) = doc {
                if let Some(name) = &p.name {
                    self.package_names
                        .entry(name.clone())
                        .or_default()
                        .push(path.clone());
                }
            }
        }
        self.manifests = manifests;
        self.rust = rust;
        self.python = python;
        self
    }
    pub fn with_workspaces(mut self, groups: BTreeMap<String, BTreeSet<String>>) -> Self {
        self.workspace_groups = groups;
        self
    }
    pub fn with_go(mut self, go: crate::go_project::GoProject) -> Self {
        self.go = go;
        self
    }
    pub fn go(&self) -> &crate::go_project::GoProject {
        &self.go
    }
    /// Probe ancestor package roots, not every unrelated workspace per import.
    pub fn nearest_workspace(&self, file: &str) -> Option<(&String, &BTreeSet<String>)> {
        let mut directory = parent(file);
        loop {
            let key = if directory.is_empty() {
                "package.json".to_owned()
            } else {
                format!("{directory}/package.json")
            };
            if let Some(pair) = self.workspace_groups.get_key_value(&key) {
                return Some(pair);
            }
            if directory.is_empty() {
                return None;
            }
            directory = parent(directory);
        }
    }
    pub fn workspace_groups(&self) -> &BTreeMap<String, BTreeSet<String>> {
        &self.workspace_groups
    }
    pub fn packages_named(&self, name: &str) -> &[String] {
        self.package_names.get(name).map_or(&[], Vec::as_slice)
    }
    pub fn manifests(&self) -> &BTreeMap<String, crate::module_inputs::ConfigDocument> {
        &self.manifests
    }
    pub fn rust(&self) -> &crate::module_inputs::RustProject {
        &self.rust
    }
    pub fn python(&self) -> &crate::module_inputs::PythonProject {
        &self.python
    }
    pub fn files(&self) -> &FileCatalog {
        &self.files
    }
    pub fn configs(&self) -> &BTreeMap<String, TypeScriptConfig> {
        &self.typescript
    }
    pub fn rust_aliases(&self) -> &std::collections::HashMap<String, String> {
        &self.rust_aliases
    }
    /// Nearest-directory ownership is CodeCortex policy, not tsc's complete
    /// project file-selection (files/include/references) algorithm.
    pub fn nearest_config(&self, file: &str) -> Option<&TypeScriptConfig> {
        let mut dir = parent(file);
        loop {
            for name in ["tsconfig.json", "jsconfig.json"] {
                let p = if dir.is_empty() {
                    name.to_string()
                } else {
                    format!("{dir}/{name}")
                };
                if let Some(c) = self.typescript.get(&p) {
                    return Some(c);
                }
            }
            if dir.is_empty() {
                return None;
            }
            dir = parent(dir);
        }
    }
}
