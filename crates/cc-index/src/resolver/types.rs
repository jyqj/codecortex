//! Standalone type definitions for the resolver module.

use std::collections::HashMap;

use cc_model::edge::ResolutionKind;
use cc_model::scope::ScopeBinding;
use cc_model::symbol::SymbolKind;

// ---------------------------------------------------------------------------
// Data structures
// ---------------------------------------------------------------------------

/// A scope extracted from parse output, used for scope-chain resolution.
#[derive(Clone, Debug)]
pub struct CatalogScope {
    pub scope_id: String,
    pub parent_id: Option<String>,
    pub name: String,
    pub file_path: String,
    pub start_line: u32,
    pub end_line: u32,
    pub bindings: Vec<ScopeBinding>,
}

/// An import binding used during resolution.
#[derive(Clone, Debug)]
pub struct ImportBinding {
    pub local_name: String,
    pub source_module: String,
    pub imported_name: Option<String>, // None for namespace import
    pub file_path: String,
    pub is_namespace: bool,
    pub is_default: bool,
}

/// Per-file derived resolver context computed once from a [`ParseOutcome`].
#[derive(Clone, Debug, Default)]
pub struct ResolutionContext {
    pub scopes: HashMap<String, CatalogScope>,
    pub imports: Vec<ImportBinding>,
}

/// Per-call-site disambiguation signals carried from a call edge into name
/// resolution. Only the fuzzy-multi ladder steps consume them, so a default
/// (empty) value reproduces signal-free resolution exactly.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct CallSiteSignals<'a> {
    /// Number of arguments at the call site (parser-extracted).
    pub(crate) arg_count: Option<u32>,
    /// Receiver expression of the call (`obj` in `obj.method(...)`).
    pub(crate) receiver: Option<&'a str>,
}

// ---------------------------------------------------------------------------
// Resolution ladder
// ---------------------------------------------------------------------------

/// One step of the resolution ladder.
///
/// [`RESOLVE_LADDER`] is the single declaration of the step order;
/// `resolve_name_inner` consumes it through an exhaustive `match`, so adding
/// a variant here fails compilation at the consuming site until the new
/// step's behavior is written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ResolveStep {
    /// `this.x` / `self.x` member resolution on the owner class.
    /// Authoritative for this/self-prefixed names: a miss stops the ladder
    /// (a this/self reference never resolves to an unrelated global).
    SelfMember,
    /// Lexical scope-chain bindings.
    ScopeBinding,
    /// Same-file candidates (qname exact, member chain, scope proximity).
    SameFile,
    /// Import bindings traced to their exporting module.
    Import,
    /// Qualified-name suffix match (pkg.mod.Func ↔ mod.Func).
    Suffix,
    /// Globally unique leaf-name match.
    GlobalUnique,
    /// Single fuzzy candidate by leaf name.
    FuzzySingle,
    /// Fuzzy-multi narrowing ①: TypeCatalog parameter-count filter driven by
    /// the call site's `arg_count` signal.
    FuzzyArgCount,
    /// Fuzzy-multi narrowing ②: TypeCatalog receiver-type filter driven by
    /// the call site's `receiver` signal.
    FuzzyReceiver,
    /// Fuzzy-multi narrowing ③: import-distance (path-prefix) ranking among
    /// the surviving candidates.
    FuzzyImportDistance,
}

/// The resolution ladder, in evaluation order. The signal-driven steps sit
/// between `FuzzySingle` and the import-distance fallback so that call-site
/// evidence outranks pure path proximity.
pub(crate) const RESOLVE_LADDER: [ResolveStep; 10] = [
    ResolveStep::SelfMember,
    ResolveStep::ScopeBinding,
    ResolveStep::SameFile,
    ResolveStep::Import,
    ResolveStep::Suffix,
    ResolveStep::GlobalUnique,
    ResolveStep::FuzzySingle,
    ResolveStep::FuzzyArgCount,
    ResolveStep::FuzzyReceiver,
    ResolveStep::FuzzyImportDistance,
];

/// Internal resolution kind — maps to `ResolutionKind` for output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InternalResKind {
    Exact,
    Qualified,
    ScopeResolved,
    ImportResolved,
    /// Globally unique leaf-name match with no stronger scope/import proof.
    GlobalUnique,
    /// Qualified-name suffix match (e.g. pkg.mod.Func ↔ mod.Func).
    SuffixMatch,
    Heuristic,
    /// Single candidate resolved by name (no scope/import proof).
    FuzzySingle,
    /// Multiple fuzzy candidates narrowed to exactly one by call-site
    /// signals (arg count / receiver type).
    FuzzySignal,
    /// Multiple candidates, resolved by import-distance tie-breaking.
    FuzzyMulti,
    Unresolved,
}

impl InternalResKind {
    pub(in crate::resolver) fn to_resolution_kind(self) -> ResolutionKind {
        match self {
            Self::Exact => ResolutionKind::Exact,
            Self::Qualified => ResolutionKind::Qualified,
            Self::ScopeResolved => ResolutionKind::ScopeResolved,
            Self::ImportResolved => ResolutionKind::ScopeResolved, // best available mapping
            Self::GlobalUnique
            | Self::SuffixMatch
            | Self::Heuristic
            | Self::FuzzySingle
            | Self::FuzzySignal
            | Self::FuzzyMulti => ResolutionKind::Heuristic,
            Self::Unresolved => ResolutionKind::Unresolved,
        }
    }

