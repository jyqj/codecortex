//! Bounded context metadata and same-domain fragment coalescing.
use cc_model::source::{BoundaryKind, SourceStructure};
/// Original order is retained; only adjacent intervals may be joined. A symbol
/// or documentation boundary is a barrier, even when it shares a source line.
pub(super) fn coalesce(
    source: &cc_model::source::SourceSnapshot<'_>,
    budget: super::budget::Budget,
    pieces: Vec<super::split::Piece>,
) -> Vec<super::split::Piece> {
    let mut result: Vec<super::split::Piece> = Vec::with_capacity(pieces.len());
    for next in pieces {
        if let Some(last) = result.last_mut() {
            let joined = cc_model::source::ByteSpan {
                start: last.span.start,
                end: next.span.end,
            };
            if budget.policy.merge_min_bytes > 0
                && last.span.end == next.span.start
                && last.owner == next.owner
                && last.domain == next.domain
                && last.mergeable
                && next.mergeable
                && last.span.len().min(next.span.len()) < (budget.policy.merge_min_bytes as usize)
                && budget.fits(source, joined)
            {
                last.span = joined;
                last.boundary = "merged_same_domain";
                continue;
            }
        }
        result.push(next);
    }
    result
}

/// Declared direct members, computed once per file, never synthesized from a query.
/// These labels are retrieval metadata, not injected into original source text.
pub(super) fn members(structure: &SourceStructure) -> std::collections::BTreeMap<usize, String> {
    use cc_model::symbol::SymbolKind;
    let mut owners = vec![None; structure.boundaries.len()];
    let mut labels: std::collections::BTreeMap<usize, Vec<&str>> = Default::default();
    for (i, b) in structure.boundaries.iter().enumerate() {
        let parent = b.parent.and_then(|p| owners[p as usize]);
        owners[i] = if b.kind == BoundaryKind::Symbol {
            Some(i)
        } else {
            parent
        };
        if b.kind != BoundaryKind::Symbol {
            continue;
        }
        if let (Some(parent), Some(name)) = (parent, b.name.as_deref()) {
            if matches!(
                structure.boundaries[parent].symbol_kind,
                Some(
                    SymbolKind::Class
                        | SymbolKind::Interface
                        | SymbolKind::Module
                        | SymbolKind::Namespace
                        | SymbolKind::Enum
                )
            ) {
                let names = labels.entry(parent).or_default();
                if names.len() < 16 && !names.contains(&name) {
                    names.push(name);
                }
            }
        }
    }
    labels
        .into_iter()
        .map(|(i, names)| (i, names.join(", ")))
        .collect()
}
pub(super) fn breadcrumb(
    structure: &SourceStructure,
    owner: Option<usize>,
    members: &std::collections::BTreeMap<usize, String>,
) -> String {
    let mut names = Vec::new();
    let mut node = owner;
    for _ in 0..512 {
        let Some(i) = node else {
            break;
        };
        let Some(b) = structure.boundaries.get(i) else {
            break;
        };
        if b.kind == BoundaryKind::Symbol {
            if let Some(name) = &b.name {
                names.push(name.as_str());
            }
        }
        node = b.parent.map(|i| i as usize);
    }
    names.reverse();
    let mut result = names.join(" > ");
    if let Some(label) = owner.and_then(|i| members.get(&i)) {
        result.push_str(" [members: ");
        result.push_str(label);
        result.push(']');
    }
    // Separate bounded metadata. Chunk source and parent/signature spans stay exact.
    if result.len() > 1024 {
        let mut end = 1021;
        while !result.is_char_boundary(end) {
            end -= 1;
        }
        result.truncate(end);
        result.push_str("...");
    }
    result
}
