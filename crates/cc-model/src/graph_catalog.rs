//! Shared catalog of graph relationship facts.
//!
//! This Module is the single source of truth for Cypher edge execution and the
//! MCP `status(aspect = "schema")` graph relationship description. Keep DB
//! table/column facts, join semantics, schema-facing descriptions, properties,
//! and next-tool hints here instead of duplicating them in search/server crates.

use std::collections::BTreeSet;

/// A queryable graph relationship and its schema metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphRelationship {
    /// Cypher edge name, e.g. `CALLS`.
    pub edge: &'static str,
    /// Backing DB table.
    pub table: &'static str,
    /// Source endpoint column and join behavior.
    pub source: GraphRelationshipEndpoint,
    /// Destination endpoint column and join behavior.
    pub destination: GraphRelationshipEndpoint,
    /// Optional edge-table SQL filter, written against the edge table alias.
    pub extra_filter: Option<&'static str>,
    /// Schema-facing shape and description.
    pub schema: GraphRelationshipSchema,
    /// Filterable/informational edge properties for agents.
    pub properties: GraphRelationshipProperties,
    /// Optional hint for the next MCP tool to use after discovering this edge.
    pub next_tool_hint: Option<&'static str>,
    /// Whether runtime evidence can be linked to this edge kind.
    pub runtime_evidence: bool,
    /// Whether this relationship should appear in graph schema patterns.
    pub visible_in_schema: bool,
    /// Whether the Cypher executor can expand this edge with `*min..max`.
    pub variable_length: bool,
}

/// One side of a relationship.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphRelationshipEndpoint {
    /// Edge table column containing the endpoint id/path.
    pub column: &'static str,
    /// Whether the default node join uses `symbols.symbol_uid` (`true`) or
    /// `file_path` (`false`). Ignored when `join_on` overrides the join.
    pub is_symbol: bool,
    /// Optional override for the node table column used by the default join.
    pub join_key: Option<&'static str>,
    /// Optional full SQL `ON` expression template. Placeholders `{src}`, `{dst}`,
    /// and `{e}` are substituted by the executor. `Some("")` means this side has
    /// no concrete node table and the join should be skipped.
    pub join_on: Option<&'static str>,
}

/// Relationship shape as shown to MCP clients.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphRelationshipSchema {
    pub from: &'static str,
    pub to: &'static str,
    pub description: &'static str,
}

/// Agent-facing property hints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphRelationshipProperties {
    pub filterable: &'static [&'static str],
    pub informational: &'static [&'static str],
}

impl GraphRelationshipProperties {
    pub const EMPTY: Self = Self {
        filterable: &[],
        informational: &[],
    };

    pub fn is_empty(self) -> bool {
        self.filterable.is_empty() && self.informational.is_empty()
    }
}

const CALL_PROPS: GraphRelationshipProperties = GraphRelationshipProperties {
    filterable: &[
        "dispatch_kind",
        "call_kind",
        "resolution_kind",
        "parser_tier",
        "synthesized_by",
    ],
    informational: &[
        "confidence",
        "parser_confidence",
        "synthesis_key",
        "registered_file",
    ],
};

const HTTP_PROPS: GraphRelationshipProperties = GraphRelationshipProperties {
    filterable: &["method", "call_kind", "broker_type"],
    informational: &["confidence", "url_or_path", "normalized_path"],
};

const ROUTE_PROPS: GraphRelationshipProperties = GraphRelationshipProperties {
    filterable: &["method", "framework", "route_kind"],
    informational: &[
        "confidence",
        "route_path",
        "handler_name",
        "normalized_path",
    ],
};

const DATA_FLOW_PROPS: GraphRelationshipProperties = GraphRelationshipProperties {
    filterable: &["flow_kind"],
    informational: &["confidence", "env_key"],
};

const SEMANTIC_PROPS: GraphRelationshipProperties = GraphRelationshipProperties {
    filterable: &["relation_kind"],
    informational: &["confidence"],
};

const IMPORT_PROPS: GraphRelationshipProperties = GraphRelationshipProperties {
    filterable: &[
        "imported_name",
        "alias",
        "is_namespace",
        "is_default",
        "is_reexport",
    ],
    informational: &["import_string", "resolved_path"],
};

