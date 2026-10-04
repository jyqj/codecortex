//! Pure, opt-in source declaration addresses. Never changes SymbolRecord or StableId.
//! Inputs are adapter assertions, not filesystem authorization or runtime import evidence.
use crate::{identity::hash, repo_path, source::ByteSpan, CcError, CcResult};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentitySchema {
    PythonDeclarationV1,
}

/// An explicit configuration assertion from a future validated config adapter.
/// `directive` identifies the interpreted setting; digest binds its original bytes.
/// This model does not interpret arbitrary config text or accept inferred roots.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConfiguredRoot {
    pub directory: String,
    pub config_path: String,
    pub config_digest: String,
    pub directive: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScopeKind {
    Class,
    Function,
}

/// Parser-asserted lexical ancestry, outermost first. Names and ranges are checked
/// against captured bytes; AST meaning remains the parser adapter's responsibility.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeclarationSegment {
    pub name: String,
    pub kind: ScopeKind,
    pub span: ByteSpan,
    pub name_span: ByteSpan,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeclarationInput {
    pub file_path: String,
    pub source_digest: String,
    pub ancestry: Vec<DeclarationSegment>,
}

/// A logical address may have multiple declaration occurrences (e.g. conditional
/// definitions). The binding below distinguishes occurrences; this is not a UID.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeclarationAddress {
    pub schema: IdentitySchema,
    pub owner: String,
    pub source_root: String,
    pub module: Vec<String>,
    pub lexical: Vec<(String, ScopeKind)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeclarationBinding {
    pub snapshot_digest: String,
    pub file_path: String,
    pub source_digest: String,
    pub ancestry: Vec<DeclarationSegment>,
    pub root: ConfiguredRoot,
    pub package_files: BTreeMap<String, String>,
}

