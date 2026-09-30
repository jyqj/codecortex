//! Package evidence derived from file-local surfaces; no second parser/store.
use crate::public_surface::{PublicSurface, SurfaceKnowledge};
use crate::{CcError, CcResult};
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageKey {
    pub directory: String,
    pub name: String,
    pub test_files: bool,
}
impl PackageKey {
    pub fn from_surface(s: &PublicSurface) -> Option<Self> {
        if s.language != "go" {
            return None;
        }
        let mut declarations = s.entries.iter().filter(|e| e.kind == "go_package");
        let package = declarations.next()?;
        if declarations.next().is_some() || package.qualified_name.is_empty() {
            return None;
        }
        Some(Self {
            directory: s
                .module
                .rsplit_once('/')
                .map(|(d, _)| d)
                .unwrap_or("")
                .into(),
            name: package.qualified_name.clone(),
            test_files: s.module.ends_with("_test.go"),
        })
    }
    /// Internal tests see production files of the same package, never vice versa.
    pub fn contribution_keys(&self) -> Vec<Self> {
        let mut keys = vec![self.clone()];
        if self.test_files {
            let mut production = self.clone();
            production.test_files = false;
            keys.push(production);
        }
        keys
    }
    pub fn storage_key(&self) -> String {
        serde_json::to_string(self).expect("package key serialization")
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageSurface {
    pub key: PackageKey,
    pub members: Vec<String>,
    pub knowledge: SurfaceKnowledge,
    pub fingerprint: Option<String>,
}
impl PackageSurface {
    /// Fixed build membership is the caller's input. Conditional files remain
    /// Unknown, and production/test packages are never silently merged.
    pub fn combine(key: PackageKey, surfaces: &[PublicSurface]) -> CcResult<Self> {
        let mut parts = std::collections::BTreeMap::new();
        let mut unknown = false;
        for s in surfaces {
            if !key.contribution_keys().contains(
                &PackageKey::from_surface(s)
                    .ok_or_else(|| CcError::InvalidParams("missing package identity".into()))?,
            ) {
                return Err(CcError::InvalidParams("package membership mismatch".into()));
            }
            s.validate()?;
            if parts.insert(s.module.clone(), s.fingerprint()).is_some() {
                return Err(CcError::InvalidParams("duplicate package member".into()));
            }
            unknown |= s.fingerprint().is_none();
        }
        let encoded =
            serde_json::to_vec(&(1, key.storage_key(), &parts)).expect("package encoding");
        Ok(Self {
            key,
            members: parts.keys().cloned().collect(),
            knowledge: if unknown {
                SurfaceKnowledge::Unknown
            } else if parts.is_empty() {
                SurfaceKnowledge::KnownEmpty
            } else {
                SurfaceKnowledge::Known
            },
            fingerprint: (!unknown).then(|| format!("pkg1:{}", blake3::hash(&encoded).to_hex())),
        })
    }
}
