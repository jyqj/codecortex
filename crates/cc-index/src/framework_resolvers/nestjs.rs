//! NestJS framework resolver.
//!
//! - `enrich_file`: extracts route definitions from NestJS decorators
//! - `resolve_cross_file`: resolves module DI graph

use cc_model::edge::RouteEdgeRecord;
use cc_model::id::StableId;
use cc_model::parse::ParseOutcome;
use cc_model::{Language, ParserTier};
use regex::Regex;
use std::sync::LazyLock;

use super::{line_for_offset, FrameworkResolver, ProjectFrameworkContext};

// ---------------------------------------------------------------------------
// Regex patterns
// ---------------------------------------------------------------------------

/// @Controller("prefix") or @Controller('prefix')
/// Also matches @Controller() (no prefix).
///
/// Captures: (1) optional prefix string (without quotes)
static CONTROLLER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)@Controller\(\s*(?:["']([^"']*)["'])?\s*\)"#).expect("nestjs controller re")
});

/// Method-level route decorators: @Get("path"), @Post(), @Put("/:id"), etc.
///
/// Captures: (1) HTTP method name, (2) optional route path
static METHOD_DECORATOR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?m)@(Get|Post|Put|Delete|Patch|Head|Options|All)\(\s*(?:["']([^"']*)["'])?\s*\)"#,
    )
    .expect("nestjs method decorator re")
});

/// Extracts a method name from a TypeScript method declaration line.
/// Matches: `async getUsers(` or `createUser(` or `public async findOne(`
///
/// Captures: (1) method name
static TS_METHOD_NAME_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)(?:public|protected|private|static|async|\s)*(\w+)\s*\(")
        .expect("ts method name re")
});

pub struct NestJsResolver;

impl NestJsResolver {
    /// Find the class-level @Controller prefix, if any.
    /// Returns empty string if @Controller() has no argument or is not found.
    fn find_controller_prefix(source: &str) -> String {
        CONTROLLER_RE
            .captures(source)
            .and_then(|cap| cap.get(1))
            .map(|m| {
                let prefix = m.as_str();
                // Normalize: ensure prefix starts with /
                if prefix.is_empty() {
                    String::new()
                } else if prefix.starts_with('/') {
                    prefix.to_string()
                } else {
                    format!("/{}", prefix)
                }
            })
            .unwrap_or_default()
    }

    /// Find the next TypeScript method name after a given byte offset.
    fn find_next_method_name(source: &str, after_offset: usize) -> Option<String> {
        let remaining = &source[after_offset..];
        TS_METHOD_NAME_RE
            .captures(remaining)
            .and_then(|cap| cap.get(1))
            .map(|m| m.as_str().to_string())
    }

    /// Combine controller prefix with method path.
    fn combine_paths(prefix: &str, method_path: &str) -> String {
        if method_path.is_empty() {
            if prefix.is_empty() {
                "/".to_string()
            } else {
                prefix.to_string()
            }
        } else {
            let normalized = if method_path.starts_with('/') {
                method_path.to_string()
            } else {
                format!("/{}", method_path)
            };
            if prefix.is_empty() {
                normalized
            } else {
                format!("{}{}", prefix, normalized)
            }
        }
    }
}

impl FrameworkResolver for NestJsResolver {
    fn framework_key(&self) -> &str {
        "nestjs"
    }

