//! Spring Framework resolver.
//!
//! Enrichment:
//! - `enrich_file`: extract @RequestMapping / @GetMapping / @PostMapping annotations → route_edges;
//!   detect @Autowired field and constructor injection → semantic_edges (UsesType)
//! - `resolve_cross_file`: resolve injected types against SymbolCatalog, bind UIDs,
//!   create Injects semantic edges for DI wiring (controller → service/repository/component)

use cc_model::edge::{RouteEdgeRecord, SemanticEdgeRecord, SemanticRelation};
use cc_model::id::StableId;
use cc_model::parse::ParseOutcome;
use cc_model::{Language, ParserTier, SymbolKind};
use regex::Regex;
use std::sync::LazyLock;

use super::{line_for_offset, FrameworkResolver, ProjectFrameworkContext};

// ---------------------------------------------------------------------------
// Regex patterns
// ---------------------------------------------------------------------------

/// Class-level @RequestMapping("/prefix") or @RequestMapping(value = "/prefix")
static CLASS_REQUEST_MAPPING_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)@RequestMapping\s*\(\s*(?:value\s*=\s*)?"([^"]+)"\s*\)"#)
        .expect("class request mapping re")
});

/// Method-level HTTP verb annotations: @GetMapping("/path"), @PostMapping("/path"), etc.
/// Also matches bare @GetMapping without a path argument.
static METHOD_MAPPING_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?m)@(Get|Post|Put|Delete|Patch)Mapping(?:\s*\(\s*(?:value\s*=\s*)?"([^"]*)"[^)]*\))?"#,
    )
    .expect("method mapping re")
});

/// Method-level @RequestMapping with method attribute:
/// @RequestMapping(value = "/path", method = RequestMethod.GET)
static METHOD_REQUEST_MAPPING_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?m)@RequestMapping\s*\([^)]*value\s*=\s*"([^"]+)"[^)]*method\s*=\s*RequestMethod\.(\w+)[^)]*\)"#,
    )
    .expect("method request mapping re")
});

/// Extracts a method name from a Java method declaration line.
/// Matches: `public List<User> getUsers(` or `void create(`
static JAVA_METHOD_NAME_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?m)(?:public|protected|private|static|final|abstract|synchronized|default|\s)+(?:<[^>]+>\s+)?(?:[A-Za-z_][A-Za-z0-9_<>,\[\]\s?]*?)\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(",
    )
    .expect("java method name re")
});

/// @Autowired field injection:
///   @Autowired
///   private UserService userService;
///
/// Also matches single-line: @Autowired private UserService userService;
///
/// Captures: (1) type name (e.g. "UserService")
static AUTOWIRED_FIELD_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?m)@Autowired\s+(?:private|protected|public)?\s*([A-Z][A-Za-z0-9_]*(?:<[^>]+>)?)\s+\w+\s*;"#,
    )
    .expect("autowired field re")
});

/// Constructor injection: constructor parameters with types.
/// Matches constructor parameter list with type names.
///
/// Example:
///   public UserController(UserService userService, OrderService orderService) {
///
/// We first find the constructor, then extract each parameter type.
///
/// Captures: (1) class name, (2) full parameter list
static CONSTRUCTOR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?m)(?:@Autowired\s+)?(?:public|protected|private)\s+([A-Z][A-Za-z0-9_]*)\s*\(([^)]+)\)\s*\{"#,
    )
    .expect("constructor re")
});

/// Extracts individual typed parameters from a comma-separated parameter list.
/// Matches: `UserService userService` or `@Qualifier("main") OrderService orderService`
///
/// Captures: (1) type name
static PARAM_TYPE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?:@\w+(?:\([^)]*\))?\s+)*([A-Z][A-Za-z0-9_]*(?:<[^>]+>)?)\s+\w+"#)
        .expect("param type re")
});

/// Detect class declaration with its name.
/// Captures: (1) class name
static CLASS_DECL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)(?:public\s+)?class\s+([A-Z][A-Za-z0-9_]*)"#).expect("class decl re")
});

/// Strip generic type parameters: `List<User>` → `List`, `Map<String, Integer>` → `Map`
fn strip_generics(type_name: &str) -> &str {
    type_name.split('<').next().unwrap_or(type_name)
}

pub struct SpringResolver;