const REF_PROPS: GraphRelationshipProperties = GraphRelationshipProperties {
    filterable: &["ref_kind", "resolution_kind"],
    informational: &["symbol_name", "ref_name", "target_symbol_uid"],
};

const TEST_PROPS: GraphRelationshipProperties = GraphRelationshipProperties {
    filterable: &["reason"],
    informational: &["confidence"],
};

const CO_CHANGE_PROPS: GraphRelationshipProperties = GraphRelationshipProperties {
    filterable: &[],
    informational: &[
        "confidence",
        "co_change_count",
        "total_commits_a",
        "total_commits_b",
    ],
};

const CALL_HINT: &str = "trace(from, to, source_mode='body') for call paths; relations(symbol, kind='callers'|'callees') for direct edges";
const ROUTE_HINT: &str =
    "architecture(aspect='routes') for all routes; explore(symbols, mode='flow') for request flow";
const HTTP_HINT: &str =
    "architecture(aspect='services') for service map; ingest_traces to validate with runtime data";
const DATA_FLOW_HINT: &str =
    "explore(symbols, mode='flow') for data dependencies; relations(symbol, kind='refs') for references";
const SEMANTIC_HINT: &str =
    "relations(symbol, kind='hierarchy') for type hierarchy; node(symbol, include='trail') for overview";
const IMPORT_HINT: &str =
    "architecture(aspect='deps') for dependency map; relations(symbol, kind='refs') for symbol-level references";
const TEST_HINT: &str =
    "find_dead_code(include_tests=false) before pruning; explore(symbols, mode='overview') for coverage context";

const fn endpoint(
    column: &'static str,
    is_symbol: bool,
    join_key: Option<&'static str>,
    join_on: Option<&'static str>,
) -> GraphRelationshipEndpoint {
    GraphRelationshipEndpoint {
        column,
        is_symbol,
        join_key,
        join_on,
    }
}

const fn symbol_endpoint(column: &'static str) -> GraphRelationshipEndpoint {
    endpoint(column, true, None, None)
}

const fn file_endpoint(column: &'static str) -> GraphRelationshipEndpoint {
    endpoint(column, false, None, None)
}

const fn schema(
    from: &'static str,
    to: &'static str,
    description: &'static str,
) -> GraphRelationshipSchema {
    GraphRelationshipSchema {
        from,
        to,
        description,
    }
}