    fn resolver_tier(&self) -> &'static str {
        "full"
    }

    fn languages(&self) -> &[Language] {
        &[Language::TypeScript]
    }

    fn enrich_file(
        &self,
        file_path: &str,
        source: &str,
        _lang: Language,
        outcome: &mut ParseOutcome,
        _ctx: &ProjectFrameworkContext,
    ) {
        // Only process TypeScript files
        if !file_path.ends_with(".ts") {
            return;
        }

        let controller_prefix = Self::find_controller_prefix(source);

        // Extract @Get/@Post/etc. decorators
        for cap in METHOD_DECORATOR_RE.captures_iter(source) {
            let http_verb = cap.get(1).map(|m| m.as_str()).unwrap_or("GET");
            let method_path = cap.get(2).map(|m| m.as_str()).unwrap_or("");

            let decorator_offset = cap.get(0).unwrap().start();
            let decorator_end = cap.get(0).unwrap().end();
            let line = line_for_offset(source, decorator_offset);

            let handler_name = Self::find_next_method_name(source, decorator_end);

            let route_path = Self::combine_paths(&controller_prefix, method_path);

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
                framework: Some("nestjs".to_string()),
                route_kind: Some("http".to_string()),
                confidence: 0.88,
                parser_tier: ParserTier::Heuristic,
            });
        }
    }

    fn resolve_cross_file(
        &self,
        catalog: &crate::resolver::SymbolCatalog,
        outcomes: &mut [(String, ParseOutcome)],
        _ctx: &ProjectFrameworkContext,
    ) {
        // Resolve handler_symbol_uid for NestJS route_edges.
        //
        // NestJS controllers define methods decorated with @Get/@Post/etc.
        // The handler_name captured during enrich_file is the method name.
        // We look up that method name in the catalog to bind the UID.
        //
        // For class-qualified lookups, try "ClassName.methodName" first,
        // then fall back to just "methodName".
        for (file_path, outcome) in outcomes.iter_mut() {
            let fp = file_path.clone();

            // Try to extract the controller class name from symbols in this file
            let controller_class: Option<String> = outcome
                .symbols
                .iter()
                .find(|s| s.kind == cc_model::SymbolKind::Class)
                .map(|s| s.name.clone());

            for edge in &mut outcome.route_edges {
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

    fn run_nestjs(file_path: &str, source: &str) -> Vec<RouteEdgeRecord> {
        let mut outcome = ParseOutcome::default();
        let ctx = ProjectFrameworkContext::new();
        NestJsResolver.enrich_file(file_path, source, Language::TypeScript, &mut outcome, &ctx);
        outcome.route_edges
    }

    #[test]
    fn test_nestjs_controller_with_prefix() {
        let source = r#"
import { Controller, Get, Post } from '@nestjs/common';

@Controller("users")
export class UsersController {

    @Get()
    async findAll() {
        return this.usersService.findAll();
    }

    @Post()
    async create(@Body() dto: CreateUserDto) {
        return this.usersService.create(dto);
    }

    @Get("/:id")
    async findOne(@Param("id") id: string) {
        return this.usersService.findOne(id);
    }
}
"#;
        let routes = run_nestjs("src/users/users.controller.ts", source);
        assert_eq!(routes.len(), 3, "expected 3 routes, got {}", routes.len());

        let get_all = routes
            .iter()
            .find(|r| r.method == Some("GET".into()) && r.route_path == "/users");
        assert!(get_all.is_some(), "should have GET /users");
        assert_eq!(get_all.unwrap().handler_name.as_deref(), Some("findAll"));

        let post_route = routes.iter().find(|r| r.method == Some("POST".into()));
        assert!(post_route.is_some(), "should have POST /users");
        assert_eq!(post_route.unwrap().route_path, "/users");
        assert_eq!(post_route.unwrap().handler_name.as_deref(), Some("create"));

        let get_one = routes
            .iter()
            .find(|r| r.method == Some("GET".into()) && r.route_path == "/users/:id");
        assert!(get_one.is_some(), "should have GET /users/:id");
        assert_eq!(get_one.unwrap().handler_name.as_deref(), Some("findOne"));

        assert!(routes
            .iter()
            .all(|r| r.framework.as_deref() == Some("nestjs")));
    }

    #[test]
    fn test_nestjs_controller_no_prefix() {
        let source = r#"
import { Controller, Get } from '@nestjs/common';

@Controller()
export class AppController {

    @Get()
    getHello() {
        return "Hello World";
    }

    @Get("/health")
    health() {
        return { status: "ok" };
    }
}
"#;
        let routes = run_nestjs("src/app.controller.ts", source);
        assert_eq!(routes.len(), 2, "expected 2 routes, got {}", routes.len());

        assert!(routes
            .iter()
            .any(|r| r.route_path == "/" && r.method == Some("GET".into())));
        assert!(routes
            .iter()
            .any(|r| r.route_path == "/health" && r.method == Some("GET".into())));
    }

    #[test]
    fn test_nestjs_multiple_http_verbs() {
        let source = r#"
import { Controller, Get, Post, Put, Delete, Patch } from '@nestjs/common';

@Controller("/api/items")
export class ItemsController {

    @Get()
    findAll() {}

    @Post()
    create() {}

    @Put("/:id")
    update() {}

    @Delete("/:id")
    remove() {}

    @Patch("/:id")
    partialUpdate() {}
}
"#;
        let routes = run_nestjs("src/items/items.controller.ts", source);
        assert_eq!(routes.len(), 5, "expected 5 routes, got {}", routes.len());

        let methods: Vec<String> = routes.iter().filter_map(|r| r.method.clone()).collect();
        assert!(methods.contains(&"GET".to_string()));
        assert!(methods.contains(&"POST".to_string()));
        assert!(methods.contains(&"PUT".to_string()));
        assert!(methods.contains(&"DELETE".to_string()));
        assert!(methods.contains(&"PATCH".to_string()));

        // Verify path combination
        assert!(routes
            .iter()
            .any(|r| r.route_path == "/api/items" && r.method == Some("GET".into())));
        assert!(routes
            .iter()
            .any(|r| r.route_path == "/api/items/:id" && r.method == Some("PUT".into())));
    }

    #[test]
    fn test_nestjs_ignores_non_ts_files() {
        let source = r#"@Controller("test") @Get() findAll() {}"#;
        let routes = run_nestjs("test.js", source);
        assert!(routes.is_empty(), "should ignore non-.ts files");
    }

    #[test]
    fn test_nestjs_controller_with_slash_prefix() {
        let source = r#"
@Controller("/api/v1/products")
export class ProductsController {
    @Get("/:id")
    findOne() {}
}
"#;
        let routes = run_nestjs("src/products.controller.ts", source);
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].route_path, "/api/v1/products/:id");
    }
}
