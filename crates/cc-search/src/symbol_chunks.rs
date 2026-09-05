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
