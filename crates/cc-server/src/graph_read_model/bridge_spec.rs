//! Declaration point for read-side virtual bridge edges.
//!
//! `cc_index::dispatch_synthesis::registry()` owns build-time synthesized
//! edges; the HTTP/async bridges are read-side *virtual* projections — not
//! persisted, recomputed per [`super::cache::GraphReadGeneration`] from
//! `http_call_edges` + `routes`. They used to bypass any declaration: their
//! `dispatch_kind` strings (`http_bridge` / `async_bridge`) appeared only as
//! literals in [`super::bridges`], so adding a third bridge kind could silently
//! escape the catalog's closed-set guarantees and the GraphExplain /
//! disable-cleanup machinery that keys off `synthesized_by`.
//!
//! This module is the parallel declaration point: a closed registry plus the
//! single `call_kind → dispatch_kind` mapping ([`dispatch_kind_for`]) that
//! [`super::bridges`] consumes, so the set of virtual bridge kinds is closed
//! and machine-checked (see `registry_pins_bridge_kinds`). It mirrors the
//! shape of `dispatch_synthesis::registry` — a `&'static [Spec]` plus
//! consistency tests — adapted to the read-side projection (no compute fn:
//! the projection lives in [`super::bridges`], the declaration lives here).

/// Provenance template stamped onto every synthesized bridge edge. Fields not
/// listed here are either derived (`synthesized_by == output_dispatch_kind`,
/// `synthesis_key == normalized_path`, `confidence == min(...)`) or always
/// `None` for bridges (no source registration point: `registered_file`,
/// `registered_line`, `parser_tier`, `resolution_strategy`, `parser_confidence`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct BridgeProvenanceTemplate {
    /// `resolution_kind` stamped on the edge.
    pub(super) resolution_kind: &'static str,
}

/// One read-side virtual bridge kind.
pub(super) struct BridgeSynthesisSpec {
    /// Matches `http_call_edges.call_kind` case-insensitively. Ignored when
    /// [`BridgeSynthesisSpec::is_fallback`] is set.
    pub(super) input_call_kind: &'static str,
    /// `dispatch_kind` (and `synthesized_by`) stamped on synthesized edges.
    pub(super) output_dispatch_kind: &'static str,
    /// Catch-all for `call_kind`s matched by no other spec. At most one
    /// (asserted by `registry_pins_bridge_kinds`); [`dispatch_kind_for`]
    /// consults it last.
    pub(super) is_fallback: bool,
    pub(super) provenance: BridgeProvenanceTemplate,
}

/// Cap on `http_call_edges` / route nodes loaded per build. Truncation surfaces
/// as `bridge_cap` in `GraphExplain`. Override via [`BRIDGE_EDGE_LIMIT_ENV`].
pub(super) const BRIDGE_EDGE_LIMIT_DEFAULT: usize = 10_000;
pub(super) const BRIDGE_EDGE_LIMIT_ENV: &str = "CODECORTEX_BRIDGE_EDGE_LIMIT";

/// Resolve the bridge edge limit from the environment, falling back to
/// [`BRIDGE_EDGE_LIMIT_DEFAULT`] when unset/unparseable/zero. Single read site
/// so the cap is discoverable from the declaration.
pub(super) fn bridge_edge_limit() -> usize {
    std::env::var(BRIDGE_EDGE_LIMIT_ENV)
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|limit| *limit > 0)
        .unwrap_or(BRIDGE_EDGE_LIMIT_DEFAULT)
}

/// The closed registry of read-side virtual bridge kinds. Order is not
/// load-bearing (unlike `dispatch_synthesis::registry`): a non-fallback spec is
/// consulted before the fallback in [`dispatch_kind_for`], but the registry is
/// otherwise a set.
pub(super) fn bridge_registry() -> &'static [BridgeSynthesisSpec] {
    const REGISTRY: &[BridgeSynthesisSpec] = &[
        BridgeSynthesisSpec {
            input_call_kind: "http",
            output_dispatch_kind: "http_bridge",
            is_fallback: false,
            provenance: BridgeProvenanceTemplate {
                resolution_kind: "synthesized",
            },
        },
        BridgeSynthesisSpec {
            // Fallback: `input_call_kind` is unused (any non-http call_kind,
            // e.g. async message brokers, routes here).
            input_call_kind: "",
            output_dispatch_kind: "async_bridge",
            is_fallback: true,
            provenance: BridgeProvenanceTemplate {
                resolution_kind: "synthesized",
            },
        },
    ];
    REGISTRY
}