/// Canonical graph relationship facts.
static GRAPH_RELATIONSHIPS: &[GraphRelationship] = &[
    GraphRelationship {
        edge: "CALLS",
        table: "call_edges",
        source: symbol_endpoint("caller_symbol_uid"),
        destination: symbol_endpoint("callee_symbol_uid"),
        extra_filter: None,
        schema: schema("Function", "Function", "Direct or dynamic function call"),
        properties: CALL_PROPS,
        next_tool_hint: Some(CALL_HINT),
        runtime_evidence: false,
        visible_in_schema: true,
        variable_length: true,
    },
    GraphRelationship {
        edge: "IMPORTS",
        table: "imports",
        source: file_endpoint("file_path"),
        destination: file_endpoint("resolved_path"),
        extra_filter: None,
        schema: schema("File", "File", "Import dependency between files/modules"),
        properties: IMPORT_PROPS,
        next_tool_hint: Some(IMPORT_HINT),
        runtime_evidence: false,
        visible_in_schema: true,
        variable_length: false,
    },
    GraphRelationship {
        edge: "TESTS",
        table: "test_edges",
        source: file_endpoint("test_file_path"),
        destination: file_endpoint("code_file_path"),
        extra_filter: None,
        schema: schema("File", "File", "Test file covers a code file"),
        properties: TEST_PROPS,
        next_tool_hint: Some(TEST_HINT),
        runtime_evidence: false,
        visible_in_schema: true,
        variable_length: false,
    },
    GraphRelationship {
        edge: "HANDLES",
        table: "routes",
        source: endpoint("edge_id", false, Some("edge_id"), None),
        destination: symbol_endpoint("handler_symbol_uid"),
        extra_filter: None,
        schema: schema("Route", "Function", "HTTP route mapped to handler function"),
        properties: ROUTE_PROPS,
        next_tool_hint: Some(ROUTE_HINT),
        runtime_evidence: false,
        visible_in_schema: true,
        variable_length: false,
    },
    GraphRelationship {
        edge: "ROUTES",
        table: "routes",
        source: file_endpoint("file_path"),
        destination: endpoint("edge_id", false, Some("edge_id"), None),
        extra_filter: None,
        schema: schema("File", "Route", "File declares an HTTP route"),
        properties: ROUTE_PROPS,
        next_tool_hint: Some(ROUTE_HINT),
        runtime_evidence: false,
        visible_in_schema: true,
        variable_length: false,
    },
    GraphRelationship {
        edge: "ROUTE",
        table: "routes",
        source: file_endpoint("file_path"),
        destination: endpoint("edge_id", false, Some("edge_id"), None),
        extra_filter: None,
        schema: schema("File", "Route", "Alias for ROUTES"),
        properties: ROUTE_PROPS,
        next_tool_hint: Some(ROUTE_HINT),
        runtime_evidence: false,
        visible_in_schema: false,
        variable_length: false,
    },
    GraphRelationship {
        edge: "REFERENCES",
        table: "symbol_refs",
        source: file_endpoint("file_path"),
        destination: file_endpoint("target_file_path"),
        extra_filter: None,
        schema: schema("File", "File", "Symbol reference from one file to another"),
        properties: REF_PROPS,
        next_tool_hint: Some(IMPORT_HINT),
        runtime_evidence: false,
        visible_in_schema: true,
        variable_length: false,
    },
    GraphRelationship {
        edge: "REFS",
        table: "symbol_refs",
        source: file_endpoint("file_path"),
        destination: file_endpoint("target_file_path"),
        extra_filter: None,
        schema: schema("File", "File", "Alias for REFERENCES"),
        properties: REF_PROPS,
        next_tool_hint: Some(IMPORT_HINT),
        runtime_evidence: false,
        visible_in_schema: false,
        variable_length: false,
    },
    GraphRelationship {
        edge: "CO_CHANGE",
        table: "co_change_edges",
        source: file_endpoint("file_a"),
        destination: file_endpoint("file_b"),
        extra_filter: None,
        schema: schema(
            "File",
            "File",
            "Files frequently changed together in commits",
        ),
        properties: CO_CHANGE_PROPS,
        next_tool_hint: None,
        runtime_evidence: false,
        visible_in_schema: true,
        variable_length: false,
    },
    GraphRelationship {
        edge: "DATA_FLOW",
        table: "data_flow_edges",
        source: symbol_endpoint("source_symbol_uid"),
        destination: symbol_endpoint("target_symbol_uid"),
        extra_filter: None,
        schema: schema("Function", "Function", "Data flows between functions"),
        properties: DATA_FLOW_PROPS,
        next_tool_hint: Some(DATA_FLOW_HINT),
        runtime_evidence: false,
        visible_in_schema: true,
        variable_length: false,
    },
    GraphRelationship {
        edge: "HTTP_CALLS",
        table: "http_call_edges",
        source: symbol_endpoint("caller_symbol_uid"),
        destination: endpoint("normalized_path", false, Some("normalized_path"), None),
        extra_filter: Some("call_kind = 'http'"),
        schema: schema("Function", "Route", "Code makes an outbound HTTP request"),
        properties: HTTP_PROPS,
        next_tool_hint: Some(HTTP_HINT),
        runtime_evidence: true,
        visible_in_schema: true,
        variable_length: false,
    },
    GraphRelationship {
        edge: "HTTP_CALL",
        table: "http_call_edges",
        source: symbol_endpoint("caller_symbol_uid"),
        destination: endpoint("normalized_path", false, Some("normalized_path"), None),
        extra_filter: Some("call_kind = 'http'"),
        schema: schema(
            "Function",
            "Route",
            "Compatibility alias for HTTP_CALLS outbound HTTP requests",
        ),
        properties: HTTP_PROPS,
        next_tool_hint: Some(HTTP_HINT),
        runtime_evidence: true,
        visible_in_schema: true,
        variable_length: false,
    },
    GraphRelationship {
        edge: "ASYNC_CALLS",
        table: "http_call_edges",
        source: symbol_endpoint("caller_symbol_uid"),
        destination: endpoint("normalized_path", false, Some("normalized_path"), None),
        extra_filter: Some("call_kind IN ('async', 'grpc')"),
        schema: schema(
            "Function",
            "Route",
            "Code dispatches async or gRPC-style outbound work",
        ),
        properties: HTTP_PROPS,
        next_tool_hint: Some(HTTP_HINT),
        runtime_evidence: false,
        visible_in_schema: true,
        variable_length: false,
    },
    GraphRelationship {
        edge: "INHERITS",
        table: "semantic_edges",
        source: symbol_endpoint("source_symbol_uid"),
        destination: symbol_endpoint("target_symbol_uid"),
        extra_filter: Some("relation_kind = 'inherits'"),
        schema: schema("Class", "Class", "Class inheritance (extends)"),
        properties: SEMANTIC_PROPS,
        next_tool_hint: Some(SEMANTIC_HINT),
        runtime_evidence: false,
        visible_in_schema: true,
        variable_length: false,
    },
    GraphRelationship {
        edge: "IMPLEMENTS",
        table: "semantic_edges",
        source: symbol_endpoint("source_symbol_uid"),
        destination: symbol_endpoint("target_symbol_uid"),
        extra_filter: Some("relation_kind = 'implements'"),
        schema: schema("Class", "Interface", "Interface implementation"),
        properties: SEMANTIC_PROPS,
        next_tool_hint: Some(SEMANTIC_HINT),
        runtime_evidence: false,
        visible_in_schema: true,
        variable_length: false,
    },
    GraphRelationship {
        edge: "DECORATES",
        table: "semantic_edges",
        source: symbol_endpoint("source_symbol_uid"),
        destination: symbol_endpoint("target_symbol_uid"),
        extra_filter: Some("relation_kind = 'decorates'"),
        schema: schema("Function", "Function", "Decorator / annotation application"),
        properties: SEMANTIC_PROPS,
        next_tool_hint: Some(SEMANTIC_HINT),
        runtime_evidence: false,
        visible_in_schema: true,
        variable_length: false,
    },
    GraphRelationship {
        edge: "THROWS",
        table: "semantic_edges",
        source: symbol_endpoint("source_symbol_uid"),
        destination: symbol_endpoint("target_symbol_uid"),
        extra_filter: Some("relation_kind = 'throws'"),
        schema: schema("Function", "Class", "Exception / error throw relation"),
        properties: SEMANTIC_PROPS,
        next_tool_hint: Some(SEMANTIC_HINT),
        runtime_evidence: false,
        visible_in_schema: true,
        variable_length: false,
    },
    GraphRelationship {
        edge: "USES_TYPE",
        table: "semantic_edges",
        source: symbol_endpoint("source_symbol_uid"),
        destination: symbol_endpoint("target_symbol_uid"),
        extra_filter: Some("relation_kind = 'uses_type'"),
        schema: schema(
            "Function",
            "Class",
            "Type usage in parameters or return types",
        ),
        properties: SEMANTIC_PROPS,
        next_tool_hint: Some(SEMANTIC_HINT),
        runtime_evidence: false,
        visible_in_schema: true,
        variable_length: false,
    },
    GraphRelationship {
        edge: "SEMANTIC",
        table: "semantic_edges",
        source: symbol_endpoint("source_symbol_uid"),
        destination: symbol_endpoint("target_symbol_uid"),
        extra_filter: None,
        schema: schema("Symbol", "Symbol", "Generic semantic relationship"),
        properties: SEMANTIC_PROPS,
        next_tool_hint: Some(SEMANTIC_HINT),
        runtime_evidence: false,
        visible_in_schema: false,
        variable_length: false,
    },
    GraphRelationship {
        edge: "RENDERS_COMPONENT",
        table: "semantic_edges",
        source: symbol_endpoint("source_symbol_uid"),
        destination: symbol_endpoint("target_symbol_uid"),
        extra_filter: Some("relation_kind = 'renders_component'"),
        schema: schema(
            "Function",
            "Function",
            "React/Vue component renders another component",
        ),
        properties: SEMANTIC_PROPS,
        next_tool_hint: Some(SEMANTIC_HINT),
        runtime_evidence: false,
        visible_in_schema: true,
        variable_length: false,
    },
    GraphRelationship {
        edge: "DEFINES",
        table: "semantic_edges",
        source: endpoint(
            "source_symbol_uid",
            false,
            None,
            Some("{src}.file_path = SUBSTR({e}.source_symbol_uid, 7)"),
        ),
        destination: symbol_endpoint("target_symbol_uid"),
        extra_filter: Some("relation_kind = 'defines'"),
        schema: schema("File", "Symbol", "File defines a top-level symbol"),
        properties: SEMANTIC_PROPS,
        next_tool_hint: Some(SEMANTIC_HINT),
        runtime_evidence: false,
        visible_in_schema: true,
        variable_length: true,
    },
    GraphRelationship {
        edge: "DEFINES_METHOD",
        table: "semantic_edges",
        source: symbol_endpoint("source_symbol_uid"),
        destination: symbol_endpoint("target_symbol_uid"),
        extra_filter: Some("relation_kind = 'defines_method'"),
        schema: schema("Class", "Method", "Class/struct defines a method"),
        properties: SEMANTIC_PROPS,
        next_tool_hint: Some(SEMANTIC_HINT),
        runtime_evidence: false,
        visible_in_schema: true,
        variable_length: true,
    },
    GraphRelationship {
        edge: "CONTAINS_FILE",
        table: "semantic_edges",
        source: endpoint("source_symbol_uid", false, None, Some("")),
        destination: endpoint(
            "target_symbol_uid",
            false,
            None,
            Some("{dst}.file_path = SUBSTR({e}.target_symbol_uid, 7)"),
        ),
        extra_filter: Some("relation_kind = 'contains_file'"),
        schema: schema("Module", "File", "Folder/module contains a file"),
        properties: SEMANTIC_PROPS,
        next_tool_hint: Some(SEMANTIC_HINT),
        runtime_evidence: false,
        visible_in_schema: true,
        variable_length: true,
    },
    GraphRelationship {
        edge: "CONTAINS_MODULE",
        table: "semantic_edges",
        source: endpoint("source_symbol_uid", false, None, Some("")),
        destination: endpoint("target_symbol_uid", false, None, Some("")),
        extra_filter: Some("relation_kind = 'contains_module'"),
        schema: schema("Module", "Module", "Module contains a submodule"),
        properties: SEMANTIC_PROPS,
        next_tool_hint: Some(SEMANTIC_HINT),
        runtime_evidence: false,
        visible_in_schema: true,
        variable_length: true,
    },
];

