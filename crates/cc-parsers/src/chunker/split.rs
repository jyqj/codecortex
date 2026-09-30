//! Hierarchical partitioning; exact slices, explicit fallback and bounded traversal.
use super::budget::Budget;
use cc_model::chunk_policy::ChunkPolicy;
use cc_model::source::*;
use std::collections::BTreeSet;
#[derive(Clone)]
pub(super) struct Piece {
    pub span: ByteSpan,
    pub owner: Option<usize>,
    pub domain: usize,
    pub mergeable: bool,
    pub boundary: &'static str,
}
pub(super) struct Partition<'a, 's> {
    pub source: &'a SourceSnapshot<'s>,
    pub structure: &'a SourceStructure,
    pub budget: Budget,
    pub children: Vec<Vec<usize>>,
    pub has_member: Vec<bool>,
    pub has_control: Vec<bool>,
    pub pieces: Vec<Piece>,
}
impl<'a, 's> Partition<'a, 's> {
    pub fn new(
        source: &'a SourceSnapshot<'s>,
        structure: &'a SourceStructure,
        policy: ChunkPolicy,
    ) -> Self {
        let mut children = vec![vec![]; structure.boundaries.len() + 1];
        for (i, b) in structure.boundaries.iter().enumerate() {
            let p = b.parent.map_or(structure.boundaries.len(), |p| p as usize);
            if p < children.len() {
                children[p].push(i);
            }
        }
        let mut has_member = vec![false; children.len()];
        let mut has_control = vec![false; children.len()];
        for i in (0..structure.boundaries.len()).rev() {
            let b = &structure.boundaries[i];
            let parent = b.parent.map_or(structure.boundaries.len(), |p| p as usize);
            has_member[parent] |= has_member[i] || b.kind == BoundaryKind::Symbol;
            has_control[parent] |=
                has_control[i] || matches!(b.kind, BoundaryKind::Control | BoundaryKind::Block);
        }
        Self {
            source,
            structure,
            budget: Budget::new(policy),
            children,
            has_member,
            has_control,
            pieces: vec![],
        }
    }
    fn fits(&self, span: ByteSpan) -> bool {
        self.budget.fits(self.source, span)
    }
    pub fn run(mut self) -> Vec<Piece> {
        if self.source.bytes().is_empty() {
            return self.pieces;
        }
        self.region(
            self.source.whole(),
            self.structure.boundaries.len(),
            None,
            true,
            0,
        );
        self.pieces
    }
    fn region(
        &mut self,
        span: ByteSpan,
        node: usize,
        owner: Option<usize>,
        root: bool,
        depth: usize,
    ) {
        if span.is_empty() {
            return;
        }
        // A container's methods remain individually retrievable even when the
        // class fits the budget. Leaf functions stay whole; no duplicate parent
        // body is emitted merely to retain the class name (the header owns it).
        let container_members = owner.is_some_and(|i| {
            matches!(
                self.structure.boundaries[i].symbol_kind,
                Some(
                    cc_model::symbol::SymbolKind::Class
                        | cc_model::symbol::SymbolKind::Interface
                        | cc_model::symbol::SymbolKind::Module
                        | cc_model::symbol::SymbolKind::Namespace
                        | cc_model::symbol::SymbolKind::Enum
                )
            )
        }) && self.has_member[node];
        if !root && !container_members && self.fits(span) {
            self.pieces.push(Piece {
                span,
                owner,
                domain: self.structure.boundaries[node]
                    .parent
                    .map_or(self.structure.boundaries.len(), |p| p as usize),
                mergeable: self.structure.boundaries[node].kind == BoundaryKind::Statement
                    && !self.has_control[node],
                boundary: if owner == Some(node) {
                    "symbol"
                } else {
                    "ast_block"
                },
            });
            return;
        }
        if depth > 512 {
            self.fallback(span, owner, node);
            return;
        }
        if !root && owner == Some(node) {
            let boundary = &self.structure.boundaries[node];
            if let Some(doc) = boundary.documentation {
                if span.start <= boundary.span.start && doc.end > span.start && doc.end < span.end {
                    let prefix = ByteSpan {
                        start: span.start,
                        end: doc.end,
                    };
                    if self.fits(prefix) {
                        self.pieces.push(Piece {
                            span: prefix,
                            owner,
                            boundary: "signature_documentation",
                            domain: node,
                            mergeable: false,
                        });
                        self.region(
                            ByteSpan {
                                start: doc.end,
                                end: span.end,
                            },
                            node,
                            owner,
                            false,
                            depth + 1,
                        );
                        return;
                    }
                }
            }
        }
        let children = self.children[node].clone();
        let mut relevant: Vec<(usize, ByteSpan)> = Vec::new();
        let mut claimed_comments = BTreeSet::new();
        for &i in &children {
            let b = &self.structure.boundaries[i];
            if b.kind == BoundaryKind::Symbol {
                if let Some(doc) = b.leading_comment {
                    claimed_comments.insert((doc.start, doc.end));
                }
            }
        }
        for i in children {
            let b = &self.structure.boundaries[i];
            if b.kind == BoundaryKind::Comment
                && claimed_comments
                    .iter()
                    .any(|&(a, z)| a <= b.span.start && b.span.end <= z)
            {
                continue;
            }
            let mut s = b.span;
            if let Some(doc) = b.leading_comment {
                s.start = doc.start;
            }
            if b.kind == BoundaryKind::Symbol {
                if let Ok((line, _)) = self.source.point(s.start) {
                    let a = self.source.line_start(line).unwrap();
                    if self
                        .source
                        .slice(ByteSpan {
                            start: a,
                            end: s.start,
                        })
                        .is_ok_and(|t| t.trim().is_empty())
                    {
                        s.start = a;
                    }
                }
            }
            s.start = s.start.max(span.start);
            s.end = s.end.min(span.end);
            if !s.is_empty() && span.contains(s) {
                // Adjacent sibling comment nodes form one documentation unit.
                // Do not cross a blank source line, declaration or parent scope.
                if b.kind == BoundaryKind::Comment {
                    if let Some((previous, previous_span)) = relevant.last_mut() {
                        let joined = ByteSpan {
                            start: previous_span.start,
                            end: s.end,
                        };
                        if self.structure.boundaries[*previous].kind == BoundaryKind::Comment
                            && previous_span.end <= s.start
                            && self
                                .source
                                .slice(ByteSpan {
                                    start: previous_span.end,
                                    end: s.start,
                                })
                                .is_ok_and(|gap| {
                                    gap.trim().is_empty()
                                        && gap.bytes().filter(|b| *b == b'\n').count() <= 1
                                })
                            && self.fits(joined)
                        {
                            previous_span.end = s.end;
                            continue;
                        }
                    }
                }
                relevant.push((i, s));
            }
        }
        relevant.sort_by_key(|(_, s)| (s.start, std::cmp::Reverse(s.end)));
        if relevant.is_empty() {
            self.fallback(span, owner, node);
            return;
        }
        let mut pos = span.start;
        for (i, mut child) in relevant {
            if child.end <= pos {
                continue;
            }
            child.start = child.start.max(pos);
            if child.start > pos {
                let gap = ByteSpan {
                    start: pos,
                    end: child.start,
                };
                if self.source.slice(gap).is_ok_and(|s| s.trim().is_empty()) {
                    child.start = pos;
                } else {
                    self.fallback(gap, owner, node);
                }
            }
            let b = &self.structure.boundaries[i];
            let child_owner = if b.kind == BoundaryKind::Symbol {
                Some(i)
            } else {
                owner
            };
            self.region(child, i, child_owner, false, depth + 1);
            pos = child.end;
        }
        if pos < span.end {
            let tail = ByteSpan {
                start: pos,
                end: span.end,
            };
            if self.source.slice(tail).is_ok_and(|t| t.trim().is_empty()) {
                if let Some(prev) = self.pieces.last() {
                    let joined = ByteSpan {
                        start: prev.span.start,
                        end: span.end,
                    };
                    if prev.span.end == pos && self.fits(joined) {
                        self.pieces.last_mut().unwrap().span.end = span.end;
                        return;
                    }
                }
            }
            self.fallback(tail, owner, node);
        }
    }
    fn fallback(&mut self, span: ByteSpan, owner: Option<usize>, domain: usize) {
        super::fallback::append(
            self.source,
            self.budget,
            span,
            owner,
            domain,
            &mut self.pieces,
        );
    }
}
