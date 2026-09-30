//! One budget predicate and maximal UTF-8/CRLF-safe prefix for all producers.
use cc_model::{
    chunk_policy::ChunkPolicy,
    source::{ByteSpan, SourceSnapshot},
};
#[derive(Clone, Copy)]
pub(super) struct Budget {
    pub policy: ChunkPolicy,
}
impl Budget {
    pub fn new(policy: ChunkPolicy) -> Self {
        Self {
            policy: policy.bounded(),
        }
    }
    pub fn fits(self, source: &SourceSnapshot<'_>, span: ByteSpan) -> bool {
        if span.len() > self.policy.effective_bytes() {
            return false;
        }
        source
            .lines(span)
            .is_ok_and(|(a, z)| z - a < self.policy.lines)
            && source.slice(span).is_ok_and(|s| {
                s.len() <= self.policy.chars as usize
                    || s.chars().take(self.policy.chars as usize + 1).count()
                        <= self.policy.chars as usize
            })
    }
    pub fn prefix_end(self, source: &SourceSnapshot<'_>, span: ByteSpan) -> usize {
        let first = source.point(span.start).expect("validated UTF-8 span").0;
        let line_end = source
            .line_end(first.saturating_add(self.policy.lines as usize - 1))
            .unwrap_or(span.end);
        let text = source.text().expect("UTF-8 source");
        let mut end = span
            .end
            .min(line_end)
            .min(span.start.saturating_add(self.policy.effective_bytes()));
        while end > span.start && !text.is_char_boundary(end) {
            end -= 1;
        }
        if end - span.start > self.policy.chars as usize {
            if let Some((offset, _)) = text[span.start..end]
                .char_indices()
                .nth(self.policy.chars as usize)
            {
                end = span.start + offset;
            }
        }
        if end > span.start
            && end < span.end
            && source.bytes()[end - 1] == b'\r'
            && source.bytes()[end] == b'\n'
        {
            end -= 1;
        }
        // A four-byte scalar or two-byte CRLF fits every valid policy.
        assert!(end > span.start, "valid chunk policy must make progress");
        end
    }
}
