//! Exact source fallback shared by symbols, gaps, tails and non-AST input.
use super::{budget::Budget, split::Piece};
use cc_model::source::{ByteSpan, SourceSnapshot};
pub(super) fn append(
    source: &SourceSnapshot<'_>,
    budget: Budget,
    span: ByteSpan,
    owner: Option<usize>,
    domain: usize,
    pieces: &mut Vec<Piece>,
) {
    let mut start = span.start;
    while start < span.end {
        let end = budget.prefix_end(
            source,
            ByteSpan {
                start,
                end: span.end,
            },
        );
        pieces.push(Piece {
            span: ByteSpan { start, end },
            owner,
            domain,
            mergeable: true,
            boundary: "bounded_fallback",
        });
        start = end;
    }
}
