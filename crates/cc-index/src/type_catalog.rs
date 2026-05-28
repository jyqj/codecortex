//! Lightweight type catalog for method dispatch resolution.
//!
//! Stores type information extracted from parsed symbols to improve
//! call graph accuracy when multiple symbols share the same name.
//! This is **not** a full type system — it only indexes information
//! directly visible in declarations (receiver types, parameter counts,
//! base types) and uses it for disambiguation during resolution.

use std::collections::HashMap;

use cc_model::symbol::SymbolRecord;
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
    /// canonical qname (lowercase) -> TypeInfo
    type_index_by_qname: HashMap<String, TypeInfo>,
    /// short name (lowercase) -> list of canonical qnames (lowercase)
    short_to_qnames: HashMap<String, Vec<String>>,
    /// alias_name (lowercase) -> canonical_name (lowercase)
    type_aliases: HashMap<String, String>,
    /// Maps (file_path, var_name_lowercase) -> type_name for local variable type inference.
    type_assign_index: HashMap<(String, String), String>,
}

impl TypeCatalog {
    /// Build a TypeCatalog from a slice of parsed symbols.
    pub fn build_from_symbols(symbols: &[SymbolRecord]) -> Self {
        let mut method_index: HashMap<String, Vec<MethodEntry>> = HashMap::new();
        let mut type_index_by_qname: HashMap<String, TypeInfo> = HashMap::new();
        let mut short_to_qnames: HashMap<String, Vec<String>> = HashMap::new();
        let mut type_aliases: HashMap<String, String> = HashMap::new();

        for sym in symbols {
            let uid = match sym.symbol_uid.as_ref() {
                Some(u) => u.clone(),
                None => continue,
            };

            match sym.kind {
                cc_model::symbol::SymbolKind::Method | cc_model::symbol::SymbolKind::Function => {
                    let entry = MethodEntry {
                        symbol_uid: uid,
                        receiver_type: sym.receiver_type.clone(),
                        param_count: sym.param_count,
                    };
                    method_index
                        .entry(sym.name.to_lowercase())
                        .or_default()
                        .push(entry);
                }
                cc_model::symbol::SymbolKind::Class
                | cc_model::symbol::SymbolKind::Interface
                | cc_model::symbol::SymbolKind::Enum => {
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

                    type_index_by_qname.insert(
                        canonical_key.clone(),
                        TypeInfo {
                            base_types: base,
                            implements: impls,
                        },
                    );

                    // Maintain short name -> qnames mapping
                    let short_key = sym.name.to_lowercase();
                    let qnames = short_to_qnames.entry(short_key).or_default();
                    if !qnames.contains(&canonical_key) {
                        qnames.push(canonical_key);
                    }
                }
                cc_model::symbol::SymbolKind::TypeAlias => {
                    // For type aliases, if we have base_types, use the first as the canonical
                    if let Some(ref bt) = sym.base_types {
                        let first = bt.split(',').next().unwrap_or("").trim();
                        if !first.is_empty() {
                            type_aliases.insert(sym.name.to_lowercase(), first.to_lowercase());
                        }
                    }
                }
                _ => {}
            }
        }

        Self {
            method_index,
            type_index_by_qname,
            short_to_qnames,
            type_aliases,
            type_assign_index: HashMap::new(),
        }
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

        // Map short name to qname if unique
        if let Some(qnames) = self.short_to_qnames.get(resolved) {
            if qnames.len() == 1 {
                return qnames[0].clone();
            }
        }

        // Check if it's already a qname in the index
        if self.type_index_by_qname.contains_key(resolved) {
            return resolved.to_string();
        }

        resolved.to_string()
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
            match self.type_aliases.get(current) {
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
        if let Some(info) = self.type_index_by_qname.get(&child_norm) {
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
    fn subtype_check_with_implements() {
        let symbols = vec![
            make_class_with_qname("Readable", "uid-readable", None, Some("io.Readable")),
            make_class_with_impls("FileReader", "uid-fr", None, Some("Readable")),
        ];
        let catalog = TypeCatalog::build_from_symbols(&symbols);

        assert!(catalog.is_subtype("FileReader", "Readable"));
        assert!(!catalog.is_subtype("Readable", "FileReader"));
    }
}
