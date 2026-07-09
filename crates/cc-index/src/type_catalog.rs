//! Lightweight type catalog for method dispatch resolution.
//!
//! Stores type information extracted from parsed symbols to improve
//! call graph accuracy when multiple symbols share the same name.
//! This is **not** a full type system — it only indexes information
//! directly visible in declarations (receiver types, parameter counts,
//! base types) and uses it for disambiguation during resolution.
//!
//! # Incremental maintenance
//!
//! The catalog supports per-file removal ([`TypeCatalog::remove_files`]) and
//! per-symbol addition ([`TypeCatalog::add_symbol`]) so the resolver's
//! cross-build catalog cache can delta-maintain it instead of rebuilding
//! from every persisted symbol each build. To make removal exact, the three
//! type maps store one contribution per *(file, value)* pair instead of a
//! last-writer-wins scalar: reads take the last live contribution, which
//! preserves the historical "later insert wins" semantics while letting a
//! removed file's contribution disappear without erasing another file's.

use std::collections::{HashMap, HashSet};

use cc_model::symbol::{SymbolKind, SymbolRecord};
use cc_model::type_assign::TypeAssignRecord;

// ---------------------------------------------------------------------------
// Data structures
// ---------------------------------------------------------------------------

/// A method entry in the type catalog.
#[derive(Clone, Debug)]
struct MethodEntry {
    symbol_uid: String,
    receiver_type: Option<String>,
    param_count: Option<u32>,
}

/// Type hierarchy information for a named type.
#[derive(Clone, Debug)]
struct TypeInfo {
    base_types: Vec<String>,
    implements: Vec<String>,
}

/// The identity facets of a symbol that key its type-catalog contributions.
/// Borrowed view so removal can be driven from any entry-like store (the
/// resolver's `CatalogEntry`) without cloning the whole record. The owning
/// file is not part of the meta — removal is always file-scoped, so the
/// caller passes the removed-file set separately.
pub(crate) struct SymbolKeyMeta<'a> {
    pub(crate) name: &'a str,
    pub(crate) qname: Option<&'a str>,
    pub(crate) kind: SymbolKind,
    pub(crate) symbol_uid: Option<&'a str>,
}

/// A lightweight type catalog for method dispatch resolution.
///
/// Built from parsed [`SymbolRecord`]s, it provides:
/// - method lookup by receiver type
/// - method disambiguation by parameter count
/// - type alias chasing (up to 16 levels)
/// - subtype relationship checks via base_types / implements
pub struct TypeCatalog {
    /// method_name (lowercase) -> list of method entries
    method_index: HashMap<String, Vec<MethodEntry>>,
    /// canonical qname (lowercase) -> per-file contributions; the *last*
    /// entry is the live TypeInfo (matches the historical insert-overwrite).
    type_index_by_qname: HashMap<String, Vec<(String, TypeInfo)>>,
    /// short name (lowercase) -> (file, canonical qname lowercase) pairs.
    /// Readers reduce to the distinct qname set.
    short_to_qnames: HashMap<String, Vec<(String, String)>>,
    /// alias_name (lowercase) -> per-file (file, canonical lowercase) pairs;
    /// the last entry is the live alias target.
    type_aliases: HashMap<String, Vec<(String, String)>>,
    /// Maps (file_path, var_name_lowercase) -> type_name for local variable type inference.
    type_assign_index: HashMap<(String, String), String>,
}

impl TypeCatalog {
    fn empty() -> Self {
        Self {
            method_index: HashMap::new(),
            type_index_by_qname: HashMap::new(),
            short_to_qnames: HashMap::new(),
            type_aliases: HashMap::new(),
            type_assign_index: HashMap::new(),
        }
    }

    /// Build a TypeCatalog from parsed symbols (any iterable of references).
    pub fn build_from_symbols<'a, I>(symbols: I) -> Self
    where
        I: IntoIterator<Item = &'a SymbolRecord>,
    {
        let mut catalog = Self::empty();
        for sym in symbols {
            catalog.add_symbol(sym);
        }
        catalog
    }