impl SpringResolver {
    /// Find the class-level @RequestMapping prefix, if any.
    fn find_class_prefix(source: &str) -> String {
        // Only take the first match (assuming one controller per file)
        CLASS_REQUEST_MAPPING_RE
            .captures(source)
            .and_then(|cap| cap.get(1))
            .map(|m| m.as_str().to_string())
            .unwrap_or_default()
    }

    /// Find the next Java method name declaration after a given byte offset.
    fn find_next_method_name(source: &str, after_offset: usize) -> Option<String> {
        let remaining = &source[after_offset..];
        JAVA_METHOD_NAME_RE
            .captures(remaining)
            .and_then(|cap| cap.get(1))
            .map(|m| m.as_str().to_string())
    }

    /// Extract injected dependency type names from source.
    /// Returns vec of (type_name, line) tuples.
    fn extract_injected_types(source: &str) -> Vec<(String, u32)> {
        let mut results = Vec::new();

        // 1. @Autowired field injection
        for cap in AUTOWIRED_FIELD_RE.captures_iter(source) {
            if let Some(type_match) = cap.get(1) {
                let type_name = strip_generics(type_match.as_str()).to_string();
                let line = line_for_offset(source, type_match.start());
                results.push((type_name, line));
            }
        }

        // 2. Constructor injection (with or without @Autowired on constructor)
        // Find the class name first
        let class_name = CLASS_DECL_RE
            .captures(source)
            .and_then(|cap| cap.get(1))
            .map(|m| m.as_str().to_string());

        for cap in CONSTRUCTOR_RE.captures_iter(source) {
            let ctor_name = cap.get(1).map(|m| m.as_str());
            let params = cap.get(2).map(|m| m.as_str()).unwrap_or("");
            let ctor_offset = cap.get(0).unwrap().start();
            let line = line_for_offset(source, ctor_offset);

            // Only treat as constructor if the method name matches the class name
            let is_constructor = match (&class_name, ctor_name) {
                (Some(cls), Some(name)) => cls == name,
                _ => false,
            };

            if !is_constructor {
                continue;
            }

            // Extract each parameter type
            for param_cap in PARAM_TYPE_RE.captures_iter(params) {
                if let Some(type_match) = param_cap.get(1) {
                    let type_name = strip_generics(type_match.as_str()).to_string();
                    // Skip common JDK/primitive types that aren't DI candidates
                    if !Self::is_di_candidate(&type_name) {
                        continue;
                    }
                    results.push((type_name, line));
                }
            }
        }

        // Deduplicate by type name (keep first occurrence)
        let mut seen = std::collections::HashSet::new();
        results.retain(|(name, _)| seen.insert(name.clone()));

        results
    }

    /// Check if a type name is likely a DI candidate (not a JDK primitive wrapper).
    fn is_di_candidate(type_name: &str) -> bool {
        !matches!(
            type_name,
            "String"
                | "Integer"
                | "Long"
                | "Double"
                | "Float"
                | "Boolean"
                | "Byte"
                | "Short"
                | "Character"
                | "Object"
                | "List"
                | "Map"
                | "Set"
                | "Collection"
                | "Optional"
                | "int"
                | "long"
                | "double"
                | "float"
                | "boolean"
                | "byte"
                | "short"
                | "char"
                | "void"
        )
    }
}

impl FrameworkResolver for SpringResolver {
    fn framework_key(&self) -> &str {
        "spring"
    }

