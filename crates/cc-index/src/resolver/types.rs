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

/// Cache key for `resolve_name` results, combining the lookup parameters.
#[derive(Hash, Eq, PartialEq, Clone)]
pub(in crate::resolver) struct ResolveKey {
    pub(in crate::resolver) name: String,
    pub(in crate::resolver) file_path: String,
    pub(in crate::resolver) line: u32,
    pub(in crate::resolver) container: Option<String>,
}
