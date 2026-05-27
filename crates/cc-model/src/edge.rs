/// # Edge Provenance Standard
///
/// All edge types should carry these provenance fields:
/// - `confidence: f64` — overall confidence (0.0–1.0)
/// - `parser_tier: String` — extraction method (generic/heuristic/tree_sitter/semantic/verified)
/// - `parser_confidence: f64` — parser's confidence in extraction correctness
/// - `resolution_kind: Option<String>` — resolution result (exact/qualified/heuristic/unresolved/synthesized)
/// - `resolution_strategy: Option<String>` — resolution method (import_map/global_unique/scope_resolved)
/// - `synthesized_by: Option<String>` — synthesis source for non-AST edges
/// - `synthesis_key: Option<String>` — synthesis identifier
///
/// Current coverage by edge table:
/// - call_edges: ALL fields ✓
/// - symbol_refs: resolution_* + parser_* ✓, no synthesized_*
/// - http_call_edges: confidence + parser_tier only
/// - semantic_edges: confidence + parser_tier only
/// - data_flow_edges: confidence + parser_tier only
/// - test_edges: confidence only (no parser_tier!)
/// - route_edges: confidence + parser_tier + framework
use crate::ParserTier;
use serde::{Deserialize, Serialize};

/// How a call is dispatched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DispatchKind {
    #[default]
    Direct,
    Dynamic,
    VirtualDispatch,
    OptionalChain,
    Constructor,
    EventEmitter,
    CallbackRelay,
    ReactiveBinding,
    FieldObserver,
}

impl DispatchKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Dynamic => "dynamic",
            Self::VirtualDispatch => "virtual_dispatch",
            Self::OptionalChain => "optional_chain",
            Self::Constructor => "constructor",
            Self::EventEmitter => "event_emitter",
            Self::CallbackRelay => "callback_relay",
            Self::ReactiveBinding => "reactive_binding",
            Self::FieldObserver => "field_observer",
        }
    }
}

/// Resolution status of a symbol reference or call edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionKind {
    #[default]
    Unresolved,
    Exact,
    Qualified,
    ScopeResolved,
    Heuristic,
}

impl ResolutionKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Unresolved => "unresolved",
            Self::Exact => "exact",
            Self::Qualified => "qualified",
            Self::ScopeResolved => "scope_resolved",
            Self::Heuristic => "heuristic",
        }
    }
}

/// A function/method call edge.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CallEdgeRecord {
    pub edge_id: String,
    pub file_path: String,
    pub caller_symbol: Option<String>,
    pub callee_symbol: String,
    pub line: u32,
    pub start_col: u32,
    pub end_line: Option<u32>,
    pub end_col: u32,
    pub target_symbol_id: Option<String>,
    pub target_file_path: Option<String>,
    pub caller_symbol_id: Option<String>,
    pub caller_symbol_uid: Option<String>,
    pub callee_symbol_uid: Option<String>,
    pub callee_ref_id: Option<String>,
    pub dispatch_kind: DispatchKind,
    pub call_kind: String,
    pub resolution_kind: ResolutionKind,
    /// Fine-grained resolver confidence (distinct from parser confidence).
    pub resolution_confidence: f64,
    /// Fine-grained resolution strategy name (e.g. exact/import_map/global_unique).
    pub resolution_strategy: String,
    pub receiver_expr: Option<String>,
    pub arg_count: Option<u32>,
    pub is_optional_chain: bool,
    pub is_awaited: bool,
    pub is_constructor: bool,
    pub parser_tier: ParserTier,
    pub parser_confidence: f64,
    #[serde(default)]
    pub synthesized_by: Option<String>,
    #[serde(default)]
    pub synthesis_key: Option<String>,
    #[serde(default)]
    pub registered_file: Option<String>,
    #[serde(default)]
    pub registered_line: Option<u32>,
}

/// An import declaration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportRecord {
    pub file_path: String,
    pub import_string: String,
    pub resolved_path: Option<String>,
    pub imported_name: Option<String>,
    pub alias: Option<String>,
    pub is_namespace: bool,
    pub is_default: bool,
    pub is_reexport: bool,
}

/// A test→code coverage edge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestEdgeRecord {
    pub edge_id: String,
    pub test_file_path: String,
    pub code_file_path: String,
    pub reason: String,
    pub confidence: f64,
}

/// A route endpoint node (first-class graph node, not just an edge).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteNodeRecord {
    pub route_id: String,
    pub file_path: String,
    pub route_path: String,
    pub method: Option<String>,
    pub handler_symbol_uid: Option<String>,
    pub handler_name: Option<String>,
    pub framework: Option<String>,
    pub line: u32,
    pub end_line: Option<u32>,
    pub normalized_path: Option<String>,
    pub confidence: f64,
    pub parser_tier: ParserTier,
}