/// Resolve a `http_call_edges.call_kind` to its virtual bridge `dispatch_kind`
/// via the registry. The single source of the mapping — [`super::bridges`]
/// routes through here so a new bridge kind cannot appear as a literal.
///
/// Returns `""` only if the registry declares no fallback (forbidden by
/// `registry_pins_bridge_kinds`); callers treat an empty kind as "no bridge".
pub(super) fn dispatch_kind_for(call_kind: &str) -> &'static str {
    for spec in bridge_registry() {
        if !spec.is_fallback && call_kind.eq_ignore_ascii_case(spec.input_call_kind) {
            return spec.output_dispatch_kind;
        }
    }
    for spec in bridge_registry() {
        if spec.is_fallback {
            return spec.output_dispatch_kind;
        }
    }
    ""
}

/// The `resolution_kind` the registry stamps for a given `dispatch_kind`, or
/// `None` when the kind is not a declared bridge kind.
pub(super) fn resolution_kind_for(dispatch_kind: &str) -> Option<&'static str> {
    bridge_registry()
        .iter()
        .find(|spec| spec.output_dispatch_kind == dispatch_kind)
        .map(|spec| spec.provenance.resolution_kind)
}

// HTTP calls that produce no bridge edge are currently dropped silently by
// `super::bridges`. The unmatched categories — `no_caller_uid`,
// `no_normalized_path`, `no_route_handler` — are the schema for a future
// read-side "unmatched count by reason" surface. Surfacing needs idempotent
// per-walk wiring (counts, not a first-wins flag like `bridge_cap`), which
// belongs with that change alongside `GraphExplain` counter notes — not a
// `#[allow(dead_code)]` placeholder here.

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// Pin the closed registry of virtual bridge kinds. This is the single
    /// lock on which read-side bridge dispatch kinds exist — mirroring
    /// `dispatch_synthesis::registry_pins_pass_order_and_owned_edge_kinds`.
    #[test]
    fn registry_pins_bridge_kinds() {
        let specs = bridge_registry();

        // Declared kinds and order.
        let kinds: Vec<&str> = specs.iter().map(|spec| spec.output_dispatch_kind).collect();
        assert_eq!(kinds, ["http_bridge", "async_bridge"]);

        // At least one spec, output kinds unique and non-empty.
        assert!(!specs.is_empty());
        let mut seen = HashSet::new();
        for spec in specs {
            assert!(!spec.output_dispatch_kind.is_empty());
            assert!(seen.insert(spec.output_dispatch_kind), "duplicate kind");
            assert!(!spec.provenance.resolution_kind.is_empty());
        }

        // Exactly one fallback (consulted last by dispatch_kind_for).
        let fallbacks: Vec<&BridgeSynthesisSpec> =
            specs.iter().filter(|spec| spec.is_fallback).collect();
        assert_eq!(fallbacks.len(), 1, "exactly one fallback spec");
        assert_eq!(fallbacks[0].output_dispatch_kind, "async_bridge");

        // Non-fallback specs have a non-empty input_call_kind, unique.
        let mut inputs = HashSet::new();
        for spec in specs.iter().filter(|spec| !spec.is_fallback) {
            assert!(!spec.input_call_kind.is_empty());
            assert!(
                inputs.insert(spec.input_call_kind),
                "duplicate non-fallback input_call_kind"
            );
        }
    }

    /// `dispatch_kind_for` is the single mapping consumed by `bridges.rs`.
    /// Pin its routing so a new call_kind cannot silently change target.
    #[test]
    fn dispatch_kind_routes_via_registry() {
        assert_eq!(dispatch_kind_for("http"), "http_bridge");
        assert_eq!(dispatch_kind_for("HTTP"), "http_bridge");
        // Any non-http kind falls through to the fallback (async_bridge).
        assert_eq!(dispatch_kind_for("async"), "async_bridge");
        assert_eq!(dispatch_kind_for("kafka"), "async_bridge");
        assert_eq!(dispatch_kind_for(""), "async_bridge");

        // Every output of dispatch_kind_for must be a declared registry kind
        // (or "" when no fallback — forbidden above, but checked anyway).
        let declared: HashSet<&str> = bridge_registry()
            .iter()
            .map(|spec| spec.output_dispatch_kind)
            .collect();
        for sample in ["http", "HTTP", "async", "kafka", "", "amqp"] {
            let kind = dispatch_kind_for(sample);
            assert!(
                kind.is_empty() || declared.contains(kind),
                "dispatch_kind_for({sample:?}) = {kind:?} not declared"
            );
        }
    }

    #[test]
    fn resolution_kind_lookup_matches_registry() {
        assert_eq!(resolution_kind_for("http_bridge"), Some("synthesized"));
        assert_eq!(resolution_kind_for("async_bridge"), Some("synthesized"));
        assert_eq!(resolution_kind_for("call"), None);
    }
}
