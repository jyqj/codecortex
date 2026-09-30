//! Static Go module inputs and package membership; no host build profile is guessed.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoReplace {
    pub module: String,
    pub version: Option<String>,
    pub target: String,
    pub target_version: Option<String>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoConfig {
    pub module: Option<String>,
    pub requires: BTreeMap<String, String>,
    pub uses: Vec<String>,
    pub replaces: Vec<GoReplace>,
    pub diagnostics: Vec<String>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoSourceFacts {
    pub package: Option<String>,
    pub conditions: Vec<String>,
    pub unknown: bool,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoSourceInput {
    pub digest: String,
    pub facts: GoSourceFacts,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoProject {
    pub configs: BTreeMap<String, GoConfig>,
    pub sources: BTreeMap<String, GoSourceFacts>,
    pub directories: BTreeMap<String, Vec<String>>,
}
