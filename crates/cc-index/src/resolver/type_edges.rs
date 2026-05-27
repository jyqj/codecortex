//! Type edge derivation (USES_TYPE).

use std::collections::HashSet;

use cc_model::edge::{SemanticEdgeRecord, SemanticRelation};
use cc_model::parse::ParseOutcome;

use super::catalog::SymbolCatalog;
use super::helpers::*;

impl SymbolCatalog {
    // -----------------------------------------------------------------------
    // USES_TYPE derivation
    // -----------------------------------------------------------------------

    /// Try to resolve a type atom name to a symbol UID via the catalog.
    pub(in crate::resolver) fn resolve_type_atom(
        &self,
        atom: &str,
        file_path: &str,
    ) -> Option<String> {
        // 1. Try same-file match (type-like kinds preferred)
        if let Some(idx) = self.find_by_name_in_file(atom, file_path, true) {
            return self.entries[idx].symbol_uid.clone();
        }

        // 2. Try global unique match for type-like symbols
        let lower = atom.to_lowercase();
        if let Some(indices) = self.by_name.get(&lower) {
            let type_matches: Vec<usize> = indices
                .iter()
                .copied()
                .filter(|&i| is_type_like(self.entries[i].kind))
                .collect();

            if type_matches.len() == 1 {
                return self.entries[type_matches[0]].symbol_uid.clone();
            }
        }

        None
    }

    /// Derive USES_TYPE edges from symbol type fields (receiver_type, param_types, return_type).
    ///
    /// For each symbol with type annotations, creates semantic edges to the referenced types.
    /// This avoids needing each parser to explicitly extract type usage relationships.
    pub fn derive_uses_type_edges(&self, file_path: &str, outcome: &mut ParseOutcome) {
        let mut seen: HashSet<(String, String)> = HashSet::new(); // (source_uid, target_name)
        let mut new_edges: Vec<SemanticEdgeRecord> = Vec::new();

        for symbol in &outcome.symbols {
            let source_uid = match symbol.symbol_uid.as_ref() {
                Some(uid) => uid.clone(),
                None => continue,
            };

            // Collect type strings to process
            let type_strings: Vec<&str> = [
                symbol.receiver_type.as_deref(),
                symbol.param_types.as_deref(),
                symbol.return_type.as_deref(),
            ]
            .iter()
            .filter_map(|o| *o)
            .collect();

            for type_str in type_strings {
                for atom in type_atoms(type_str) {
                    let key = (source_uid.clone(), atom.clone());
                    if seen.contains(&key) {
                        continue;
                    }
                    seen.insert(key);

                    // Try to resolve the type atom to a catalog entry
                    let target_uid = self.resolve_type_atom(&atom, file_path);

                    new_edges.push(SemanticEdgeRecord {
                        edge_id: format!(
                            "se-{}:{}:uses_type:{}",
                            file_path, symbol.start_line, atom
                        ),
                        file_path: file_path.to_string(),
                        source_symbol: symbol.name.clone(),
                        source_symbol_uid: Some(source_uid.clone()),
                        target_symbol: atom,
                        target_symbol_uid: target_uid,
                        relation_kind: SemanticRelation::UsesType,
                        line: symbol.start_line,
                        confidence: 0.8, // derived, not declared
                        parser_tier: symbol.parser_tier,
                    });
                }
            }
        }

        if !new_edges.is_empty() {
            tracing::debug!(
                file = file_path,
                count = new_edges.len(),
                "derived USES_TYPE edges from type annotations"
            );
            outcome.semantic_edges.extend(new_edges);
        }
    }
}

impl Default for SymbolCatalog {
    fn default() -> Self {
        Self::new()
    }
}
