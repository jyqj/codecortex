//! Opt-in, complete Linux inventory and Python declaration admission.
//! No scanner filters, persistence, interpreter execution or atomic snapshot claim.
use super::{config_cache::Loader, python};
use cc_model::{declaration_identity::*, module_inputs::*, project_model::ConfigInput};
use cc_parsers::python_identity::*;
use std::{collections::BTreeMap, path::Path};

/// Authorization is an explicit assertion by the caller, never inferred from config.
#[derive(Debug)]
pub enum AuthorizedScope {
    EntireProject,
    /// Deliberately refused: omitted siblings can change markers or collisions.
    Subtree(String),
}
#[derive(Debug)]
pub struct CaptureRequest<'a> {
    pub owner: &'a str,
    pub scope: AuthorizedScope,
    /// Public contract: only an empty exclusion list is supported. Ignore files
    /// and scanner defaults do not exclude anything from this inventory.
    pub exclusions: &'a [String],
}
#[derive(Debug, Clone, Copy)]
pub struct CaptureLimits {
    /// Counts directories as well as regular files, including the root.
    pub entries: usize,
    pub files: usize,
    pub total_bytes: usize,
    pub file_bytes: usize,
    /// Root depth is zero; immediate children have depth one.
    pub depth: usize,
    pub path_bytes: usize,
    pub total_path_bytes: usize,
    pub output_count: usize,
    /// Aggregate serialized outcomes, including unavailable outcomes.
    pub output_bytes: usize,
}
#[derive(Debug, Clone, Copy)]
pub struct AdmissionPolicies {
    pub capture: CaptureLimits,
    pub ast: PythonIdentityLimits,
    pub declarations: DeclarationLimits,
}
#[derive(Debug)]
pub enum CaptureRefusal {
    UnsupportedPlatform,
    UnauthorizedScope,
    InvalidOwner,
    Path,
    Alias,
    DuplicateKey,
    SymlinkOrNonregular,
    Io,
    Drift,
    Budget(&'static str),
    Configuration,
    Ast {
        path: String,
        reason: PythonIdentityReason,
    },
    Parser(cc_model::CcError),
    Model(DeclarationError),
}
type Result<T> = std::result::Result<T, CaptureRefusal>;

/// Private construction keeps actual bytes, raw configuration and provenance together.
/// Returned bytes are immutable; serialized outputs remain assertions requiring rederivation.
#[derive(Debug)]
pub struct CapturedDeclarations {
    owner: String,
    files: BTreeMap<String, Vec<u8>>,
    config: ConfigInput,
    provenance: PythonRootProvenance,
    outcomes: BTreeMap<String, Vec<IdentityOutcome>>,
}
impl CapturedDeclarations {
    pub fn owner(&self) -> &str {
        &self.owner
    }
    pub fn scope(&self) -> AuthorizedScope {
        AuthorizedScope::EntireProject
    }
    pub fn inventory(&self) -> &BTreeMap<String, Vec<u8>> {
        &self.files
    }
    pub fn config(&self) -> &ConfigInput {
        &self.config
    }
    pub fn provenance(&self) -> &PythonRootProvenance {
        &self.provenance
    }
    pub fn outcomes(&self) -> &BTreeMap<String, Vec<IdentityOutcome>> {
        &self.outcomes
    }
}
fn bound(actual: usize, limit: usize, resource: &'static str) -> Result<()> {
    if actual > limit {
        Err(CaptureRefusal::Budget(resource))
    } else {
        Ok(())
    }
}
fn sum(a: usize, b: usize, resource: &'static str) -> Result<usize> {
    a.checked_add(b).ok_or(CaptureRefusal::Budget(resource))
}

/// Captures all regular files before parsing; verifies the same inventory and all
/// bytes after admission. Linux only. Canonical aliases of the trusted project
/// root are accepted; aliases below it and all multiply-linked files are refused.
pub fn capture_python_declarations(
    root: &Path,
    request: CaptureRequest<'_>,
    policies: AdmissionPolicies,
) -> Result<CapturedDeclarations> {
    capture_inner(root, request, policies, || {})
}
fn capture_inner(
    root: &Path,
    request: CaptureRequest<'_>,
    policies: AdmissionPolicies,
    after_capture: impl FnOnce(),
) -> Result<CapturedDeclarations> {
    if !matches!(request.scope, AuthorizedScope::EntireProject) || !request.exclusions.is_empty() {
        return Err(CaptureRefusal::UnauthorizedScope);
    }
    if request.owner.is_empty()
        || request.owner.len() > 4096
        || request.owner.chars().any(char::is_control)
    {
        return Err(CaptureRefusal::InvalidOwner);
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (root, policies, after_capture);
        Err(CaptureRefusal::UnsupportedPlatform)
    }
    #[cfg(target_os = "linux")]
    {
        let anchor = root.canonicalize().map_err(|_| CaptureRefusal::Io)?;
        let first = native::inventory(&anchor, policies.capture)?;
        // Complete size admission precedes content allocation/hashing and Loader.
        policies
            .declarations
            .check_inventory_sizes(first.files.len(), first.files.values().map(|s| s.size))
            .map_err(CaptureRefusal::Model)?;
        let mut files = BTreeMap::new();
        for (path, stamp) in &first.files {
            let bytes = cc_model::input_file::read(&anchor, path, policies.capture.file_bytes)
                .map_err(|_| CaptureRefusal::Drift)?
                .ok_or(CaptureRefusal::Drift)?;
            if bytes.len() != stamp.size {
                return Err(CaptureRefusal::Drift);
            }
            files.insert(path.clone(), bytes);
        }
        after_capture();
        // Reuse the actual bounded config Loader and provenance adapter, with no
        // arbitrary cache input, secondary TOML parser or caller-supplied evidence.
        if !files.contains_key("pyproject.toml")
            || files
                .keys()
                .any(|p| p != "pyproject.toml" && p.ends_with("pyproject.toml"))
        {
            return Err(CaptureRefusal::Configuration);
        }
        let old = BTreeMap::new();
        let mut loader = Loader::new(&anchor, &old);
        let config = loader
            .load("pyproject.toml")
            .map_err(CaptureRefusal::Parser)?;
        if config.digest.as_deref() != Some(content_digest(&files["pyproject.toml"]).as_str()) {
            return Err(CaptureRefusal::Drift);
        }
        if config.error.is_some() {
            return Err(CaptureRefusal::Configuration);
        }
        let doc = config
            .parsed
            .as_ref()
            .ok_or(CaptureRefusal::Configuration)?;
        supported_document(doc)?;
        let project = python::build(
            &BTreeMap::from([("pyproject.toml".into(), doc.clone())]),
            &loader.inputs,
        );
        let provenance = project
            .provenance
            .get("")
            .ok_or(CaptureRefusal::Configuration)?;
        if provenance.state != PythonRootState::Explicit
            || !provenance.limitations.is_empty()
            || provenance.roots.len() != 1
        {
            return Err(CaptureRefusal::Configuration);
        }
        let (directory, evidence) = provenance
            .roots
            .iter()
            .next()
            .ok_or(CaptureRefusal::Configuration)?;
        if !first.dirs.contains_key(directory) {
            return Err(CaptureRefusal::Configuration);
        }
        if evidence.is_empty()
            || evidence
                .iter()
                .any(|e| !matches!(e, PythonRootEvidence::Explicit { .. }))
        {
            return Err(CaptureRefusal::Configuration);
        }
        // Preserve every directive and original value, including normalized duplicates.
        let directive =
            serde_json::to_string(evidence).map_err(|_| CaptureRefusal::Configuration)?;
        let configured = ConfiguredRoot {
            directory: directory.clone(),
            config_path: "pyproject.toml".into(),
            config_digest: config.digest.clone().ok_or(CaptureRefusal::Configuration)?,
            directive,
        };
        let snapshot = DeclarationSnapshot::with_limits(
            request.owner.into(),
            &files,
            vec![configured],
            policies.declarations,
        )
        .map_err(CaptureRefusal::Model)?;
        let mut outcomes = BTreeMap::new();
        let mut count = 0;
        let mut text = 2;
        bound(text, policies.capture.output_bytes, "output_bytes")?;
        for (path, bytes) in files.iter().filter(|(p, _)| p.ends_with(".py")) {
            let inputs = match declaration_inputs(path, bytes, policies.ast)
                .map_err(CaptureRefusal::Parser)?
            {
                PythonIdentityOutcome::Inputs(v) => v,
                PythonIdentityOutcome::Unavailable(reason) => {
                    return Err(CaptureRefusal::Ast {
                        path: path.clone(),
                        reason,
                    })
                }
            };
            count = sum(count, inputs.len(), "output_count")?;
            bound(count, policies.capture.output_count, "output_count")?;
            let mut meter = OutputMeter {
                bytes: text,
                limit: policies.capture.output_bytes,
            };
            serde_json::to_writer(&mut meter, path)
                .map_err(|_| CaptureRefusal::Budget("output_bytes"))?;
            // Colon, array delimiters and the preceding map separator.
            text = sum(
                meter.bytes,
                3 + usize::from(!outcomes.is_empty()),
                "output_bytes",
            )?;
            bound(text, policies.capture.output_bytes, "output_bytes")?;
            let mut values = Vec::new();
            for input in &inputs {
                let outcome = snapshot
                    .resolve_with_limits(input)
                    .map_err(CaptureRefusal::Model)?;
                // A single temporary outcome is bounded by explicit model limits;
                // count serialized bytes without allocating a second text copy.
                text = sum(text, usize::from(!values.is_empty()), "output_bytes")?;
                let mut meter = OutputMeter {
                    bytes: text,
                    limit: policies.capture.output_bytes,
                };
                serde_json::to_writer(&mut meter, &outcome)
                    .map_err(|_| CaptureRefusal::Budget("output_bytes"))?;
                text = meter.bytes;
                values.push(outcome);
            }
            outcomes.insert(path.clone(), values);
        }
        native::verify(&anchor, root, &first, &files, policies.capture)?;
        Ok(CapturedDeclarations {
            owner: request.owner.into(),
            files,
            config,
            provenance: provenance.clone(),
            outcomes,
        })
    }
}
// Only complete collection-root directives are supported. Selectors, include/
// exclude, namespace switches, Poetry package selectors and other setuptools
// settings are refused, even when the legacy provenance has no limitation.
fn supported_document(doc: &ConfigDocument) -> Result<()> {
    let ConfigDocument::Toml(v) = doc else {
        return Err(CaptureRefusal::Configuration);
    };
    if v.pointer("/tool/poetry").is_some() {
        return Err(CaptureRefusal::Configuration);
    }
    if let Some(s) = v.pointer("/tool/setuptools").and_then(|v| v.as_object()) {
        if s.keys()
            .any(|k| !matches!(k.as_str(), "package-dir" | "packages"))
        {
            return Err(CaptureRefusal::Configuration);
        }
    }
    if let Some(p) = v
        .pointer("/tool/setuptools/packages")
        .and_then(|v| v.as_object())
    {
        if p.keys().any(|k| k != "find") {
            return Err(CaptureRefusal::Configuration);
        }
    }
    if let Some(p) = v
        .pointer("/tool/setuptools/packages/find")
        .and_then(|v| v.as_object())
    {
        if p.keys().any(|k| k != "where") {
            return Err(CaptureRefusal::Configuration);
        }
    }
    Ok(())
}
struct OutputMeter {
    bytes: usize,
    limit: usize,
}
impl std::io::Write for OutputMeter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self
            .bytes
            .checked_add(buf.len())
            .filter(|n| *n <= self.limit)
            .ok_or_else(|| std::io::Error::other("output byte limit"))?;
        self.bytes = n;
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
#[cfg(target_os = "linux")]
mod native;

#[cfg(all(test, target_os = "linux"))]
mod tests;