    /// Register one symbol's contributions (methods, type hierarchy, alias).
    /// The insertion-order semantics match the historical
    /// `build_from_symbols` loop exactly.
    pub(crate) fn add_symbol(&mut self, sym: &SymbolRecord) {
        let uid = match sym.symbol_uid.as_ref() {
            Some(u) => u.clone(),
            None => return,
        };

        match sym.kind {
            SymbolKind::Method | SymbolKind::Function => {
                let entry = MethodEntry {
                    symbol_uid: uid,
                    receiver_type: sym.receiver_type.clone(),
                    param_count: sym.param_count,
                };
                self.method_index
                    .entry(sym.name.to_lowercase())
                    .or_default()
                    .push(entry);
            }
            SymbolKind::Class | SymbolKind::Interface | SymbolKind::Enum => {
                let base = sym
                    .base_types
                    .as_deref()
                    .map(|s| {
                        s.split(',')
                            .map(|t| t.trim().to_string())
                            .filter(|t| !t.is_empty())
                            .collect()
                    })
                    .unwrap_or_default();
                let impls = sym
                    .implements
                    .as_deref()
                    .map(|s| {
                        s.split(',')
                            .map(|t| t.trim().to_string())
                            .filter(|t| !t.is_empty())
                            .collect()
                    })
                    .unwrap_or_default();

                // Use qname as the canonical key (fallback to name)
                let canonical_key = sym.qname.as_ref().unwrap_or(&sym.name).to_lowercase();

                self.type_index_by_qname
                    .entry(canonical_key.clone())
                    .or_default()
                    .push((
                        sym.file_path.clone(),
                        TypeInfo {
                            base_types: base,
                            implements: impls,
                        },
                    ));

                // Maintain short name -> qnames mapping
                self.short_to_qnames
                    .entry(sym.name.to_lowercase())
                    .or_default()
                    .push((sym.file_path.clone(), canonical_key));
            }
            SymbolKind::TypeAlias => {
                // For type aliases, if we have base_types, use the first as the canonical
                if let Some(ref bt) = sym.base_types {
                    let first = bt.split(',').next().unwrap_or("").trim();
                    if !first.is_empty() {
                        self.type_aliases
                            .entry(sym.name.to_lowercase())
                            .or_default()
                            .push((sym.file_path.clone(), first.to_lowercase()));
                    }
                }
            }
            _ => {}
        }
    }

