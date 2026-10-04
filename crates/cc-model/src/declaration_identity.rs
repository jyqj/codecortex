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

/// Caller-selected logical work limits. Zero admits only empty values; no unlimited mode.
/// Byte limits use UTF-8 bytes / slice lengths, not capacity or allocator/RSS usage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeclarationLimits {
    pub max_files: usize,
    pub max_total_bytes: usize,
    pub max_file_bytes: usize,
    pub max_roots: usize,
    pub max_evidence: usize,
    pub max_ancestry_depth: usize,
    pub max_identifier_bytes: usize,
}
impl DeclarationLimits {
    /// Finite compatibility policy for the unwired prototype, not production policy.
    pub const PROTOTYPE: Self = Self {
        max_files: 4096,
        max_total_bytes: 64 * 1024 * 1024,
        max_file_bytes: 8 * 1024 * 1024,
        max_roots: 64,
        max_evidence: 256,
        max_ancestry_depth: 256,
        max_identifier_bytes: 4096,
    };

    /// Size-only preflight, also usable before capture allocations. Counts and sums
    /// are checked; supplied sizes must exactly match file_count. No bytes are hashed.
    pub fn check_inventory_sizes(
        self,
        file_count: usize,
        sizes: impl IntoIterator<Item = usize>,
    ) -> DeclarationResult<usize> {
        check(ResourceKind::FileCount, file_count, self.max_files)?;
        let mut count = 0usize;
        let mut total = 0usize;
        for size in sizes {
            count = add(ResourceKind::FileCount, count, 1)?;
            check(ResourceKind::FileCount, count, file_count)?;
            check(ResourceKind::FileBytes, size, self.max_file_bytes)?;
            total = add(ResourceKind::TotalBytes, total, size)?;
            check(ResourceKind::TotalBytes, total, self.max_total_bytes)?;
        }
        if count != file_count {
            return Err(invalid().into());
        }
        Ok(total)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceKind {
    FileCount,
    TotalBytes,
    FileBytes,
    Roots,
    Evidence,
    AncestryDepth,
    IdentifierBytes,
    MetadataBytes,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceRefusal {
    LimitExceeded {
        resource: ResourceKind,
        limit: usize,
        actual: usize,
    },
    ArithmeticOverflow {
        resource: ResourceKind,
    },
}
#[derive(Debug, thiserror::Error)]
pub enum DeclarationError {
    #[error("declaration resource refusal: {0:?}")]
    Resource(ResourceRefusal),
    #[error(transparent)]
    Model(#[from] CcError),
}
pub type DeclarationResult<T> = Result<T, DeclarationError>;
fn check(resource: ResourceKind, actual: usize, limit: usize) -> DeclarationResult<()> {
    if actual > limit {
        Err(DeclarationError::Resource(ResourceRefusal::LimitExceeded {
            resource,
            limit,
            actual,
        }))
    } else {
        Ok(())
    }
}
fn add(resource: ResourceKind, a: usize, b: usize) -> DeclarationResult<usize> {
    a.checked_add(b).ok_or(DeclarationError::Resource(
        ResourceRefusal::ArithmeticOverflow { resource },
    ))
}
fn compatibility_error(error: DeclarationError) -> CcError {
    match error {
        DeclarationError::Model(error) => error,
        error => CcError::InvalidParams(error.to_string()),
    }
}
fn metadata(value: &str) -> DeclarationResult<()> {
    check(ResourceKind::MetadataBytes, value.len(), 4096)
}

mod sealed {
    use std::collections::BTreeMap;
    pub trait Inventory {}
    impl Inventory for BTreeMap<String, Vec<u8>> {}
    impl Inventory for &BTreeMap<String, Vec<u8>> {}
}
/// Sealed storage: only an owned map or shared borrow can carry admitted evidence.
/// Custom providers cannot substitute changing content after digest validation.
pub trait DeclarationInventory: sealed::Inventory {
    fn inventory(&self) -> &BTreeMap<String, Vec<u8>>;
}
impl DeclarationInventory for BTreeMap<String, Vec<u8>> {
    fn inventory(&self) -> &BTreeMap<String, Vec<u8>> {
        self
    }
}
impl DeclarationInventory for &BTreeMap<String, Vec<u8>> {
    fn inventory(&self) -> &BTreeMap<String, Vec<u8>> {
        self
    }
}

/// Complete immutable inventory of regular files for one owner/config scope.
/// Capture adapters must reject symlinks, native path aliases and incomplete scans.
/// No I/O, environment lookup, default roots, import execution, or cache writes.
pub struct DeclarationSnapshot<F = BTreeMap<String, Vec<u8>>> {
    owner: String,
    files: F,
    inventory: BTreeMap<String, String>,
    limits: DeclarationLimits,
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
    /// Compatibility only: uses DeclarationLimits::PROTOTYPE. Production adapters
    /// must select policy explicitly via with_limits; this never admits unlimited data.
    pub fn new(
        owner: String,
        files: BTreeMap<String, Vec<u8>>,
        roots: Vec<ConfiguredRoot>,
    ) -> CcResult<Self> {
        Self::with_limits(owner, files, roots, DeclarationLimits::PROTOTYPE)
            .map_err(compatibility_error)
    }
}
impl<F: DeclarationInventory> DeclarationSnapshot<F> {
    /// Validate all sizes and metadata before normalization, hashes or output maps.
    /// Passing &BTreeMap borrows immutable bytes for the lifetime of the snapshot;
    /// passing BTreeMap moves bytes. Neither clones file contents. Caller allocations
    /// made before this call are outside this contract; admission is atomic.
    pub fn with_limits(
        owner: String,
        files: F,
        mut roots: Vec<ConfiguredRoot>,
        limits: DeclarationLimits,
    ) -> DeclarationResult<Self> {
        let captured = files.inventory();
        check(ResourceKind::FileCount, captured.len(), limits.max_files)?;
        check(ResourceKind::Roots, roots.len(), limits.max_roots)?;
        limits.check_inventory_sizes(captured.len(), captured.values().map(Vec::len))?;
        metadata(&owner)?;
        if owner.is_empty() || owner.chars().any(char::is_control) {
            return Err(invalid().into());
        }
        for path in captured.keys() {
            metadata(path)?;
            if !repo_path::is_canonical_file(path) {
                return Err(invalid().into());
            }
        }
        for root in &roots {
            metadata(&root.directory)?;
            metadata(&root.config_path)?;
            metadata(&root.directive)?;
            metadata(&root.config_digest)?;
            if !repo_path::is_canonical_file(&root.config_path)
                || root.directive.is_empty()
                || root.directive.chars().any(char::is_control)
            {
                return Err(invalid().into());
            }
        }
        // Normalize before hashing any content; paths are already size bounded.
        for root in &mut roots {
            root.directory = repo_path::normalize_relative(&root.directory)?;
        }
        let inventory: BTreeMap<String, String> = captured
            .iter()
            .map(|(p, b)| (p.clone(), content_digest(b)))
            .collect();
        for root in &roots {
            if inventory.get(&root.config_path) != Some(&root.config_digest) {
                return Err(invalid().into());
            }
        }
        roots.sort_by(|a, b| {
            a.directory
                .cmp(&b.directory)
                .then(a.config_path.cmp(&b.config_path))
                .then(a.directive.cmp(&b.directive))
        });
        let digest = hash(&("declaration-snapshot-v1", &owner, &inventory, &roots))?;
        Ok(Self {
            owner,
            files,
            inventory,
            limits,
            roots,
            digest,
        })
    }
    pub fn limits(&self) -> DeclarationLimits {
        self.limits
    }

    /// Rederive without cloning caller ancestry before budget checks.
    pub fn is_current_with_limits(
        &self,
        declaration: &BoundDeclaration,
    ) -> DeclarationResult<bool> {
        let binding = declaration.binding();
        Ok(
            matches!(self.resolve_parts(&binding.file_path, &binding.source_digest, &binding.ancestry)?,
            IdentityOutcome::Derived(current) if current.as_ref() == declaration),
        )
    }
    pub fn is_current(&self, declaration: &BoundDeclaration) -> CcResult<bool> {
        self.is_current_with_limits(declaration)
            .map_err(compatibility_error)
    }
    /// Compatibility error envelope; typed resource refusals use resolve_with_limits.
    pub fn resolve(&self, input: &DeclarationInput) -> CcResult<IdentityOutcome> {
        self.resolve_with_limits(input).map_err(compatibility_error)
    }
    pub fn resolve_with_limits(
        &self,
        input: &DeclarationInput,
    ) -> DeclarationResult<IdentityOutcome> {
        self.resolve_parts(&input.file_path, &input.source_digest, &input.ancestry)
    }
    fn resolve_parts(
        &self,
        file_path: &str,
        source_digest: &str,
        ancestry: &[DeclarationSegment],
    ) -> DeclarationResult<IdentityOutcome> {
        check(
            ResourceKind::AncestryDepth,
            ancestry.len(),
            self.limits.max_ancestry_depth,
        )?;
        metadata(file_path)?;
        metadata(source_digest)?;
        for segment in ancestry {
            check(
                ResourceKind::IdentifierBytes,
                segment.name.len(),
                self.limits.max_identifier_bytes,
            )?;
        }
        use IdentityReason::*;
        let unavailable = |r| Ok(IdentityOutcome::Unavailable(r));
        let file = repo_path::normalize_relative(file_path)?;
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
        let package_count = relative
            .split('/')
            .count()
            .checked_sub(1)
            .ok_or_else(invalid)?;
        check(
            ResourceKind::Evidence,
            package_count,
            self.limits.max_evidence,
        )?;
        for (index, part) in relative.split('/').enumerate() {
            let name = if index == package_count {
                part.strip_suffix(".py").unwrap_or(part)
            } else {
                part
            };
            if index == package_count && part == "__init__.py" {
                continue;
            }
            check(
                ResourceKind::IdentifierBytes,
                name.len(),
                self.limits.max_identifier_bytes,
            )?;
        }
        let Some(bytes) = self.files.inventory().get(&file) else {
            return unavailable(MissingSource);
        };
        if self.inventory.get(&file).map(String::as_str) != Some(source_digest) {
            return unavailable(StaleSource);
        }
        if ancestry.is_empty() {
            return unavailable(InvalidDeclaration);
        }
        let mut enclosing = ByteSpan {
            start: 0,
            end: bytes.len(),
        };
        for segment in ancestry {
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
        if ancestry[..ancestry.len() - 1]
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
            let Some(marker_digest) = self.inventory.get(&marker) else {
                return unavailable(NamespaceAncestry);
            };
            if self
                .files
                .inventory()
                .contains_key(&format!("{directory}.py"))
            {
                return unavailable(ModulePackageCollision);
            }
            package_files.insert(marker, marker_digest.clone());
        }
        if !initializer {
            let base = file.strip_suffix(".py").ok_or_else(invalid)?;
            if self
                .files
                .inventory()
                .contains_key(&format!("{base}/__init__.py"))
            {
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
                lexical: ancestry.iter().map(|s| (s.name.clone(), s.kind)).collect(),
            },
            binding: DeclarationBinding {
                snapshot_digest: self.digest.clone(),
                file_path: file,
                source_digest: source_digest.to_owned(),
                ancestry: ancestry.to_vec(),
                root: root.clone(),
                package_files,
            },
        })))
    }
}
