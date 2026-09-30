//! Captured syntax/configuration data. No IO, runtime evaluation or inferred targets.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportSyntax {
    #[default]
    Unspecified,
    Static,
    Require,
    Dynamic,
    TypeOnly,
    TypeOnlyRequire,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ImportContext {
    pub syntax: ImportSyntax,
    pub lexical_module: Vec<String>,
    pub conditions: Vec<String>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum PackageTarget {
    #[default]
    Absent,
    Null,
    Path(String),
    Array(Vec<PackageTarget>),
    Object(Vec<(String, PackageTarget)>),
    Invalid,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageConfig {
    pub name: Option<String>,
    pub package_type: Option<String>,
    pub exports: PackageTarget,
    pub imports: PackageTarget,
    pub main: Option<String>,
    pub types: Option<String>,
    pub workspaces: Vec<String>,
    pub dependencies: BTreeMap<String, String>,
    pub diagnostics: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ConfigDocument {
    TypeScript(crate::project_model::RawProjectConfig),
    Package(PackageConfig),
    Toml(serde_json::Value),
    Go(crate::go_project::GoConfig),
    Invalid(String),
}
impl ConfigDocument {
    pub fn typescript(self) -> Option<crate::project_model::RawProjectConfig> {
        if let Self::TypeScript(c) = self {
            Some(c)
        } else {
            None
        }
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RustModuleDecl {
    pub name: String,
    pub parent: Vec<String>,
    pub inline: bool,
    pub path: Option<String>,
    pub conditions: Vec<String>,
    pub unsupported: Option<String>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RustSourceFacts {
    pub modules: Vec<RustModuleDecl>,
    pub inner_conditions: Vec<String>,
    pub syntax_error: bool,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RustSourceInput {
    pub digest: String,
    pub facts: RustSourceFacts,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RustCrate {
    #[serde(default)]
    pub external_dependencies: BTreeSet<String>,
    pub blocked_dependencies: BTreeMap<String, String>,
    pub manifest: String,
    pub entry: String,
    pub name: String,
    pub conditions: BTreeSet<String>,
    pub dependencies: BTreeMap<String, String>,
    pub diagnostics: Vec<String>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RustModuleLocation {
    pub crate_manifest: String,
    pub logical_path: Vec<String>,
    pub file: String,
    pub directory: String,
    pub blocked: Option<String>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RustProject {
    #[serde(default)]
    pub by_entry: BTreeMap<String, Vec<String>>,
    pub by_file: BTreeMap<String, Vec<usize>>,
    pub by_module: BTreeMap<String, Vec<usize>>,
    pub crates: BTreeMap<String, RustCrate>,
    pub locations: Vec<RustModuleLocation>,
    pub diagnostics: Vec<String>,
}
impl RustProject {
    pub fn module_key(manifest: &str, path: &[String]) -> String {
        serde_json::to_string(&(manifest, path)).expect("module key")
    }
    pub fn index_locations(&mut self) {
        self.by_entry.clear();
        for (manifest, krate) in &self.crates {
            self.by_entry
                .entry(krate.entry.clone())
                .or_default()
                .push(manifest.clone());
        }
        self.by_file.clear();
        self.by_module.clear();
        for (i, l) in self.locations.iter().enumerate() {
            self.by_file.entry(l.file.clone()).or_default().push(i);
            self.by_module
                .entry(Self::module_key(&l.crate_manifest, &l.logical_path))
                .or_default()
                .push(i);
        }
    }
    pub fn in_file<'a>(&'a self, file: &str) -> impl Iterator<Item = &'a RustModuleLocation> + 'a {
        self.by_file
            .get(file)
            .into_iter()
            .flatten()
            .map(|&i| &self.locations[i])
    }
    pub fn at<'a>(
        &'a self,
        manifest: &str,
        path: &[String],
    ) -> impl Iterator<Item = &'a RustModuleLocation> + 'a {
        self.by_module
            .get(&Self::module_key(manifest, path))
            .into_iter()
            .flatten()
            .map(|&i| &self.locations[i])
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PythonProject {
    pub roots: BTreeMap<String, Vec<String>>,
    pub diagnostics: BTreeMap<String, Vec<String>>,
}