/// Return all queryable graph relationships, including hidden aliases.
pub fn graph_relationships() -> &'static [GraphRelationship] {
    GRAPH_RELATIONSHIPS
}

/// Find a relationship by Cypher edge name.
pub fn graph_relationship(edge: &str) -> Option<&'static GraphRelationship> {
    GRAPH_RELATIONSHIPS.iter().find(|rel| rel.edge == edge)
}

/// Return unique backing edge table names in deterministic order.
pub fn graph_relationship_table_names() -> Vec<&'static str> {
    GRAPH_RELATIONSHIPS
        .iter()
        .map(|rel| rel.table)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

// ── EdgeKindSet: typed per-tool edge-kind subsets ───────────────────────────

/// A typed, ordered set of catalog edge kinds consumed by one tool surface.
///
/// The catalog above is runtime data, so membership cannot be proven at
/// compile time; instead every declared set is validated against the catalog
/// by the `tool_graph_subsets` tests below（未知 kind = 测试失败）and by a
/// `debug_assert` at each seeding site (`GraphExplainCollector::
/// declare_edge_kinds`). Kinds are kept in documentation order: primary
/// traversal kinds first, advisory/secondary kinds after.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EdgeKindSet {
    kinds: &'static [&'static str],
}

impl EdgeKindSet {
    pub const fn new(kinds: &'static [&'static str]) -> Self {
        Self { kinds }
    }

    /// The declared kind names, in declaration order.
    pub const fn kinds(self) -> &'static [&'static str] {
        self.kinds
    }

    pub fn contains(self, kind: &str) -> bool {
        self.kinds.iter().any(|declared| *declared == kind)
    }

    pub fn iter(self) -> impl Iterator<Item = &'static str> {
        self.kinds.iter().copied()
    }

    /// Members that do NOT resolve in the catalog (empty = valid declaration).
    pub fn unknown_kinds(self) -> Vec<&'static str> {
        self.kinds
            .iter()
            .copied()
            .filter(|kind| graph_relationship(kind).is_none())
            .collect()
    }
}