/// Constructed only by resolve; serialized outputs must be rederived on ingestion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BoundDeclaration {
    address: DeclarationAddress,
    binding: DeclarationBinding,
}
impl BoundDeclaration {
    pub fn address(&self) -> &DeclarationAddress {
        &self.address
    }
    pub fn binding(&self) -> &DeclarationBinding {
        &self.binding
    }
    /// Equality guard for this bound occurrence, not a replacement stable UID.
    pub fn fingerprint(&self) -> CcResult<String> {
        hash(&("source-bound-declaration-v1", self))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentityReason {
    NoConfiguredRoot,
    MultipleRoots,
    OutsideRoot,
    MissingSource,
    StaleSource,
    InvalidDeclaration,
    UnsupportedIdentifier,
    UnsupportedFile,
    NamespaceAncestry,
    RootInitializer,
    LocalDeclaration,
    ModulePackageCollision,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", content = "value", rename_all = "snake_case")]
pub enum IdentityOutcome {
    Derived(Box<BoundDeclaration>),
    Unavailable(IdentityReason),
}

/// Complete immutable inventory of regular files for one owner/config scope.
/// Capture adapters must reject symlinks, native path aliases and incomplete scans.
/// No I/O, environment lookup, default roots, import execution, or cache writes.
pub struct DeclarationSnapshot {
    owner: String,
    files: BTreeMap<String, Vec<u8>>,
    roots: Vec<ConfiguredRoot>,
    digest: String,
}
fn invalid() -> CcError {
    CcError::InvalidParams("invalid declaration snapshot evidence".into())
}
pub fn content_digest(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}
// V1 deliberately admits ASCII identifiers only; Unicode needs Python XID rules.
fn identifier(s: &str) -> bool {
    let mut chars = s.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !matches!(
            s,
            "False"
                | "None"
                | "True"
                | "and"
                | "as"
                | "assert"
                | "async"
                | "await"
                | "break"
                | "class"
                | "continue"
                | "def"
                | "del"
                | "elif"
                | "else"
                | "except"
                | "finally"
                | "for"
                | "from"
                | "global"
                | "if"
                | "import"
                | "in"
                | "is"
                | "lambda"
                | "nonlocal"
                | "not"
                | "or"
                | "pass"
                | "raise"
                | "return"
                | "try"
                | "while"
                | "with"
                | "yield"
        )
}
impl DeclarationSnapshot {
    /// Rederive against this immutable capture. Source bytes alone are insufficient:
    /// config, package boundaries, owner and complete inventory must also match.
    pub fn is_current(&self, declaration: &BoundDeclaration) -> CcResult<bool> {
        let binding = declaration.binding();
        let input = DeclarationInput {
            file_path: binding.file_path.clone(),
            source_digest: binding.source_digest.clone(),
            ancestry: binding.ancestry.clone(),
        };
        Ok(
            matches!(self.resolve(&input)?, IdentityOutcome::Derived(current)
            if current.as_ref() == declaration),
        )
    }

    pub fn new(
        owner: String,
        files: BTreeMap<String, Vec<u8>>,
        mut roots: Vec<ConfiguredRoot>,
    ) -> CcResult<Self> {
        if owner.is_empty() || owner.len() > 4096 || owner.chars().any(char::is_control) {
            return Err(invalid());
        }
        // Inventory keys must already be canonical native repository spellings;
        // never merge aliases. Portable user declaration/root paths normalize once.
        if files
            .keys()
            .any(|p| !repo_path::is_canonical_file(p) || p.len() > 4096)
        {
            return Err(invalid());
        }
        for root in &mut roots {
            root.directory = repo_path::normalize_relative(&root.directory)?;
            if !repo_path::is_canonical_file(&root.config_path)
                || root.directory.len() > 4096
                || root.directive.is_empty()
                || root.directive.len() > 4096
                || root.directive.chars().any(char::is_control)
                || files.get(&root.config_path).map(|b| content_digest(b))
                    != Some(root.config_digest.clone())
            {
                return Err(invalid());
            }
        }
        roots.sort_by(|a, b| {
            a.directory
                .cmp(&b.directory)
                .then(a.config_path.cmp(&b.config_path))
                .then(a.directive.cmp(&b.directive))
        });
        // Bind complete inventory and root evidence, including absence of package
        // markers and competing modules. Conservative whole-scope invalidation.
        let inventory: BTreeMap<_, _> = files.iter().map(|(p, b)| (p, content_digest(b))).collect();
        let digest = hash(&("declaration-snapshot-v1", &owner, inventory, &roots))?;
        Ok(Self {
            owner,
            files,
            roots,
            digest,
        })
    }

    pub fn resolve(&self, input: &DeclarationInput) -> CcResult<IdentityOutcome> {
        use IdentityReason::*;
        let unavailable = |r| Ok(IdentityOutcome::Unavailable(r));
        let file = repo_path::normalize_relative(&input.file_path)?;
        if self.roots.is_empty() {
            return unavailable(NoConfiguredRoot);
        }
        // No root-order or longest-prefix choice; multi-root semantics deferred.
        if self.roots.len() != 1 {
            return unavailable(MultipleRoots);
        }
        let root = &self.roots[0];
        let relative = if root.directory.is_empty() {
            file.as_str()
        } else {
            let Some(relative) = file.strip_prefix(&format!("{}/", root.directory)) else {
                return unavailable(OutsideRoot);
            };
            relative
        };
        let Some(bytes) = self.files.get(&file) else {
            return unavailable(MissingSource);
        };
        if input.source_digest != content_digest(bytes) {
            return unavailable(StaleSource);
        }
        if input.ancestry.is_empty() {
            return unavailable(InvalidDeclaration);
        }
        let mut enclosing = ByteSpan {
            start: 0,
            end: bytes.len(),
        };
        for segment in &input.ancestry {
            if !identifier(&segment.name) {
                return unavailable(UnsupportedIdentifier);
            }
            if segment.span.is_empty()
                || segment.name_span.is_empty()
                || segment.span.start > segment.span.end
                || segment.name_span.start > segment.name_span.end
                || !enclosing.contains(segment.span)
                || !segment.span.contains(segment.name_span)
                || bytes.get(segment.name_span.start..segment.name_span.end)
                    != Some(segment.name.as_bytes())
            {
                return unavailable(InvalidDeclaration);
            }
            enclosing = segment.span;
        }
        if input.ancestry[..input.ancestry.len() - 1]
            .iter()
            .any(|s| s.kind == ScopeKind::Function)
        {
            return unavailable(LocalDeclaration);
        }
        let mut module: Vec<String> = relative.split('/').map(str::to_owned).collect();
        let filename = module.pop().ok_or_else(invalid)?;
        let Some(stem) = filename.strip_suffix(".py") else {
            return unavailable(UnsupportedFile);
        };
        let initializer = stem == "__init__";
        if initializer && module.is_empty() {
            return unavailable(RootInitializer);
        }
        if !initializer && !identifier(stem) {
            return unavailable(UnsupportedFile);
        }
        if module.iter().any(|p| !identifier(p)) {
            return unavailable(UnsupportedIdentifier);
        }
        let mut package_files = BTreeMap::new();
        let mut directory = root.directory.clone();
        for part in &module {
            directory = if directory.is_empty() {
                part.clone()
            } else {
                format!("{directory}/{part}")
            };
            let marker = format!("{directory}/__init__.py");
            let Some(marker_bytes) = self.files.get(&marker) else {
                return unavailable(NamespaceAncestry);
            };
            if self.files.contains_key(&format!("{directory}.py")) {
                return unavailable(ModulePackageCollision);
            }
            package_files.insert(marker, content_digest(marker_bytes));
        }
        if !initializer {
            let base = file.strip_suffix(".py").ok_or_else(invalid)?;
            if self.files.contains_key(&format!("{base}/__init__.py")) {
                return unavailable(ModulePackageCollision);
            }
            module.push(stem.into());
        }
        Ok(IdentityOutcome::Derived(Box::new(BoundDeclaration {
            address: DeclarationAddress {
                schema: IdentitySchema::PythonDeclarationV1,
                owner: self.owner.clone(),
                source_root: root.directory.clone(),
                module,
                lexical: input
                    .ancestry
                    .iter()
                    .map(|s| (s.name.clone(), s.kind))
                    .collect(),
            },
            binding: DeclarationBinding {
                snapshot_digest: self.digest.clone(),
                file_path: file,
                source_digest: input.source_digest.clone(),
                ancestry: input.ancestry.clone(),
                root: root.clone(),
                package_files,
            },
        })))
    }
}