/// 数据流边类型分类
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlowKind {
    Read,
    Write,
    TypeRef,
    EnvAccess,
    ParamPass,
    ReturnFlow,
}

impl FlowKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
            Self::TypeRef => "type_ref",
            Self::EnvAccess => "env_access",
            Self::ParamPass => "param_pass",
            Self::ReturnFlow => "return_flow",
        }
    }

    pub fn parse_str(s: &str) -> Self {
        match s {
            "write" => Self::Write,
            "type_ref" => Self::TypeRef,
            "env_access" => Self::EnvAccess,
            "param_pass" => Self::ParamPass,
            "return_flow" => Self::ReturnFlow,
            _ => Self::Read,
        }
    }
}

/// A data-flow dependency edge (variable/type usage across functions).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataFlowEdgeRecord {
    pub edge_id: String,
    pub file_path: String,
    pub source_symbol_uid: Option<String>,
    pub target_symbol_uid: Option<String>,
    pub flow_kind: String,
    pub line: u32,
    pub confidence: f64,
    pub parser_tier: ParserTier,
    pub env_key: Option<String>,
}

/// A git co-change correlation edge (files that frequently change together).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoChangeEdgeRecord {
    pub edge_id: String,
    pub file_a: String,
    pub file_b: String,
    pub co_change_count: u32,
    pub total_commits_a: u32,
    pub total_commits_b: u32,
    /// Jaccard similarity: co_changes / (total_a + total_b - co_changes)
    pub confidence: f64,
}

/// An HTTP or async client call edge (outbound call to a URL/endpoint/topic).
///
/// Links a caller symbol to a URL/path that may match a server-side Route node,
/// enabling cross-service call graph construction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpCallEdgeRecord {
    pub edge_id: String,
    pub file_path: String,
    pub caller_symbol_uid: Option<String>,
    /// Raw URL or path as written in source (e.g. `"/api/users/:id"`, `"http://svc/endpoint"`).
    pub url_or_path: String,
    /// Normalized path for matching against route_nodes (e.g. `/api/users/*`).
    pub normalized_path: Option<String>,
    /// HTTP method if detectable (GET, POST, etc.), None for async/grpc.
    pub method: Option<String>,
    /// `"http"` | `"async"` | `"grpc"`
    pub call_kind: String,
    pub line: u32,
    pub confidence: f64,
    pub parser_tier: ParserTier,
    /// Broker type for async calls: "kafka", "rabbitmq", "pubsub", "sqs", "sns", "celery", "bullmq", "temporal" etc.
    /// None for http/grpc calls.
    pub broker_type: Option<String>,
}

/// A semantic relationship edge (inheritance, implementation, decoration, etc.)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticEdgeRecord {
    pub edge_id: String,
    pub file_path: String,
    /// Source symbol (the class/struct that extends/implements)
    pub source_symbol: String,
    pub source_symbol_uid: Option<String>,
    /// Target symbol (the parent class/interface/trait)
    pub target_symbol: String,
    pub target_symbol_uid: Option<String>,
    /// Kind of relationship
    pub relation_kind: SemanticRelation,
    pub line: u32,
    pub confidence: f64,
    pub parser_tier: ParserTier,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticRelation {
    Inherits,
    Implements,
    Decorates,
    Throws,
    UsesType,
    /// File defines a top-level symbol (function/class/constant)
    Defines,
    /// Class/struct defines a method
    DefinesMethod,
    /// Folder/module contains a file
    ContainsFile,
    /// Module contains a submodule
    ContainsModule,
    RendersComponent,
    /// Dependency injection wiring (e.g. Spring @Autowired, NestJS constructor injection)
    Injects,
    /// Unrecognized relation kind from DB
    Unknown,
}

impl SemanticRelation {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Inherits => "inherits",
            Self::Implements => "implements",
            Self::Decorates => "decorates",
            Self::Throws => "throws",
            Self::UsesType => "uses_type",
            Self::Defines => "defines",
            Self::DefinesMethod => "defines_method",
            Self::ContainsFile => "contains_file",
            Self::ContainsModule => "contains_module",
            Self::RendersComponent => "renders_component",
            Self::Injects => "injects",
            Self::Unknown => "unknown",
        }
    }
}

/// A route→handler edge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteEdgeRecord {
    pub edge_id: String,
    pub file_path: String,
    pub route_path: String,
    pub handler_name: Option<String>,
    pub method: Option<String>,
    pub line: u32,
    pub start_col: u32,
    pub end_line: Option<u32>,
    pub end_col: u32,
    pub handler_symbol_id: Option<String>,
    pub handler_symbol_uid: Option<String>,
    pub handler_expr: Option<String>,
    pub router_symbol_uid: Option<String>,
    pub framework: Option<String>,
    pub route_kind: Option<String>,
    pub confidence: f64,
    pub parser_tier: ParserTier,
}