/// Per-tool graph subset declarations: which catalog edge kinds each tool
/// surface consults. 这是工具×边类型矩阵的单一声明点 —— 此前只存在于审计文档，
/// 现在由 catalog 一致性测试机器校验（见下方 tests）。
///
/// 纯声明 + 可见性：不改变任何工具的实际遍历；如声明与实现分歧，以实现为准
/// 修订声明。Declared as `const`（而非 `static`）so const contexts —
/// `FastPathConfig::DEFAULT` in cc-search — can reference them directly.
pub mod tool_graph_subsets {
    use super::EdgeKindSet;

    /// Cypher lazy-BFS fast path (cc-search `fast_path.rs`): only CALLS has a
    /// lazy adjacency source (`call_edges_from_uid_lite`), so only CALLS is
    /// gate-eligible. `FastPathConfig::DEFAULT.eligible_edge_kinds` references
    /// this declaration; every member must also be catalog `variable_length`
    /// (the fast path serves `*m..n` traversals — asserted in tests).
    pub const CYPHER_FAST_PATH: EdgeKindSet = EdgeKindSet::new(&["CALLS"]);

    /// impact (cc-server `impact.rs`): the blast-radius BFS walks reverse
    /// CALLS only, deliberately WITHOUT synthesized HTTP bridges
    /// (`GraphReadModel::without_http_bridges`) — cross-service effects are
    /// advisory, reported separately, and must not inflate the primary risk
    /// walk. TESTS backs `suggested_tests`; HANDLES + HTTP_CALLS +
    /// ASYNC_CALLS back advisory Pass A (route handler → reverse HTTP/async
    /// callers — the reverse lookup does not filter `call_kind`, so both
    /// kinds are consulted); CO_CHANGE backs advisory Pass B.
    pub const IMPACT: EdgeKindSet = EdgeKindSet::new(&[
        "CALLS",
        "TESTS",
        "HANDLES",
        "HTTP_CALLS",
        "ASYNC_CALLS",
        "CO_CHANGE",
    ]);

