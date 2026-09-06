//! Symbol-to-source projection independent of graph retrieval and SQL.
//!
//! A short symbol selects its smallest containing chunk. A split symbol has
//! no containing chunk: return intersecting pieces in source order instead
//! of silently losing a symbol already found in the graph.

pub(crate) fn project_symbol_chunks(
    spans: &[(String, u32, u32)],
    start: u32,
    end: u32,
) -> Vec<&str> {
    if start > end {
        return Vec::new();
    }
    if let Some((id, _, _)) = spans
        .iter()
        .filter(|(_, cs, ce)| *cs <= start && *ce >= end)
        .min_by(|a, b| (a.2 - a.1).cmp(&(b.2 - b.1)).then_with(|| a.0.cmp(&b.0)))
    {
        return vec![id.as_str()];
    }
    let mut pieces: Vec<_> = spans
        .iter()
        .filter(|(_, cs, ce)| *cs <= *ce && *cs <= end && *ce >= start)
        .collect();
    pieces.sort_by(|a, b| {
        a.1.cmp(&b.1)
            .then_with(|| a.2.cmp(&b.2))
            .then_with(|| a.0.cmp(&b.0))
    });
    pieces.into_iter().map(|(id, _, _)| id.as_str()).collect()
}

/// Reverse projection. A named hit must resolve to that name at an intersecting
/// span, never to an earlier outer container. Without a name use the smallest
/// full container. Equal-span distinct identities are ambiguous, not ordered by
/// SQL insertion order. Returns no guessed symbol for multi-symbol file slices.
pub(crate) fn symbol_for_chunk<'a>(
    symbols: &'a [cc_db::index_db::SymbolRow],
    hit: &cc_model::search::SearchHit,
) -> Option<&'a cc_db::index_db::SymbolRow> {
    let mut candidates: Vec<_> = symbols
        .iter()
        .filter(|s| {
            s.file_path == hit.file_path
                && s.symbol_uid.is_some()
                && s.start_line <= s.end_line
                && match &hit.symbol_name {
                    Some(name) => {
                        s.name == *name
                            && s.start_line <= hit.end_line
                            && s.end_line >= hit.start_line
                    }
                    None => s.start_line <= hit.start_line && s.end_line >= hit.end_line,
                }
        })
        .collect();
    candidates.sort_by_key(|s| (s.end_line - s.start_line, s.start_line));
    let first = *candidates.first()?;
    if candidates.iter().skip(1).any(|s| {
        s.end_line - s.start_line == first.end_line - first.start_line
            && s.start_line == first.start_line
            && s.symbol_uid != first.symbol_uid
    }) {
        return None;
    }
    Some(first)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn long_symbol_survives_chunk_boundaries() {
        let spans = vec![("second".into(), 81, 160), ("first".into(), 1, 80)];
        assert_eq!(project_symbol_chunks(&spans, 1, 160), ["first", "second"]);
        assert_eq!(project_symbol_chunks(&spans, 79, 82), ["first", "second"]);
    }
    #[test]
    fn smallest_container_and_stable_ties() {
        let spans = vec![
            ("large".into(), 1, 200),
            ("b".into(), 10, 20),
            ("a".into(), 10, 20),
        ];
        assert_eq!(project_symbol_chunks(&spans, 11, 19), ["a"]);
        assert!(project_symbol_chunks(&spans, 20, 10).is_empty());
        assert!(project_symbol_chunks(&spans, 300, 400).is_empty());
    }
}
