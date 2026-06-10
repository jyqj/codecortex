//! Shared semantics declaration for variable-length Cypher traversals.
//!
//! Two engines execute `-[:T*min..max]->` segments: the SQL `WITH RECURSIVE`
//! CTE in `executor::translate_variable_length` and the lazy BFS in
//! `fast_path` (ADR 0001). Their observable behavior must stay row-for-row
//! identical, which used to be a manual synchronisation obligation between
//! the two implementations. This module is the single place that *declares*
//! the traversal semantics; each engine consumes the declaration through
//! exhaustive `match`es (its mechanical mapping). The compile-time guarantee
//! is direct for three of the rules and indirect for direction: a new
//! variant on `TupleMultiplicity`, `CyclePolicy` or `ProjectionDedup` fails
//! compilation at the consuming sites of both engines at once, while a new
//! `DirectionHandling` variant directly breaks only `orient()` and the
//! fast-path gate tether — the engines observe direction through the
//! `WalkOrientation` that `orient()` returns, so the new arm in `orient()`
//! must be mapped deliberately (give it a new `WalkOrientation` variant when
//! the walk differs, which then does break both engines' walk mappings).

use super::ast::RelDirection;

/// How a variable-length segment treats the arrow in the pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DirectionHandling {
    /// COMPATIBILITY QUIRK: the SQL CTE has always walked edges from the
    /// pattern's textual source node to its destination node, regardless of
    /// whether the segment is spelled `->`, `<-` or `--`. The quirk is kept
    /// so existing queries do not change results. Fixing it means adding a
    /// variant here (e.g. `RespectArrow`) and writing its arm in `orient()`
    /// — note this constrains the engines only *indirectly*: they consume
    /// the `WalkOrientation` that `orient()` returns, so the new arm must
    /// map to a new `WalkOrientation` variant when the walk differs (that
    /// variant then breaks both engines' walk mappings directly).
    IgnoreDirection,
}

/// The orientation an engine actually walks, after `DirectionHandling` is
/// applied to the pattern's arrow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WalkOrientation {
    /// Follow edges from their source column to their destination column
    /// (e.g. caller -> callee for CALLS).
    Forward,
}

/// How many traversal tuples a node reachable along several paths produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TupleMultiplicity {
    /// One tuple per distinct `(root, node, depth)` — the CTE's `UNION`
    /// dedup. A node re-reached at a *deeper* depth is a new tuple (so a
    /// cycle re-reaches the root at depth >= 1 and `min`-depth filters see
    /// deeper re-visits); re-reaching at the *same* depth is not.
    DistinctPerRootNodeDepth,
}

/// How cycles terminate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CyclePolicy {
    /// No path-based cycle pruning: expansion stops only at the `max_hops`
    /// depth cap (`pc.depth < max` in the CTE, the depth loop in the BFS).
    BoundedByMaxHops,
}

/// How projected output rows are deduplicated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProjectionDedup {
    /// `SELECT DISTINCT` over the projected columns: identical projected
    /// rows collapse to one even when they come from different tuples.
    DistinctRows,
}

/// The full semantics declaration for variable-length traversal segments.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TraversalSemantics {
    pub(crate) direction: DirectionHandling,
    pub(crate) tuple_multiplicity: TupleMultiplicity,
    pub(crate) cycle_policy: CyclePolicy,
    pub(crate) projection_dedup: ProjectionDedup,
}

/// The single shared declaration both engines consume.
pub(crate) const VARLEN_TRAVERSAL: TraversalSemantics = TraversalSemantics {
    direction: DirectionHandling::IgnoreDirection,
    tuple_multiplicity: TupleMultiplicity::DistinctPerRootNodeDepth,
    cycle_policy: CyclePolicy::BoundedByMaxHops,
    projection_dedup: ProjectionDedup::DistinctRows,
};

impl TraversalSemantics {
    /// Map the pattern's arrow to the orientation the engine must walk.
    pub(crate) fn orient(&self, pattern_direction: RelDirection) -> WalkOrientation {
        match self.direction {
            // The arrow is intentionally ignored: every spelling walks
            // forward in pattern order.
            DirectionHandling::IgnoreDirection => {
                let _ = pattern_direction;
                WalkOrientation::Forward
            }
        }
    }
}