    fn resolver_tier(&self) -> &'static str {
        "full"
    }

    fn languages(&self) -> &[Language] {
        &[Language::Java]
    }

    fn enrich_file(
        &self,
        file_path: &str,
        source: &str,
        _lang: Language,
        outcome: &mut ParseOutcome,
        _ctx: &ProjectFrameworkContext,
    ) {
        // Only process .java files
        if !file_path.ends_with(".java") {
            return;
        }

        let class_prefix = Self::find_class_prefix(source);

        // Extract @GetMapping/@PostMapping/etc. annotations
        for cap in METHOD_MAPPING_RE.captures_iter(source) {
            let http_verb = cap.get(1).map(|m| m.as_str()).unwrap_or("GET");
            let method_path = cap.get(2).map(|m| m.as_str()).unwrap_or("");

            let annotation_offset = cap.get(0).unwrap().start();
            let line = line_for_offset(source, annotation_offset);

            let handler_name = Self::find_next_method_name(source, annotation_offset);

            let route_path = if method_path.is_empty() {
                if class_prefix.is_empty() {
                    "/".to_string()
                } else {
                    class_prefix.clone()
                }
            } else {
                format!("{}{}", class_prefix, method_path)
            };

            outcome.route_edges.push(RouteEdgeRecord {
                edge_id: StableId::edge_id("route", file_path, line, 0),
                file_path: file_path.to_string(),
                route_path,
                handler_name,
                method: Some(http_verb.to_uppercase()),
                line,
                start_col: 0,
                end_line: None,
                end_col: 0,
                handler_symbol_id: None,
                handler_symbol_uid: None,
                handler_expr: None,
                router_symbol_uid: None,
                framework: Some("spring".to_string()),
                route_kind: Some("http".to_string()),
                confidence: 0.88,
                parser_tier: ParserTier::Heuristic,
            });
        }

        // Extract @RequestMapping with explicit method attribute
        for cap in METHOD_REQUEST_MAPPING_RE.captures_iter(source) {
            let method_path = cap.get(1).map(|m| m.as_str()).unwrap_or("");
            let http_verb = cap.get(2).map(|m| m.as_str()).unwrap_or("GET");

            let annotation_offset = cap.get(0).unwrap().start();
            let line = line_for_offset(source, annotation_offset);

            let handler_name = Self::find_next_method_name(source, annotation_offset);

            let route_path = format!("{}{}", class_prefix, method_path);

            outcome.route_edges.push(RouteEdgeRecord {
                edge_id: StableId::edge_id("route", file_path, line, 0),
                file_path: file_path.to_string(),
                route_path,
                handler_name,
                method: Some(http_verb.to_uppercase()),
                line,
                start_col: 0,
                end_line: None,
                end_col: 0,
                handler_symbol_id: None,
                handler_symbol_uid: None,
                handler_expr: None,
                router_symbol_uid: None,
                framework: Some("spring".to_string()),
                route_kind: Some("http".to_string()),
                confidence: 0.85,
                parser_tier: ParserTier::Heuristic,
            });
        }

        // Extract DI dependencies (@Autowired fields + constructor injection)
        let class_name = CLASS_DECL_RE
            .captures(source)
            .and_then(|cap| cap.get(1))
            .map(|m| m.as_str().to_string());

        if let Some(ref class_name) = class_name {
            let injected_types = Self::extract_injected_types(source);
            for (type_name, line) in injected_types {
                outcome.semantic_edges.push(SemanticEdgeRecord {
                    edge_id: StableId::edge_id("spring_di", file_path, line, 0),
                    file_path: file_path.to_string(),
                    source_symbol: class_name.clone(),
                    source_symbol_uid: None, // resolved in cross-file phase
                    target_symbol: type_name,
                    target_symbol_uid: None, // resolved in cross-file phase
                    relation_kind: SemanticRelation::UsesType,
                    line,
                    confidence: 0.80,
                    parser_tier: ParserTier::Heuristic,
                });
            }
        }
    }

    fn resolve_cross_file(
        &self,
        catalog: &crate::resolver::SymbolCatalog,
        outcomes: &mut [(String, ParseOutcome)],
        _ctx: &ProjectFrameworkContext,
    ) {
        // Phase 1: Build a map of Spring stereotype classes.
        // Scan all outcomes for files that have @Service/@Repository/@Component etc.
        // Map: class_name (lowercase) → (symbol_uid, file_path)
        let mut stereotype_classes: std::collections::HashMap<String, (String, String)> =
            std::collections::HashMap::new();

        for (file_path, outcome) in outcomes.iter() {
            // Check if any symbol in this file is a class with a Spring stereotype
            // We look for class symbols whose source file contains stereotype annotations.
            // Since we can't re-read source here, we detect stereotype presence from
            // semantic_edges: if the file has a "spring_di" edge or the file contains
            // class symbols, we check the catalog.
            for sym in &outcome.symbols {
                if sym.kind == SymbolKind::Class {
                    // Register all classes — we'll filter by stereotype below
                    let uid = sym.symbol_uid.clone().unwrap_or_default();
                    stereotype_classes
                        .entry(sym.name.to_lowercase())
                        .or_insert((uid, file_path.clone()));
                }
            }
        }

        // Phase 2: For each file with spring_di UsesType edges, resolve the target type
        // against the catalog and upgrade to Injects edges with bound UIDs.
        //
        // Collect mutations to apply, then apply them (avoids borrow issues).
        struct EdgePatch {
            file_idx: usize,
            edge_idx: usize,
            source_uid: String,
            target_uid: String,
        }

        let mut patches: Vec<EdgePatch> = Vec::new();

        for (file_idx, (file_path, outcome)) in outcomes.iter().enumerate() {
            let fp = file_path.clone();

            // Find the controller/component class UID in this file
            let source_class_uid: Option<String> = outcome
                .symbols
                .iter()
                .find(|s| s.kind == SymbolKind::Class)
                .and_then(|s| s.symbol_uid.clone());

            for (edge_idx, edge) in outcome.semantic_edges.iter().enumerate() {
                // Only process our spring_di edges
                if edge.relation_kind != SemanticRelation::UsesType {
                    continue;
                }
                if !edge.edge_id.starts_with("spring_di") {
                    continue;
                }
                if edge.target_symbol_uid.is_some() {
                    continue; // already resolved
                }

                let target_type = &edge.target_symbol;

                // Try to resolve the target type via catalog
                let resolved = catalog.lookup_symbol(target_type, &fp).or_else(|| {
                    // Fallback: look up in stereotype_classes map
                    stereotype_classes
                        .get(&target_type.to_lowercase())
                        .map(|(uid, file)| (uid.clone(), file.clone()))
                });

                if let Some((target_uid, _target_file)) = resolved {
                    if !target_uid.is_empty() {
                        patches.push(EdgePatch {
                            file_idx,
                            edge_idx,
                            source_uid: source_class_uid.clone().unwrap_or_default(),
                            target_uid,
                        });
                    }
                }
            }
        }

        // Apply patches: upgrade UsesType → Injects and bind UIDs
        for patch in &patches {
            let edge = &mut outcomes[patch.file_idx].1.semantic_edges[patch.edge_idx];
            edge.relation_kind = SemanticRelation::Injects;
            edge.source_symbol_uid = Some(patch.source_uid.clone());
            edge.target_symbol_uid = Some(patch.target_uid.clone());
            edge.confidence = 0.90;
        }

        // Phase 3: Resolve handler_symbol_uid for route_edges (same as NestJS pattern)
        for (file_path, outcome) in outcomes.iter_mut() {
            let fp = file_path.clone();

            let controller_class: Option<String> = outcome
                .symbols
                .iter()
                .find(|s| s.kind == SymbolKind::Class)
                .map(|s| s.name.clone());

            for edge in &mut outcome.route_edges {
                if edge.framework.as_deref() != Some("spring") {
                    continue;
                }
                if edge.handler_symbol_uid.is_some() {
                    continue;
                }
                if let Some(ref handler_name) = edge.handler_name {
                    // Try qualified name first: ClassName.methodName
                    let resolved = controller_class
                        .as_ref()
                        .and_then(|cls| {
                            let qname = format!("{}.{}", cls, handler_name);
                            catalog.lookup_symbol(&qname, &fp)
                        })
                        .or_else(|| catalog.lookup_symbol(handler_name, &fp));

                    if let Some((uid, _)) = resolved {
                        if !uid.is_empty() {
                            edge.handler_symbol_uid = Some(uid);
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_spring(file_path: &str, source: &str) -> ParseOutcome {
        let mut outcome = ParseOutcome::default();
        let ctx = ProjectFrameworkContext::new();
        SpringResolver.enrich_file(file_path, source, Language::Java, &mut outcome, &ctx);
        outcome
    }

    fn run_spring_routes(file_path: &str, source: &str) -> Vec<RouteEdgeRecord> {
        run_spring(file_path, source).route_edges
    }

    #[test]
    fn test_spring_get_and_post_mapping() {
        let source = r#"
@RestController
@RequestMapping("/api/v1")
public class UserController {

    @GetMapping("/users")
    public List<User> getUsers() {
        return userService.findAll();
    }

    @PostMapping("/users")
    public User createUser(@RequestBody User user) {
        return userService.save(user);
    }
}
"#;
        let routes = run_spring_routes("UserController.java", source);
        assert_eq!(routes.len(), 2, "expected 2 routes, got {}", routes.len());

        let get_route = routes.iter().find(|r| r.method == Some("GET".into()));
        assert!(get_route.is_some(), "should have a GET route");
        let get_route = get_route.unwrap();
        assert_eq!(get_route.route_path, "/api/v1/users");
        assert_eq!(get_route.handler_name.as_deref(), Some("getUsers"));
        assert_eq!(get_route.framework.as_deref(), Some("spring"));

        let post_route = routes.iter().find(|r| r.method == Some("POST".into()));
        assert!(post_route.is_some(), "should have a POST route");
        assert_eq!(post_route.unwrap().route_path, "/api/v1/users");
        assert_eq!(
            post_route.unwrap().handler_name.as_deref(),
            Some("createUser")
        );
    }

    #[test]
    fn test_spring_no_class_prefix() {
        let source = r#"
@RestController
public class SimpleController {

    @GetMapping("/health")
    public String health() {
        return "ok";
    }

    @DeleteMapping("/cache")
    public void clearCache() {}
}
"#;
        let routes = run_spring_routes("SimpleController.java", source);
        assert_eq!(routes.len(), 2);
        assert!(routes
            .iter()
            .any(|r| r.route_path == "/health" && r.method == Some("GET".into())));
        assert!(routes
            .iter()
            .any(|r| r.route_path == "/cache" && r.method == Some("DELETE".into())));
    }

    #[test]
    fn test_spring_bare_mapping() {
        let source = r#"
@RestController
@RequestMapping("/api")
public class RootController {

    @GetMapping
    public String index() { return "root"; }
}
"#;
        let routes = run_spring_routes("RootController.java", source);
        assert_eq!(routes.len(), 1);
        // Bare @GetMapping with class prefix → just the prefix
        assert_eq!(routes[0].route_path, "/api");
        assert_eq!(routes[0].method, Some("GET".into()));
    }

    #[test]
    fn test_spring_request_mapping_with_method() {
        let source = r#"
@Controller
@RequestMapping("/legacy")
public class LegacyController {

    @RequestMapping(value = "/action", method = RequestMethod.POST)
    public String doAction() { return "done"; }
}
"#;
        let routes = run_spring_routes("LegacyController.java", source);
        assert!(
            routes.iter().any(|r| r.route_path == "/legacy/action"
                && r.method == Some("POST".into())
                && r.handler_name.as_deref() == Some("doAction")),
            "should extract @RequestMapping with method attr, got: {:?}",
            routes
        );
    }

    #[test]
    fn test_spring_ignores_non_java() {
        let source = r#"@GetMapping("/foo") public void foo() {}"#;
        let routes = run_spring_routes("Test.kt", source);
        assert!(routes.is_empty(), "should ignore non-.java files");
    }

    #[test]
    fn test_spring_all_http_verbs() {
        let source = r#"
@RestController
public class VerbController {
    @GetMapping("/a") public void a() {}
    @PostMapping("/b") public void b() {}
    @PutMapping("/c") public void c() {}
    @DeleteMapping("/d") public void d() {}
    @PatchMapping("/e") public void e() {}
}
"#;
        let routes = run_spring_routes("VerbController.java", source);
        assert_eq!(routes.len(), 5, "should extract all 5 HTTP verb mappings");
        let methods: Vec<String> = routes.iter().filter_map(|r| r.method.clone()).collect();
        assert!(methods.contains(&"GET".to_string()));
        assert!(methods.contains(&"POST".to_string()));
        assert!(methods.contains(&"PUT".to_string()));
        assert!(methods.contains(&"DELETE".to_string()));
        assert!(methods.contains(&"PATCH".to_string()));
    }

    // -----------------------------------------------------------------------
    // DI injection detection tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_spring_autowired_field_injection() {
        let source = r#"
@RestController
@RequestMapping("/api/users")
public class UserController {

    @Autowired
    private UserService userService;

    @Autowired
    private NotificationService notificationService;

    @GetMapping("/{id}")
    public User getUser(@PathVariable Long id) {
        return userService.findById(id);
    }
}
"#;
        let outcome = run_spring("UserController.java", source);

        // Should detect 2 injected types
        let di_edges: Vec<_> = outcome
            .semantic_edges
            .iter()
            .filter(|e| e.relation_kind == SemanticRelation::UsesType)
            .collect();
        assert_eq!(
            di_edges.len(),
            2,
            "expected 2 DI edges, got {}",
            di_edges.len()
        );

        assert!(
            di_edges.iter().any(|e| e.target_symbol == "UserService"),
            "should detect UserService injection"
        );
        assert!(
            di_edges
                .iter()
                .any(|e| e.target_symbol == "NotificationService"),
            "should detect NotificationService injection"
        );

        // All edges should have source_symbol = class name
        assert!(
            di_edges.iter().all(|e| e.source_symbol == "UserController"),
            "source_symbol should be the controller class name"
        );
    }

    #[test]
    fn test_spring_constructor_injection() {
        let source = r#"
@RestController
@RequestMapping("/api/orders")
public class OrderController {

    private final OrderService orderService;
    private final PaymentService paymentService;

    public OrderController(OrderService orderService, PaymentService paymentService) {
        this.orderService = orderService;
        this.paymentService = paymentService;
    }

    @GetMapping("/{id}")
    public Order getOrder(@PathVariable Long id) {
        return orderService.findById(id);
    }
}
"#;
        let outcome = run_spring("OrderController.java", source);

        let di_edges: Vec<_> = outcome
            .semantic_edges
            .iter()
            .filter(|e| e.relation_kind == SemanticRelation::UsesType)
            .collect();
        assert_eq!(
            di_edges.len(),
            2,
            "expected 2 constructor-injected DI edges, got {}",
            di_edges.len()
        );

        assert!(
            di_edges.iter().any(|e| e.target_symbol == "OrderService"),
            "should detect OrderService constructor injection"
        );
        assert!(
            di_edges.iter().any(|e| e.target_symbol == "PaymentService"),
            "should detect PaymentService constructor injection"
        );
    }

    #[test]
    fn test_spring_constructor_injection_with_autowired() {
        let source = r#"
@RestController
public class ProductController {

    private final ProductService productService;

    @Autowired
    public ProductController(ProductService productService) {
        this.productService = productService;
    }
}
"#;
        let outcome = run_spring("ProductController.java", source);

        let di_edges: Vec<_> = outcome
            .semantic_edges
            .iter()
            .filter(|e| e.relation_kind == SemanticRelation::UsesType)
            .collect();
        assert_eq!(
            di_edges.len(),
            1,
            "expected 1 DI edge, got {}",
            di_edges.len()
        );
        assert_eq!(di_edges[0].target_symbol, "ProductService");
    }

    #[test]
    fn test_spring_mixed_injection_deduplicates() {
        let source = r#"
@RestController
public class MixedController {

    @Autowired
    private UserService userService;

    public MixedController(UserService userService) {
        this.userService = userService;
    }
}
"#;
        let outcome = run_spring("MixedController.java", source);

        let di_edges: Vec<_> = outcome
            .semantic_edges
            .iter()
            .filter(|e| e.relation_kind == SemanticRelation::UsesType)
            .collect();
        assert_eq!(
            di_edges.len(),
            1,
            "should deduplicate: expected 1 DI edge, got {}",
            di_edges.len()
        );
        assert_eq!(di_edges[0].target_symbol, "UserService");
    }

    #[test]
    fn test_spring_ignores_primitive_types_in_constructor() {
        let source = r#"
@RestController
public class ConfigController {

    public ConfigController(String appName, UserService userService, Integer maxRetries) {
        // ...
    }
}
"#;
        let outcome = run_spring("ConfigController.java", source);

        let di_edges: Vec<_> = outcome
            .semantic_edges
            .iter()
            .filter(|e| e.relation_kind == SemanticRelation::UsesType)
            .collect();
        assert_eq!(
            di_edges.len(),
            1,
            "should only detect UserService, not String/Integer"
        );
        assert_eq!(di_edges[0].target_symbol, "UserService");
    }

    #[test]
    fn test_spring_generic_autowired_type() {
        let source = r#"
@RestController
public class GenericController {

    @Autowired
    private Repository<User> userRepository;
}
"#;
        let outcome = run_spring("GenericController.java", source);

        let di_edges: Vec<_> = outcome
            .semantic_edges
            .iter()
            .filter(|e| e.relation_kind == SemanticRelation::UsesType)
            .collect();
        assert_eq!(di_edges.len(), 1);
        // Should strip generics: Repository<User> → Repository
        assert_eq!(di_edges[0].target_symbol, "Repository");
    }

    #[test]
    fn test_spring_no_di_in_non_java_file() {
        let source = r#"
@RestController
public class FakeController {
    @Autowired
    private SomeService someService;
}
"#;
        let outcome = run_spring("FakeController.kt", source);
        assert!(outcome.semantic_edges.is_empty());
    }

    // -----------------------------------------------------------------------
    // Cross-file resolution tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_resolve_cross_file_upgrades_to_injects() {
        use crate::resolver::SymbolCatalog;
        use cc_model::symbol::SymbolRecord;

        // Set up: controller file with DI edge, service file with class symbol
        let controller_source = r#"
@RestController
@RequestMapping("/api/users")
public class UserController {
    @Autowired
    private UserService userService;

    @GetMapping("/{id}")
    public User getUser(@PathVariable Long id) {
        return userService.findById(id);
    }
}
"#;
        let service_source = r#"
@Service
public class UserService {
    public User findById(Long id) { return null; }
}
"#;

        let mut controller_outcome = ParseOutcome::default();
        let mut service_outcome = ParseOutcome::default();
        let ctx = ProjectFrameworkContext::new();

        SpringResolver.enrich_file(
            "UserController.java",
            controller_source,
            Language::Java,
            &mut controller_outcome,
            &ctx,
        );
        SpringResolver.enrich_file(
            "UserService.java",
            service_source,
            Language::Java,
            &mut service_outcome,
            &ctx,
        );

        // Add class symbols to outcomes (simulating parser output)
        controller_outcome.symbols.push(SymbolRecord {
            symbol_id: "ctrl_id".to_string(),
            symbol_uid: Some("uid:UserController".to_string()),
            name: "UserController".to_string(),
            kind: SymbolKind::Class,
            file_path: "UserController.java".to_string(),
            start_line: 3,
            end_line: 12,
            start_col: 0,
            end_col: 0,
            container: None,
            signature: None,
            doc: None,
            parser_tier: ParserTier::Heuristic,
            parser_confidence: 0.9,
            qname: None,
            parent_symbol_id: None,
            scope_id: None,
            export_name: None,
            is_default_export: false,
            framework_role: None,
            receiver_type: None,
            param_types: None,
            return_type: None,
            param_count: None,
            base_types: None,
            implements: None,
        });
        service_outcome.symbols.push(SymbolRecord {
            symbol_id: "svc_id".to_string(),
            symbol_uid: Some("uid:UserService".to_string()),
            name: "UserService".to_string(),
            kind: SymbolKind::Class,
            file_path: "UserService.java".to_string(),
            start_line: 2,
            end_line: 5,
            start_col: 0,
            end_col: 0,
            container: None,
            signature: None,
            doc: None,
            parser_tier: ParserTier::Heuristic,
            parser_confidence: 0.9,
            qname: None,
            parent_symbol_id: None,
            scope_id: None,
            export_name: None,
            is_default_export: false,
            framework_role: None,
            receiver_type: None,
            param_types: None,
            return_type: None,
            param_count: None,
            base_types: None,
            implements: None,
        });

        // Build catalog
        let mut catalog = SymbolCatalog::new();
        catalog.add_symbols(&controller_outcome.symbols);
        catalog.add_symbols(&service_outcome.symbols);

        // Verify pre-condition: UsesType edge exists
        assert_eq!(
            controller_outcome
                .semantic_edges
                .iter()
                .filter(|e| e.relation_kind == SemanticRelation::UsesType)
                .count(),
            1
        );

        // Run cross-file resolution
        let mut outcomes = vec![
            ("UserController.java".to_string(), controller_outcome),
            ("UserService.java".to_string(), service_outcome),
        ];
        SpringResolver.resolve_cross_file(&catalog, &mut outcomes, &ctx);

        // Verify: edge should be upgraded to Injects
        let controller_edges = &outcomes[0].1.semantic_edges;
        let injects_edges: Vec<_> = controller_edges
            .iter()
            .filter(|e| e.relation_kind == SemanticRelation::Injects)
            .collect();
        assert_eq!(
            injects_edges.len(),
            1,
            "expected 1 Injects edge, got {}",
            injects_edges.len()
        );

        let edge = injects_edges[0];
        assert_eq!(edge.source_symbol, "UserController");
        assert_eq!(
            edge.source_symbol_uid.as_deref(),
            Some("uid:UserController")
        );
        assert_eq!(edge.target_symbol, "UserService");
        assert_eq!(edge.target_symbol_uid.as_deref(), Some("uid:UserService"));
        assert_eq!(edge.confidence, 0.90);

        // Verify no UsesType edges remain for spring_di
        let uses_type_edges: Vec<_> = controller_edges
            .iter()
            .filter(|e| {
                e.relation_kind == SemanticRelation::UsesType && e.edge_id.starts_with("spring_di")
            })
            .collect();
        assert!(
            uses_type_edges.is_empty(),
            "UsesType spring_di edges should have been upgraded to Injects"
        );
    }
}
