//! Content-keyed, bounded configuration loading. No mtime-only cache reuse.
use cc_model::{project_model::*, CcError, CcResult};
use std::{collections::BTreeMap, path::Path};
pub(super) const MAX_CONFIG_BYTES: usize = 1024 * 1024;
const MAX_TOTAL_BYTES: usize = 16 * 1024 * 1024;
/// All components are checked; symlinked configuration inputs are deliberately
/// unsupported. The root itself may have a platform alias (/tmp on macOS).
pub(super) fn read_bytes(root: &Path, path: &str) -> Result<Option<Vec<u8>>, String> {
    // Discovery and watcher use this same policy, including generated/cache
    // exclusions. Never accept an input that the watcher is guaranteed to drop.
    if !super::potential_config_path(path) {
        return Err("unsupported_config_path".into());
    }
    cc_model::input_file::read(root, path, MAX_CONFIG_BYTES).map_err(|e| e.config_reason().into())
}
fn parse_typescript(bytes: &[u8]) -> CcResult<RawProjectConfig> {
    let text = std::str::from_utf8(bytes).map_err(|_| CcError::Config("config_not_utf8".into()))?;
    let v = super::jsonc::parse(text)?;
    let obj = v
        .as_object()
        .ok_or_else(|| CcError::Config("config_must_be_object".into()))?;
    let extends = match obj.get("extends") {
        None => vec![],
        Some(serde_json::Value::String(s)) => vec![s.clone()],
        Some(serde_json::Value::Array(a)) if a.len() <= 32 => a
            .iter()
            .map(|v| {
                v.as_str()
                    .filter(|s| !s.is_empty() && s.len() <= 4096)
                    .map(str::to_owned)
                    .ok_or_else(|| CcError::Config("invalid_extends_entry".into()))
            })
            .collect::<CcResult<_>>()?,
        _ => return Err(CcError::Config("invalid_extends".into())),
    };
    if extends.iter().any(|s| s.is_empty() || s.len() > 4096) {
        return Err(CcError::Config("invalid_extends_entry".into()));
    }
    let mut options = BTreeMap::new();
    if let Some(co) = obj.get("compilerOptions") {
        let co = co
            .as_object()
            .ok_or_else(|| CcError::Config("compilerOptions_must_be_object".into()))?;
        for key in [
            "baseUrl",
            "paths",
            "module",
            "moduleResolution",
            "customConditions",
            "resolveJsonModule",
            "rootDirs",
            "moduleSuffixes",
            "allowArbitraryExtensions",
            "noResolve",
        ] {
            if let Some(v) = co.get(key) {
                options.insert(key.to_string(), v.clone());
            }
        }
    }
    Ok(RawProjectConfig { extends, options })
}
fn parse(path: &str, bytes: &[u8]) -> CcResult<cc_model::module_inputs::ConfigDocument> {
    use cc_model::module_inputs::ConfigDocument;
    if path.rsplit('/').next() == Some("package.json") {
        return super::package::parse(bytes).map(ConfigDocument::Package);
    }
    if matches!(path.rsplit('/').next(), Some("go.mod" | "go.work")) {
        return super::go::parse(bytes, path.ends_with("go.work"));
    }
    if path.ends_with(".toml") {
        let text =
            std::str::from_utf8(bytes).map_err(|_| CcError::Config("config_not_utf8".into()))?;
        let v: toml::Value =
            toml::from_str(text).map_err(|_| CcError::Config("invalid_toml".into()))?;
        return Ok(ConfigDocument::Toml(serde_json::to_value(v)?));
    }
    parse_typescript(bytes).map(ConfigDocument::TypeScript)
}
pub(super) struct Loader<'a> {
    root: &'a Path,
    old: &'a BTreeMap<String, ConfigInput>,
    pub inputs: BTreeMap<String, ConfigInput>,
    pub reads: usize,
    pub hits: usize,
    bytes: usize,
}
impl<'a> Loader<'a> {
    pub fn new(root: &'a Path, old: &'a BTreeMap<String, ConfigInput>) -> Self {
        Self {
            root,
            old,
            inputs: BTreeMap::new(),
            reads: 0,
            hits: 0,
            bytes: 0,
        }
    }
    pub fn load(&mut self, path: &str) -> CcResult<ConfigInput> {
        if let Some(v) = self.inputs.get(path) {
            return Ok(v.clone());
        }
        if self.inputs.len() >= 1024 {
            return Err(CcError::Config("project_config_count_limit".into()));
        }
        let input = match read_bytes(self.root, path) {
            Ok(Some(bytes)) => {
                self.reads += 1;
                self.bytes = self.bytes.saturating_add(bytes.len());
                if self.bytes > MAX_TOTAL_BYTES {
                    return Err(CcError::Config("project_config_total_bytes_limit".into()));
                }
                let digest = blake3::hash(&bytes).to_hex().to_string();
                if let Some(cached) = self
                    .old
                    .get(path)
                    .filter(|c| c.digest.as_deref() == Some(&digest))
                {
                    self.hits += 1;
                    cached.clone()
                } else {
                    match parse(path, &bytes) {
                        Ok(parsed) => ConfigInput {
                            digest: Some(digest),
                            parsed: Some(parsed),
                            error: None,
                        },
                        Err(e) => ConfigInput {
                            digest: Some(digest),
                            parsed: None,
                            error: Some(e.to_string()),
                        },
                    }
                }
            }
            Ok(None) => ConfigInput {
                digest: None,
                parsed: None,
                error: Some("missing_config".into()),
            },
            Err(reason) => ConfigInput {
                digest: None,
                parsed: None,
                error: Some(reason),
            },
        };
        self.inputs.insert(path.to_owned(), input.clone());
        Ok(input)
    }
}
/// Revalidate captured inputs at publication, not on each import. Failure leaves
/// the prior acknowledgement unchanged so the next build replays the change.
pub(super) fn verify(root: &Path, inputs: &ProjectInputs) -> CcResult<()> {
    for (path, old) in &inputs.configs {
        let (digest, error) = match read_bytes(root, path) {
            Ok(Some(b)) => (Some(blake3::hash(&b).to_hex().to_string()), None),
            Ok(None) => (None, Some("missing_config".to_owned())),
            Err(e) => (None, Some(e)),
        };
        if digest != old.digest || (digest.is_none() && error != old.error) {
            return Err(CcError::Config(format!(
                "project configuration changed during prepare: {path}; retry index"
            )));
        }
    }
    Ok(())
}
