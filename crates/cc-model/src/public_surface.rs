//! Versioned, file-local interface evidence for incremental invalidation.
//!
//! A known empty surface is evidence; an unknown surface is not. Fingerprints
//! cover declared syntax, not compiler type checking or runtime execution.
//! Forwarding records name dependencies, never recursively embed their hashes.
use serde::{Deserialize, Serialize};

use crate::{CcError, CcResult};

pub const PUBLIC_SURFACE_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceKnowledge {
    Known,
    KnownEmpty,
    Unknown,
}

/// A declaration's lexical visibility, not a claim of project-wide reachability.
/// Ancestor module declarations remain separate surface entries.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", content = "scope", rename_all = "snake_case")]
pub enum VisibilityDomain {
    Exported,
    Module,
    Crate,
    Restricted(String),
    Global,
}

/// Tokens preserve boundaries and literal bytes: `a b` cannot alias `ab`, and
/// whitespace inside strings must never be collapsed like surrounding trivia.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceToken {
    pub kind: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceEntry {
    pub qualified_name: String,
    pub exported_name: String,
    pub kind: String,
    pub visibility: VisibilityDomain,
    pub signature: Vec<SurfaceToken>,
    pub conditions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceForward {
    /// Module specifier as declared. Resolution belongs to the project model.
    pub source: String,
    pub imported_name: String,
    pub exported_name: String,
    pub visibility: VisibilityDomain,
    pub type_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicSurface {
    pub format_version: u32,
    pub extractor_version: String,
    pub language: String,
    pub module: String,
    pub knowledge: SurfaceKnowledge,
    pub entries: Vec<SurfaceEntry>,
    pub forwards: Vec<SurfaceForward>,
    pub conditions: Vec<String>,
    pub reasons: Vec<String>,
}

impl Default for PublicSurface {
    fn default() -> Self {
        Self {
            format_version: PUBLIC_SURFACE_VERSION,
            extractor_version: "unavailable".into(),
            language: "unknown".into(),
            module: String::new(),
            knowledge: SurfaceKnowledge::Unknown,
            entries: Vec::new(),
            forwards: Vec::new(),
            conditions: Vec::new(),
            reasons: vec!["parser_surface_not_available".into()],
        }
    }
}

impl PublicSurface {
    pub fn new(language: &str, module: &str, extractor_version: &str) -> Self {
        Self {
            language: language.into(),
            module: module.into(),
            extractor_version: extractor_version.into(),
            knowledge: SurfaceKnowledge::KnownEmpty,
            reasons: Vec::new(),
            ..Self::default()
        }
    }

    pub fn mark_unknown(&mut self, reason: &str) {
        self.knowledge = SurfaceKnowledge::Unknown;
        self.reasons.push(reason.into());
    }

    /// Normalize collection order only. Parameter/token order remains semantic.
    pub fn normalize(&mut self) {
        for entry in &mut self.entries {
            entry.conditions.sort();
            entry.conditions.dedup();
        }
        self.entries.sort();
        self.entries.dedup();
        self.forwards.sort();
        self.forwards.dedup();
        self.conditions.sort();
        self.conditions.dedup();
        self.reasons.sort();
        self.reasons.dedup();
        if !self.reasons.is_empty() {
            self.knowledge = SurfaceKnowledge::Unknown;
        } else if self.knowledge != SurfaceKnowledge::Unknown {
            self.knowledge = if self.entries.is_empty() && self.forwards.is_empty() {
                SurfaceKnowledge::KnownEmpty
            } else {
                SurfaceKnowledge::Known
            };
        }
    }

    pub fn validate(&self) -> CcResult<()> {
        let valid = self.format_version == PUBLIC_SURFACE_VERSION
            && !self.extractor_version.is_empty()
            && !self.language.is_empty()
            && match self.knowledge {
                SurfaceKnowledge::Unknown => !self.reasons.is_empty(),
                SurfaceKnowledge::KnownEmpty => {
                    self.reasons.is_empty() && self.entries.is_empty() && self.forwards.is_empty()
                }
                SurfaceKnowledge::Known => {
                    self.reasons.is_empty()
                        && (!self.entries.is_empty() || !self.forwards.is_empty())
                }
            };
        if valid {
            Ok(())
        } else {
            Err(CcError::InvalidParams(
                "invalid or unsupported public surface".into(),
            ))
        }
    }

    /// Single canonical encoding shared by parsing, storage and invalidation.
    /// Integers are little endian; every string/sequence has a u64 length.
    pub fn canonical_bytes(&self) -> CcResult<Vec<u8>> {
        self.validate()?;
        let mut value = self.clone();
        value.normalize();
        let mut bytes = b"codecortex.public-surface\0".to_vec();
        bytes.extend(value.format_version.to_le_bytes());
        string(&mut bytes, &value.extractor_version);
        string(&mut bytes, &value.language);
        string(&mut bytes, &value.module);
        bytes.push(match value.knowledge {
            SurfaceKnowledge::Known => 1,
            SurfaceKnowledge::KnownEmpty => 2,
            SurfaceKnowledge::Unknown => 3,
        });
        count(&mut bytes, value.entries.len());
        for e in &value.entries {
            string(&mut bytes, &e.qualified_name);
            string(&mut bytes, &e.exported_name);
            string(&mut bytes, &e.kind);
            visibility(&mut bytes, &e.visibility);
            count(&mut bytes, e.signature.len());
            for token in &e.signature {
                string(&mut bytes, &token.kind);
                string(&mut bytes, &token.text);
            }
            strings(&mut bytes, &e.conditions);
        }
        count(&mut bytes, value.forwards.len());
        for f in &value.forwards {
            string(&mut bytes, &f.source);
            string(&mut bytes, &f.imported_name);
            string(&mut bytes, &f.exported_name);
            visibility(&mut bytes, &f.visibility);
            bytes.push(u8::from(f.type_only));
        }
        strings(&mut bytes, &value.conditions);
        strings(&mut bytes, &value.reasons);
        Ok(bytes)
    }

    /// Unknown and invalid versions are never comparable to known-empty.
    pub fn fingerprint(&self) -> Option<String> {
        if self.knowledge == SurfaceKnowledge::Unknown {
            return None;
        }
        let bytes = self.canonical_bytes().ok()?;
        Some(format!(
            "ps{}:{}",
            self.format_version,
            blake3::hash(&bytes).to_hex()
        ))
    }

    /// A missing previous record also requires conservative invalidation.
    pub fn changed_from(&self, old: Option<&Self>) -> bool {
        match (old.and_then(Self::fingerprint), self.fingerprint()) {
            (Some(a), Some(b)) => a != b,
            _ => true,
        }
    }
}

fn count(out: &mut Vec<u8>, n: usize) {
    out.extend((n as u64).to_le_bytes());
}
fn string(out: &mut Vec<u8>, text: &str) {
    count(out, text.len());
    out.extend(text.as_bytes());
}
fn strings(out: &mut Vec<u8>, texts: &[String]) {
    count(out, texts.len());
    for text in texts {
        string(out, text);
    }
}
fn visibility(out: &mut Vec<u8>, domain: &VisibilityDomain) {
    match domain {
        VisibilityDomain::Exported => out.push(1),
        VisibilityDomain::Module => out.push(2),
        VisibilityDomain::Crate => out.push(3),
        VisibilityDomain::Restricted(path) => {
            out.push(4);
            string(out, path);
        }
        VisibilityDomain::Global => out.push(5),
    }
}

/// Coverage of successfully parsed files in THIS build, not whole-repo coverage
/// or compiler certification. Each unknown file may contribute several reasons.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SurfaceCoverage {
    pub parsed_files: usize,
    pub known: usize,
    pub known_empty: usize,
    pub unknown: usize,
    pub unknown_reasons: std::collections::BTreeMap<String, usize>,
}
impl SurfaceCoverage {
    pub fn observe(&mut self, surface: &PublicSurface) {
        self.parsed_files += 1;
        match surface.knowledge {
            SurfaceKnowledge::Known => self.known += 1,
            SurfaceKnowledge::KnownEmpty => self.known_empty += 1,
            SurfaceKnowledge::Unknown => {
                self.unknown += 1;
                let reasons: std::collections::BTreeSet<_> = surface.reasons.iter().collect();
                for reason in reasons {
                    *self.unknown_reasons.entry(reason.clone()).or_default() += 1;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn entry(name: &str) -> SurfaceEntry {
        SurfaceEntry {
            qualified_name: name.into(),
            exported_name: name.into(),
            kind: "function".into(),
            visibility: VisibilityDomain::Exported,
            signature: vec![SurfaceToken {
                kind: "string".into(),
                text: "a b".into(),
            }],
            conditions: vec![],
        }
    }
    #[test]
    fn unknown_is_not_empty_or_unchanged() {
        let unknown = PublicSurface::default();
        let empty = PublicSurface::new("rust", "a.rs", "rust-v1");
        assert_eq!(unknown.fingerprint(), None);
        assert!(empty.fingerprint().is_some());
        assert!(unknown.changed_from(Some(&unknown)));
        assert!(empty.changed_from(Some(&unknown)));
        assert!(empty.changed_from(None));
        assert!(!empty.changed_from(Some(&empty)));
    }
    #[test]
    fn order_duplicates_and_literal_boundaries() {
        let mut a = PublicSurface::new("typescript", "a.ts", "ts-v1");
        a.entries = vec![entry("z"), entry("a")];
        a.normalize();
        let mut b = a.clone();
        b.entries.reverse();
        b.entries.push(entry("z"));
        assert_eq!(a.canonical_bytes().unwrap(), b.canonical_bytes().unwrap());
        b.entries[0].signature[0].text = "ab".into();
        assert_ne!(a.fingerprint(), b.fingerprint());
        let mut c = a.clone();
        c.extractor_version.push('2');
        assert_ne!(a.fingerprint(), c.fingerprint());
        c.format_version += 1;
        assert!(c.validate().is_err());
        assert_eq!(c.fingerprint(), None);
    }
    #[test]
    fn known_empty_encoding_has_independent_golden() {
        let a = PublicSurface::new("r", "m", "v");
        let mut expected = b"codecortex.public-surface\0".to_vec();
        expected.extend([1, 0, 0, 0]);
        expected.extend([
            1, 0, 0, 0, 0, 0, 0, 0, b'v', 1, 0, 0, 0, 0, 0, 0, 0, b'r', 1, 0, 0, 0, 0, 0, 0, 0,
            b'm', 2,
        ]);
        expected.extend([0; 32]); // entries, forwards, conditions, reasons: four empty sequences
        assert_eq!(a.canonical_bytes().unwrap(), expected);
        assert_eq!(
            a.fingerprint().unwrap(),
            format!("ps1:{}", blake3::hash(&expected).to_hex())
        );
    }
}