    pub(in crate::resolver) fn base_confidence(self) -> f64 {
        match self {
            Self::Exact => 1.0,
            Self::Qualified => 0.95,
            Self::ScopeResolved => 0.9,
            Self::ImportResolved => 0.85,
            Self::GlobalUnique => 0.75,
            Self::SuffixMatch => 0.65,
            Self::Heuristic => 0.5,
            Self::FuzzySingle => 0.40,
            // Between FuzzySingle (0.40) and GlobalUnique (0.75): the
            // call-site signal positively discriminated among same-name
            // candidates (stronger evidence than an uncontested single
            // match), but parser-derived arg counts and receiver text are
            // themselves heuristic, so it must stay below the
            // global-uniqueness proof.
            Self::FuzzySignal => 0.55,
            Self::FuzzyMulti => 0.30,
            Self::Unresolved => 0.0,
        }
    }

    pub(in crate::resolver) fn strategy_name(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Qualified => "qualified",
            Self::ScopeResolved => "scope",
            Self::ImportResolved => "import_map",
            Self::GlobalUnique => "global_unique",
            Self::SuffixMatch => "suffix",
            Self::Heuristic => "heuristic",
            Self::FuzzySingle => "fuzzy_single",
            Self::FuzzySignal => "fuzzy_signal",
            Self::FuzzyMulti => "fuzzy_multi",
            Self::Unresolved => "unresolved",
        }
    }
}

/// Result of recursive name resolution.
#[derive(Clone, Debug)]
pub struct ResolveResult {
    pub catalog_index: usize,
    pub resolution_kind: InternalResKind,
    pub confidence: f64,
    /// Number of candidates the winning step considered (pre-narrowing for
    /// the fuzzy steps); 1 for proof-based steps.
    pub candidate_count: u32,
    /// The ladder step that produced this result.
    pub(crate) winning_step: ResolveStep,
}

impl ResolveResult {
    /// Result from a proof-based step: a single candidate at the kind's base
    /// confidence.
    pub(in crate::resolver) fn single(
        catalog_index: usize,
        kind: InternalResKind,
        step: ResolveStep,
    ) -> Self {
        Self {
            catalog_index,
            resolution_kind: kind,
            confidence: kind.base_confidence(),
            candidate_count: 1,
            winning_step: step,
        }
    }

    /// Strategy label for edge/ref `resolution_strategy`. Signal-narrowed
    /// fuzzy wins get step-specific labels; every other step keeps the
    /// kind-level label so pre-existing strategy values are unchanged.
    pub(in crate::resolver) fn strategy_name(&self) -> &'static str {
        match self.winning_step {
            ResolveStep::FuzzyArgCount => "fuzzy_arg_count",
            ResolveStep::FuzzyReceiver => "fuzzy_receiver",
            _ => self.resolution_kind.strategy_name(),
        }
    }
}

pub(in crate::resolver) fn default_resolution_confidence(kind: ResolutionKind) -> f64 {
    match kind {
        ResolutionKind::Exact => 1.0,
        ResolutionKind::Qualified => 0.95,
        ResolutionKind::ScopeResolved => 0.9,
        ResolutionKind::Heuristic => 0.5,
        ResolutionKind::Unresolved => 0.0,
    }
}

pub(in crate::resolver) fn default_resolution_strategy(kind: ResolutionKind) -> &'static str {
    match kind {
        ResolutionKind::Exact => "parser_exact",
        ResolutionKind::Qualified => "parser_qualified",
        ResolutionKind::ScopeResolved => "parser_scope",
        ResolutionKind::Heuristic => "heuristic",
        ResolutionKind::Unresolved => "unresolved",
    }
}

// ---------------------------------------------------------------------------
// CatalogEntry
// ---------------------------------------------------------------------------

/// A single entry in the symbol catalog (extended from original).
#[derive(Clone, Debug)]
pub(in crate::resolver) struct CatalogEntry {
    pub(in crate::resolver) symbol_id: String,
    pub(in crate::resolver) symbol_uid: Option<String>,
    pub(in crate::resolver) name: String,
    pub(in crate::resolver) file_path: String,
    pub(in crate::resolver) kind: SymbolKind,
    pub(in crate::resolver) container: Option<String>,
    pub(in crate::resolver) qname: Option<String>,
    pub(in crate::resolver) is_default_export: bool,
    pub(in crate::resolver) start_line: u32,
    pub(in crate::resolver) end_line: u32,
    pub(in crate::resolver) scope_id: Option<String>,
}

// ---------------------------------------------------------------------------
// Resolve cache key
// ---------------------------------------------------------------------------

/// Compute a u64 hash from borrowed references for resolve_name cache lookup,
/// avoiding per-call String allocations.
///
/// Call-site signals are part of the key: they are deterministic inputs of
/// the resolution, so hashing them keeps cache hits for repeated identical
/// call sites without ever serving a signal-narrowed result to a
/// signal-free caller (or vice versa).
pub(in crate::resolver) fn resolve_key_hash(
    name: &str,
    file_path: &str,
    line: u32,
    container: Option<&str>,
    signals: CallSiteSignals<'_>,
) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    name.hash(&mut hasher);
    file_path.hash(&mut hasher);
    line.hash(&mut hasher);
    container.hash(&mut hasher);
    signals.arg_count.hash(&mut hasher);
    signals.receiver.hash(&mut hasher);
    hasher.finish()
}
