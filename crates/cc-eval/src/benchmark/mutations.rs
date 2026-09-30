//! Operations are applied only to disposable fixture copies, never the source repository.
use super::{invalid, validation::relative_path, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Mutation {
    Write { path: String, content: String },
    Delete { path: String },
    Rename { from: String, to: String },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MutationPlan {
    pub schema_version: u32,
    pub steps: Vec<Mutation>,
    pub probes: Vec<super::schema::SearchInput>,
}
pub(super) fn allowed(path: &str) -> Result<()> {
    relative_path(path)?;
    if path
        .split('/')
        .any(|p| p.starts_with(".codecortex") || p == ".git")
    {
        return Err(invalid("mutation cannot touch index or git internals"));
    }
    Ok(())
}
impl Mutation {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Write { path, content } => {
                allowed(path)?;
                if content.len() > 4 * 1024 * 1024 {
                    return Err(invalid("mutation source too large"));
                }
            }
            Self::Delete { path } => allowed(path)?,
            Self::Rename { from, to } => {
                allowed(from)?;
                allowed(to)?;
                if from == to {
                    return Err(invalid("rename source equals target"));
                }
            }
        }
        Ok(())
    }
    pub fn apply(&self, root: &Path) -> Result<()> {
        self.validate()?;
        match self {
            Self::Write { path, content } => {
                allowed(path)?;
                let dest = root.join(path);
                if let Some(p) = dest.parent() {
                    std::fs::create_dir_all(p)?;
                }
                std::fs::write(dest, content)?;
            }
            Self::Delete { path } => {
                allowed(path)?;
                std::fs::remove_file(root.join(path))?;
            }
            Self::Rename { from, to } => {
                allowed(from)?;
                allowed(to)?;
                let dest = root.join(to);
                if dest.exists() {
                    return Err(invalid("rename destination exists"));
                }
                if let Some(p) = dest.parent() {
                    std::fs::create_dir_all(p)?;
                }
                std::fs::rename(root.join(from), dest)?;
            }
        }
        Ok(())
    }
}