    /// Remove the contributions of symbols that lived in `removed_files`.
    /// `removed` carries the key facets of every removed symbol so only the
    /// affected buckets are probed (batched: each bucket is scanned once
    /// regardless of how many removed symbols share its key).
    pub(crate) fn remove_files(&mut self, removed: &[SymbolKeyMeta<'_>], removed_files: &HashSet<String>) {
        let mut removed_uids: HashSet<&str> = HashSet::new();
        let mut method_keys: HashSet<String> = HashSet::new();
        let mut type_keys: HashSet<(String, String)> = HashSet::new(); // (canonical, short)
        let mut alias_keys: HashSet<String> = HashSet::new();
        for meta in removed {
            if meta.symbol_uid.is_none() {
                continue; // uid-less symbols never entered the catalog
            }
            match meta.kind {
                SymbolKind::Method | SymbolKind::Function => {
                    removed_uids.insert(meta.symbol_uid.unwrap_or_default());
                    method_keys.insert(meta.name.to_lowercase());
                }
                SymbolKind::Class | SymbolKind::Interface | SymbolKind::Enum => {
                    let canonical = meta.qname.unwrap_or(meta.name).to_lowercase();
                    type_keys.insert((canonical, meta.name.to_lowercase()));
                }
                SymbolKind::TypeAlias => {
                    alias_keys.insert(meta.name.to_lowercase());
                }
                _ => {}
            }
        }

        for key in &method_keys {
            if let Some(bucket) = self.method_index.get_mut(key) {
                bucket.retain(|e| !removed_uids.contains(e.symbol_uid.as_str()));
                if bucket.is_empty() {
                    self.method_index.remove(key);
                }
            }
        }
        for (canonical, short) in &type_keys {
            if let Some(bucket) = self.type_index_by_qname.get_mut(canonical) {
                bucket.retain(|(file, _)| !removed_files.contains(file));
                if bucket.is_empty() {
                    self.type_index_by_qname.remove(canonical);
                }
            }
            if let Some(bucket) = self.short_to_qnames.get_mut(short) {
                bucket.retain(|(file, _)| !removed_files.contains(file));
                if bucket.is_empty() {
                    self.short_to_qnames.remove(short);
                }
            }
        }
        for key in &alias_keys {
            if let Some(bucket) = self.type_aliases.get_mut(key) {
                bucket.retain(|(file, _)| !removed_files.contains(file));
                if bucket.is_empty() {
                    self.type_aliases.remove(key);
                }
            }
        }
        // type_assign_index is build-local (reset each build); no file cleanup.
    }

    /// Discard variable type assignments. They are derived from the current
    /// batch's parse outcomes only (persisted files' assigns are never
    /// reloaded), so a reused catalog must reset them each build to keep
    /// fresh-build semantics.
    pub(crate) fn reset_type_assigns(&mut self) {
        self.type_assign_index.clear();
    }

    /// Normalize a type name for comparison:
    /// - Strip pointer/reference markers (*, &, ?, [])
    /// - Strip generic params (<T>)
    /// - Resolve alias
    /// - Map short name to qname if unique
    fn normalize_type_name(&self, raw: &str) -> String {
        let mut s = raw.trim().to_string();

        // Strip pointer/reference markers from start
        s = s.trim_start_matches(['*', '&']).to_string();
        // Strip trailing optional/array markers
        s = s.trim_end_matches(['?', ']']).to_string();
        s = s.trim_end_matches('[').to_string();

        // Strip generic params
        if let Some(idx) = s.find('<') {
            s.truncate(idx);
        }

        let lower = s.to_lowercase();

        // Resolve alias
        let resolved = self.resolve_alias(&lower);

        // Map short name to qname if unambiguous (a single distinct qname
        // across all live contributions).
        if let Some(pairs) = self.short_to_qnames.get(resolved) {
            if let Some((_, first)) = pairs.first() {
                if pairs.iter().all(|(_, q)| q == first) {
                    return first.clone();
                }
            }
        }

        resolved.to_string()
    }

    /// The live TypeInfo for a canonical key: the last contribution wins,
    /// mirroring the historical insert-overwrite semantics.
    fn type_info(&self, canonical: &str) -> Option<&TypeInfo> {
        self.type_index_by_qname
            .get(canonical)
            .and_then(|pairs| pairs.last())
            .map(|(_, info)| info)
    }

    /// Resolve a method by matching the receiver expression against known receiver types.
    ///
    /// `receiver_expr` is typically a dotted expression like `myStruct` or `self.client`.
    /// We extract the type-like part (last segment, or known type) and match against
    /// method entries that have a matching receiver_type.
    ///
    /// Returns the symbol_uid of the best matching method, if any.
    pub fn resolve_method_by_receiver(
        &self,
        method_name: &str,
        receiver_expr: &str,
    ) -> Option<&str> {
        let entries = self.method_index.get(&method_name.to_lowercase())?;
        if entries.len() <= 1 {
            // No disambiguation needed — let the normal resolver handle it
            return None;
        }

        // Extract the type hint from the receiver expression.
        // For "myVar.field", we try matching "field" and "myVar" as potential type names.
        // For a single identifier like "client", we try matching it directly.
        let receiver_lower = receiver_expr.to_lowercase();
        let receiver_parts: Vec<&str> = receiver_expr.split('.').collect();

        // Try to resolve alias chain for the receiver
        let canonical = self.resolve_alias(&receiver_lower);
        // Also normalize the receiver for qname-aware matching
        let canonical_norm = self.normalize_type_name(canonical);

        // Score each candidate
        let mut best: Option<(usize, u32)> = None; // (index, score)
        for (i, entry) in entries.iter().enumerate() {
            let rt = match entry.receiver_type.as_ref() {
                Some(rt) => rt.to_lowercase(),
                None => continue,
            };
            let rt_canonical = self.resolve_alias(&rt);
            let rt_norm = self.normalize_type_name(rt_canonical);

            // Direct match with normalized receiver expression
            if rt_norm == canonical_norm {
                return Some(&entry.symbol_uid);
            }

            // Match against last part of dotted receiver
            let score = if receiver_parts
                .last()
                .map(|p| {
                    let p_norm = self.normalize_type_name(p);
                    p_norm == rt_norm
                })
                .unwrap_or(false)
            {
                3
            } else if receiver_parts
                .first()
                .map(|p| {
                    let p_norm = self.normalize_type_name(p);
                    p_norm == rt_norm
                })
                .unwrap_or(false)
            {
                2
            } else if self.is_subtype(canonical, rt_canonical) {
                1
            } else {
                0
            };

            if score > 0 && (best.is_none() || score > best.unwrap().1) {
                best = Some((i, score));
            }
        }

        best.map(|(i, _)| entries[i].symbol_uid.as_str())
    }

    /// Declared parameter count of a specific method symbol, if recorded.
    ///
    /// Used by the resolver's fuzzy arg-count narrowing to test individual
    /// candidates (unlike `resolve_method_by_arg_count`, which picks a single
    /// winner among all same-named methods).
    pub fn method_param_count(&self, method_name: &str, symbol_uid: &str) -> Option<u32> {
        self.method_index
            .get(&method_name.to_lowercase())?
            .iter()
            .find(|e| e.symbol_uid == symbol_uid)
            .and_then(|e| e.param_count)
    }

    /// Tri-state receiver compatibility (alias resolution, normalization,
    /// and a one-level subtype check): `None` means the method has no
    /// recorded receiver metadata (no evidence either way — callers narrowing
    /// a candidate pool must treat this as "cannot rule out"), `Some(bool)`
    /// is a positive verdict from the recorded receiver type.
    pub fn method_receiver_compat(
        &self,
        method_name: &str,
        symbol_uid: &str,
        receiver_expr: &str,
    ) -> Option<bool> {
        let entry = self
            .method_index
            .get(&method_name.to_lowercase())
            .and_then(|entries| entries.iter().find(|e| e.symbol_uid == symbol_uid))?;
        let rt = entry.receiver_type.as_ref()?.to_lowercase();
        let rt_norm = self.normalize_type_name(self.resolve_alias(&rt));

        let recv_lower = receiver_expr.to_lowercase();
        let recv_norm = self.normalize_type_name(self.resolve_alias(&recv_lower));
        if recv_norm == rt_norm {
            return Some(true);
        }
        // Dotted receivers ("self.client") may carry the type hint in any
        // segment; mirror resolve_method_by_receiver's segment matching.
        if receiver_expr
            .split('.')
            .any(|part| self.normalize_type_name(part) == rt_norm)
        {
            return Some(true);
        }
        Some(self.is_subtype(&recv_lower, &rt))
    }

    /// Resolve a method by argument count when multiple same-named methods exist.
    ///
    /// Returns the symbol_uid if exactly one candidate matches the given arg_count.
    pub fn resolve_method_by_arg_count(&self, method_name: &str, arg_count: u32) -> Option<&str> {
        let entries = self.method_index.get(&method_name.to_lowercase())?;
        if entries.len() <= 1 {
            return None;
        }

        let matches: Vec<&MethodEntry> = entries
            .iter()
            .filter(|e| e.param_count == Some(arg_count))
            .collect();

        if matches.len() == 1 {
            Some(&matches[0].symbol_uid)
        } else {
            None
        }
    }

    /// Chase type alias chain (up to 16 levels to prevent infinite loops).
    ///
    /// Returns the canonical type name (lowercase). If no alias exists, returns
    /// the input unchanged.
    pub fn resolve_alias<'a>(&'a self, type_name: &'a str) -> &'a str {
        let mut current = type_name;
        for _ in 0..16 {
            let next = self
                .type_aliases
                .get(current)
                .and_then(|pairs| pairs.last())
                .map(|(_, canonical)| canonical.as_str());
            match next {
                Some(next) if next != current => current = next,
                _ => break,
            }
        }
        current
    }

    /// Check whether `child` is a subtype of `parent` by walking the
    /// base_types and implements chains (one level deep — no transitive closure).
    pub fn is_subtype(&self, child: &str, parent: &str) -> bool {
        let child_norm = self.normalize_type_name(child);
        let parent_norm = self.normalize_type_name(parent);
        if child_norm == parent_norm {
            return true;
        }
        if let Some(info) = self.type_info(&child_norm) {
            for base in &info.base_types {
                let base_norm = self.normalize_type_name(base);
                if base_norm == parent_norm {
                    return true;
                }
            }
            for iface in &info.implements {
                let imp_norm = self.normalize_type_name(iface);
                if imp_norm == parent_norm {
                    return true;
                }
            }
        }
        false
    }

    /// Returns true if this catalog has any method entries.
    pub fn has_methods(&self) -> bool {
        !self.method_index.is_empty()
    }

    /// Populate the type_assign_index from parsed type assignment records.
    ///
    /// Call this after `build_from_symbols` during the indexing pipeline,
    /// feeding in `ParseOutcome::type_assigns` from each file.
    /// Last write wins for the same (file, var_name) pair.
    pub fn add_type_assigns(&mut self, assigns: &[TypeAssignRecord]) {
        for assign in assigns {
            self.type_assign_index.insert(
                (assign.file_path.clone(), assign.var_name.to_lowercase()),
                assign.type_name.clone(),
            );
        }
    }

    /// Resolve a variable name to its inferred type within a file.
    ///
    /// Returns the type name if a type assignment was recorded for the given
    /// file and variable name.
    pub fn resolve_var_type(&self, file_path: &str, var_name: &str) -> Option<&str> {
        self.type_assign_index
            .get(&(file_path.to_string(), var_name.to_lowercase()))
            .map(|s| s.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cc_model::symbol::{SymbolKind, SymbolRecord};
    use cc_model::ParserTier;

    fn make_method(
        name: &str,
        uid: &str,
        receiver_type: Option<&str>,
        param_count: Option<u32>,
    ) -> SymbolRecord {
        SymbolRecord {
            symbol_id: format!("sym-{}", uid),
            file_path: "test.go".to_string(),
            name: name.to_string(),
            kind: SymbolKind::Method,
            container: receiver_type.map(String::from),
            start_line: 1,
            end_line: 10,
            start_col: 0,
            end_col: 0,
            signature: None,
            doc: None,
            parser_tier: ParserTier::Heuristic,
            parser_confidence: 0.6,
            qname: Some(format!("{}.{}", receiver_type.unwrap_or(""), name)),
            parent_symbol_id: None,
            scope_id: None,
            export_name: None,
            is_default_export: false,
            symbol_uid: Some(uid.to_string()),
            framework_role: None,
            receiver_type: receiver_type.map(String::from),
            param_types: None,
            return_type: None,
            param_count,
            base_types: None,
            implements: None,
        }
    }

    fn make_class(name: &str, uid: &str, base_types: Option<&str>) -> SymbolRecord {
        make_class_with_qname(name, uid, base_types, Some(name))
    }

    fn make_class_with_qname(
        name: &str,
        uid: &str,
        base_types: Option<&str>,
        qname: Option<&str>,
    ) -> SymbolRecord {
        SymbolRecord {
            symbol_id: format!("sym-{}", uid),
            file_path: "test.go".to_string(),
            name: name.to_string(),
            kind: SymbolKind::Class,
            container: None,
            start_line: 1,
            end_line: 10,
            start_col: 0,
            end_col: 0,
            signature: None,
            doc: None,
            parser_tier: ParserTier::Heuristic,
            parser_confidence: 0.6,
            qname: qname.map(String::from),
            parent_symbol_id: None,
            scope_id: None,
            export_name: None,
            is_default_export: false,
            symbol_uid: Some(uid.to_string()),
            framework_role: None,
            receiver_type: None,
            param_types: None,
            return_type: None,
            param_count: None,
            base_types: base_types.map(String::from),
            implements: None,
        }
    }

    fn make_class_with_impls(
        name: &str,
        uid: &str,
        base_types: Option<&str>,
        implements: Option<&str>,
    ) -> SymbolRecord {
        let mut sym = make_class(name, uid, base_types);
        sym.implements = implements.map(String::from);
        sym
    }

    #[test]
    fn resolve_by_receiver_type() {
        let symbols = vec![
            make_method("parse", "uid-a-parse", Some("ParserA"), Some(1)),
            make_method("parse", "uid-b-parse", Some("ParserB"), Some(2)),
        ];
        let catalog = TypeCatalog::build_from_symbols(&symbols);

        assert_eq!(
            catalog.resolve_method_by_receiver("parse", "ParserA"),
            Some("uid-a-parse")
        );
        assert_eq!(
            catalog.resolve_method_by_receiver("parse", "ParserB"),
            Some("uid-b-parse")
        );
        // Unknown receiver should return None
        assert_eq!(catalog.resolve_method_by_receiver("parse", "Unknown"), None);
    }

    #[test]
    fn resolve_by_arg_count() {
        let symbols = vec![
            make_method("create", "uid-create-1", Some("Factory"), Some(1)),
            make_method("create", "uid-create-3", Some("Factory"), Some(3)),
        ];
        let catalog = TypeCatalog::build_from_symbols(&symbols);

        assert_eq!(
            catalog.resolve_method_by_arg_count("create", 1),
            Some("uid-create-1")
        );
        assert_eq!(
            catalog.resolve_method_by_arg_count("create", 3),
            Some("uid-create-3")
        );
        assert_eq!(catalog.resolve_method_by_arg_count("create", 2), None);
    }

    #[test]
    fn alias_chasing() {
        let mut symbols = vec![make_method("foo", "uid-foo", Some("RealType"), None)];
        // Add a type alias: AliasName -> RealType
        symbols.push(SymbolRecord {
            symbol_id: "sym-alias".into(),
            file_path: "test.go".into(),
            name: "AliasName".into(),
            kind: SymbolKind::TypeAlias,
            container: None,
            start_line: 1,
            end_line: 1,
            start_col: 0,
            end_col: 0,
            signature: None,
            doc: None,
            parser_tier: ParserTier::Heuristic,
            parser_confidence: 0.6,
            qname: Some("AliasName".into()),
            parent_symbol_id: None,
            scope_id: None,
            export_name: None,
            is_default_export: false,
            symbol_uid: Some("uid-alias".into()),
            framework_role: None,
            receiver_type: None,
            param_types: None,
            return_type: None,
            param_count: None,
            base_types: Some("RealType".into()),
            implements: None,
        });
        let catalog = TypeCatalog::build_from_symbols(&symbols);
        assert_eq!(catalog.resolve_alias("aliasname"), "realtype");
    }

    #[test]
    fn subtype_check() {
        let symbols = vec![
            make_class("Animal", "uid-animal", None),
            make_class("Dog", "uid-dog", Some("Animal")),
        ];
        let catalog = TypeCatalog::build_from_symbols(&symbols);
        assert!(catalog.is_subtype("Dog", "Animal"));
        assert!(!catalog.is_subtype("Animal", "Dog"));
        assert!(catalog.is_subtype("Dog", "Dog"));
    }

    #[test]
    fn single_method_skips_disambiguation() {
        // When only one method with the name exists, resolve_method_by_receiver returns None
        // so the normal resolver is used instead
        let symbols = vec![make_method("unique", "uid-unique", Some("Foo"), Some(0))];
        let catalog = TypeCatalog::build_from_symbols(&symbols);
        assert_eq!(catalog.resolve_method_by_receiver("unique", "Foo"), None);
    }

    // ----- New tests for qname-aware TypeCatalog -----

    #[test]
    fn qname_priority_in_type_index() {
        // Two classes with the same short name but different qnames
        let symbols = vec![
            make_class_with_qname("Client", "uid-a", None, Some("pkg_a.Client")),
            make_class_with_qname("Client", "uid-b", None, Some("pkg_b.Client")),
        ];
        let catalog = TypeCatalog::build_from_symbols(&symbols);

        // Both should be indexed by their qnames
        assert!(catalog.type_index_by_qname.contains_key("pkg_a.client"));
        assert!(catalog.type_index_by_qname.contains_key("pkg_b.client"));

        // Short name should map to both qnames
        let qnames = catalog.short_to_qnames.get("client").unwrap();
        assert_eq!(qnames.len(), 2);
    }

    #[test]
    fn normalize_strips_decorators() {
        let symbols = vec![make_class("MyType", "uid-mytype", None)];
        let catalog = TypeCatalog::build_from_symbols(&symbols);

        // Pointer/reference markers stripped
        assert_eq!(catalog.normalize_type_name("*MyType"), "mytype");
        assert_eq!(catalog.normalize_type_name("&MyType"), "mytype");

        // Generic params stripped
        assert_eq!(catalog.normalize_type_name("MyType<T>"), "mytype");

        // Array/optional markers stripped
        assert_eq!(catalog.normalize_type_name("MyType[]"), "mytype");
        assert_eq!(catalog.normalize_type_name("MyType?"), "mytype");
    }

    #[test]
    fn normalize_resolves_unique_short_name() {
        // One class with a unique short name
        let symbols = vec![make_class_with_qname(
            "Widget",
            "uid-w",
            None,
            Some("app.ui.Widget"),
        )];
        let catalog = TypeCatalog::build_from_symbols(&symbols);

        // Short name "widget" is unique -> maps to qname
        assert_eq!(catalog.normalize_type_name("Widget"), "app.ui.widget");
    }

    #[test]
    fn normalize_ambiguous_short_name_stays_short() {
        // Two classes with the same short name
        let symbols = vec![
            make_class_with_qname("Handler", "uid-h1", None, Some("http.Handler")),
            make_class_with_qname("Handler", "uid-h2", None, Some("grpc.Handler")),
        ];
        let catalog = TypeCatalog::build_from_symbols(&symbols);

        // Short name "handler" is ambiguous -> stays as-is
        assert_eq!(catalog.normalize_type_name("Handler"), "handler");
    }

    #[test]
    fn subtype_check_with_qname() {
        // Parent uses qname, child extends by short name
        let symbols = vec![
            make_class_with_qname("Base", "uid-base", None, Some("pkg.Base")),
            make_class_with_qname("Child", "uid-child", Some("Base"), Some("pkg.Child")),
        ];
        let catalog = TypeCatalog::build_from_symbols(&symbols);

        // "Child" extends "Base" — both short names are unique, so normalize maps them
        assert!(catalog.is_subtype("Child", "Base"));
        assert!(catalog.is_subtype("pkg.Child", "pkg.Base"));
        assert!(!catalog.is_subtype("Base", "Child"));
    }

    #[test]
    fn method_param_count_per_uid() {
        let symbols = vec![
            make_method("parse", "uid-parser-parse", Some("Parser"), Some(1)),
            make_method("parse", "uid-validator-parse", Some("Validator"), Some(2)),
            make_method("parse", "uid-untyped-parse", Some("Untyped"), None),
        ];
        let catalog = TypeCatalog::build_from_symbols(&symbols);

        assert_eq!(
            catalog.method_param_count("parse", "uid-parser-parse"),
            Some(1)
        );
        assert_eq!(
            catalog.method_param_count("parse", "uid-validator-parse"),
            Some(2)
        );
        // No recorded param count
        assert_eq!(
            catalog.method_param_count("parse", "uid-untyped-parse"),
            None
        );
        // Unknown uid / method name
        assert_eq!(catalog.method_param_count("parse", "uid-missing"), None);
        assert_eq!(
            catalog.method_param_count("missing", "uid-parser-parse"),
            None
        );
    }

    #[test]
    fn method_receiver_compat_specific_uid() {
        let symbols = vec![
            make_method("parse", "uid-parser-parse", Some("Parser"), Some(1)),
            make_method("parse", "uid-validator-parse", Some("Validator"), Some(1)),
            make_method("parse", "uid-untyped-parse", None, Some(1)),
        ];
        let catalog = TypeCatalog::build_from_symbols(&symbols);

        assert_eq!(
            catalog.method_receiver_compat("parse", "uid-parser-parse", "Parser"),
            Some(true)
        );
        assert_eq!(
            catalog.method_receiver_compat("parse", "uid-parser-parse", "Validator"),
            Some(false)
        );
        // Dotted receiver carrying the type hint in a segment
        assert_eq!(
            catalog.method_receiver_compat("parse", "uid-validator-parse", "self.Validator"),
            Some(true)
        );
        // No receiver metadata / unknown uid: no evidence either way
        assert_eq!(
            catalog.method_receiver_compat("parse", "uid-untyped-parse", "Parser"),
            None
        );
        assert_eq!(
            catalog.method_receiver_compat("parse", "uid-missing", "Parser"),
            None
        );
    }

    #[test]
    fn subtype_check_with_implements() {
        let symbols = vec![
            make_class_with_qname("Readable", "uid-readable", None, Some("io.Readable")),
            make_class_with_impls("FileReader", "uid-fr", None, Some("Readable")),
        ];
        let catalog = TypeCatalog::build_from_symbols(&symbols);

        assert!(catalog.is_subtype("FileReader", "Readable"));
        assert!(!catalog.is_subtype("Readable", "FileReader"));
    }

    // ----- Incremental maintenance (cross-build catalog cache) -----

    fn key_meta(sym: &SymbolRecord) -> SymbolKeyMeta<'_> {
        SymbolKeyMeta {
            name: &sym.name,
            qname: sym.qname.as_deref(),
            kind: sym.kind,
            symbol_uid: sym.symbol_uid.as_deref(),
        }
    }

    /// remove_files + add_symbol must be equivalent to a fresh rebuild over
    /// the surviving symbol set, across all four contribution maps.
    #[test]
    fn incremental_removal_matches_fresh_rebuild() {
        let mut removed_class = make_class("Dog", "uid-dog", Some("Animal"));
        removed_class.file_path = "dogs.go".to_string();
        let mut removed_method = make_method("bark", "uid-bark", Some("Dog"), Some(0));
        removed_method.file_path = "dogs.go".to_string();
        let kept_class = make_class("Animal", "uid-animal", None);
        let kept_method = make_method("feed", "uid-feed", Some("Animal"), Some(1));

        let all = vec![
            kept_class.clone(),
            removed_class.clone(),
            kept_method.clone(),
            removed_method.clone(),
        ];
        let mut catalog = TypeCatalog::build_from_symbols(&all);
        assert!(catalog.is_subtype("Dog", "Animal"));

        let removed_files: HashSet<String> = ["dogs.go".to_string()].into();
        catalog.remove_files(
            &[key_meta(&removed_class), key_meta(&removed_method)],
            &removed_files,
        );

        // Subtype edge through the removed class is gone; kept data intact.
        assert!(!catalog.is_subtype("Dog", "Animal"));
        assert!(catalog.method_param_count("feed", "uid-feed").is_some());
        assert!(catalog.method_param_count("bark", "uid-bark").is_none());

        // Bucket-level equivalence with a fresh rebuild over the survivors.
        let fresh = TypeCatalog::build_from_symbols([&kept_class, &kept_method]);
        assert_eq!(
            catalog.method_index.keys().collect::<HashSet<_>>(),
            fresh.method_index.keys().collect::<HashSet<_>>()
        );
        assert_eq!(
            catalog.type_index_by_qname.keys().collect::<HashSet<_>>(),
            fresh.type_index_by_qname.keys().collect::<HashSet<_>>()
        );
        assert_eq!(
            catalog.short_to_qnames.keys().collect::<HashSet<_>>(),
            fresh.short_to_qnames.keys().collect::<HashSet<_>>()
        );
    }

    /// Removing one file's contribution must not erase another file's
    /// same-key contribution (the multimap exactness this refactor exists for).
    #[test]
    fn removal_keeps_same_key_contribution_from_other_file() {
        let mut cfg_a = make_class_with_qname("Config", "uid-cfg-a", Some("BaseA"), Some("Config"));
        cfg_a.file_path = "a.py".to_string();
        let mut cfg_b = make_class_with_qname("Config", "uid-cfg-b", Some("BaseB"), Some("Config"));
        cfg_b.file_path = "b.py".to_string();
        let base_a = make_class("BaseA", "uid-base-a", None);
        let base_b = make_class("BaseB", "uid-base-b", None);

        let mut catalog =
            TypeCatalog::build_from_symbols([&base_a, &base_b, &cfg_a, &cfg_b]);
        // Last writer (b.py) is live.
        assert!(catalog.is_subtype("Config", "BaseB"));

        let removed_files: HashSet<String> = ["b.py".to_string()].into();
        catalog.remove_files(&[key_meta(&cfg_b)], &removed_files);

        // a.py's contribution becomes live instead of vanishing.
        assert!(catalog.is_subtype("Config", "BaseA"));
        assert!(!catalog.is_subtype("Config", "BaseB"));
    }

    /// Alias removal restores the surviving file's alias target.
    #[test]
    fn alias_removal_restores_prior_contribution() {
        let mut alias_a = make_class("Ignored", "uid-x", None);
        alias_a.kind = SymbolKind::TypeAlias;
        alias_a.name = "Handle".to_string();
        alias_a.qname = Some("Handle".to_string());
        alias_a.base_types = Some("RealA".to_string());
        alias_a.file_path = "a.rs".to_string();
        let mut alias_b = alias_a.clone();
        alias_b.symbol_uid = Some("uid-y".to_string());
        alias_b.base_types = Some("RealB".to_string());
        alias_b.file_path = "b.rs".to_string();

        let mut catalog = TypeCatalog::build_from_symbols([&alias_a, &alias_b]);
        assert_eq!(catalog.resolve_alias("handle"), "realb");

        let removed_files: HashSet<String> = ["b.rs".to_string()].into();
        catalog.remove_files(&[key_meta(&alias_b)], &removed_files);
        assert_eq!(catalog.resolve_alias("handle"), "reala");
    }

    /// reset_type_assigns drops variable inferences without touching the
    /// declaration-derived maps.
    #[test]
    fn reset_type_assigns_is_scoped() {
        let symbols = vec![make_method("run", "uid-run", Some("Job"), Some(0))];
        let mut catalog = TypeCatalog::build_from_symbols(&symbols);
        catalog.add_type_assigns(&[TypeAssignRecord {
            file_path: "main.py".to_string(),
            enclosing_symbol_uid: None,
            var_name: "job".to_string(),
            type_name: "Job".to_string(),
            line: 3,
            confidence: 0.9,
            source: cc_model::type_assign::TypeAssignSource::Constructor,
        }]);
        assert_eq!(catalog.resolve_var_type("main.py", "job"), Some("Job"));

        catalog.reset_type_assigns();
        assert_eq!(catalog.resolve_var_type("main.py", "job"), None);
        assert!(catalog.has_methods());
    }

}