    /// trace (cc-server `graph_trace.rs`): CALLS adjacency plus synthesized
    /// cross-service bridges (`GraphReadModel::new`) — bridges join
    /// HTTP_CALLS / ASYNC_CALLS evidence to HANDLES route handlers so traces
    /// can cross service boundaries. Runtime evidence rides on HTTP_CALLS
    /// (catalog `runtime_evidence` flag), not as a separate kind.
    pub const TRACE: EdgeKindSet =
        EdgeKindSet::new(&["CALLS", "HANDLES", "HTTP_CALLS", "ASYNC_CALLS"]);

    /// search graph enrichment (cc-search `enrich.rs`): CALLS backs degree
    /// scoring and caller/callee context nodes, REFERENCES backs the
    /// ref-count score bonus, TESTS backs test-coverage context nodes.
    pub const SEARCH_ENRICH: EdgeKindSet = EdgeKindSet::new(&["CALLS", "REFERENCES", "TESTS"]);

    /// type_hierarchy (cc-server `graph_type_hierarchy.rs`): the BFS keeps
    /// only INHERITS / IMPLEMENTS edges; DEFINES_METHOD backs method listing
    /// and override detection. (The semantic adjacency cache loads all
    /// semantic edges, but only these kinds are consulted.)
    pub const TYPE_HIERARCHY: EdgeKindSet =
        EdgeKindSet::new(&["INHERITS", "IMPLEMENTS", "DEFINES_METHOD"]);

    /// circular deps (cc-server `graph_cycles.rs`): file/package granularity
    /// runs Tarjan over IMPORTS adjacency; community granularity runs it over
    /// the community-projected CALLS adjacency.
    pub const CYCLES: EdgeKindSet = EdgeKindSet::new(&["IMPORTS", "CALLS"]);

    /// relations / explore symbol detail (cc-server `engine_query.rs`,
    /// caller/callee/semantic/metrics block): CALLS backs callers/callees and
    /// degree metrics, SEMANTIC (unfiltered semantic_edges) backs
    /// `include_relations`, REFERENCES backs the ref count in metrics. These
    /// reads bypass GraphReadModel by design — see the marker comment there.
    pub const RELATIONS: EdgeKindSet = EdgeKindSet::new(&["CALLS", "SEMANTIC", "REFERENCES"]);

    /// All per-tool declarations, in stable (matrix) order, for
    /// catalog-consistency tests and the tool×edge-kind matrix snapshot.
    pub const ALL: &[(&str, EdgeKindSet)] = &[
        ("CYPHER_FAST_PATH", CYPHER_FAST_PATH),
        ("CYCLES", CYCLES),
        ("IMPACT", IMPACT),
        ("RELATIONS", RELATIONS),
        ("SEARCH_ENRICH", SEARCH_ENRICH),
        ("TRACE", TRACE),
        ("TYPE_HIERARCHY", TYPE_HIERARCHY),
    ];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_covers_core_query_edges() {
        for edge in [
            "CALLS",
            "IMPORTS",
            "HANDLES",
            "HTTP_CALLS",
            "HTTP_CALL",
            "ASYNC_CALLS",
            "DEFINES",
            "CONTAINS_FILE",
            "REFERENCES",
        ] {
            assert!(graph_relationship(edge).is_some(), "missing {edge}");
        }
    }

    #[test]
    fn table_names_are_unique_and_include_symbol_refs() {
        let tables = graph_relationship_table_names();
        let unique = tables.iter().copied().collect::<BTreeSet<_>>();
        assert_eq!(tables.len(), unique.len());
        assert!(tables.contains(&"call_edges"));
        assert!(tables.contains(&"symbol_refs"));
        assert!(tables.contains(&"semantic_edges"));
    }

    #[test]
    fn visible_relationships_have_schema_descriptions() {
        for rel in graph_relationships()
            .iter()
            .filter(|rel| rel.visible_in_schema)
        {
            assert!(
                !rel.schema.from.is_empty(),
                "{} missing from label",
                rel.edge
            );
            assert!(!rel.schema.to.is_empty(), "{} missing to label", rel.edge);
            assert!(
                !rel.schema.description.is_empty(),
                "{} missing description",
                rel.edge
            );
        }
    }

    // ── tool_graph_subsets: catalog consistency ─────────────────────────

    /// Every declared tool subset must resolve entirely in the catalog: an
    /// unknown kind name (typo, or a kind removed from the catalog without
    /// updating the declarations) fails here.
    #[test]
    fn tool_subsets_resolve_in_catalog() {
        for (tool, set) in tool_graph_subsets::ALL {
            let unknown = set.unknown_kinds();
            assert!(
                unknown.is_empty(),
                "{tool} declares edge kinds missing from the catalog: {unknown:?}"
            );
            assert!(!set.kinds().is_empty(), "{tool} declares an empty subset");
        }
    }

    /// The fast path serves variable-length traversals, so its eligible set
    /// must stay within catalog kinds the SQL CTE also supports recursively.
    #[test]
    fn cypher_fast_path_kinds_support_variable_length() {
        for kind in tool_graph_subsets::CYPHER_FAST_PATH.iter() {
            let rel = graph_relationship(kind).expect("kind resolves in catalog");
            assert!(
                rel.variable_length,
                "fast-path kind {kind} lacks catalog variable_length support"
            );
        }
    }

    #[test]
    fn edge_kind_set_contains_and_iter() {
        let set = tool_graph_subsets::IMPACT;
        assert!(set.contains("CALLS"));
        assert!(set.contains("CO_CHANGE"));
        assert!(!set.contains("IMPORTS"));
        assert_eq!(set.iter().count(), set.kinds().len());
    }

    /// Machine-checked tool×edge-kind matrix (adjacency-list rendering).
    /// This snapshot replaces the matrix that previously lived only in audit
    /// docs: changing any tool's graph subset — or adding a tool — must be a
    /// conscious edit here AND in the declaration.
    #[test]
    fn tool_edge_kind_matrix_snapshot() {
        let rendered: String = tool_graph_subsets::ALL
            .iter()
            .map(|(tool, set)| format!("{tool}: {}\n", set.kinds().join(", ")))
            .collect();
        assert_eq!(
            rendered,
            "CYPHER_FAST_PATH: CALLS\n\
             CYCLES: IMPORTS, CALLS\n\
             IMPACT: CALLS, TESTS, HANDLES, HTTP_CALLS, ASYNC_CALLS, CO_CHANGE\n\
             RELATIONS: CALLS, SEMANTIC, REFERENCES\n\
             SEARCH_ENRICH: CALLS, REFERENCES, TESTS\n\
             TRACE: CALLS, HANDLES, HTTP_CALLS, ASYNC_CALLS\n\
             TYPE_HIERARCHY: INHERITS, IMPLEMENTS, DEFINES_METHOD\n"
        );
    }
}
