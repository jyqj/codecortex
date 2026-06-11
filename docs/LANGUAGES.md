# Language & Framework Support

CodeCortex recognizes 30 auto-detected language identifiers (plus an `Unknown`
fallback) across three extraction tiers, and ships 16 semantic framework
resolvers.

## Extraction tiers

### Full tree-sitter parsing (confidence 0.7+)

Python, JavaScript, TypeScript, TSX, JSX, Java, Go, Rust, C, C++

All of these extract symbols, call edges, imports, data-flow edges (env access +
param/return flow), and semantic edges. The remaining edge kinds are
language-specific:

- **Route edges:** Python, JS/TS, Go at the parser level; Java (Spring) and Rust
  (Actix / Axum) via framework resolvers.
- **Outbound HTTP call edges:** Python and JS/TS at the AST level; Go
  (`net/http`), Java (RestTemplate / WebClient), and Rust (reqwest) via
  conservative pattern matching guarded by a URL-shape check.
- **Test edges, dispatch sites, type assignments:** most complete for Python and
  JS/TS; partial for the other languages.

### Heuristic / generic fallback (confidence 0.3 – 0.5)

C#, PHP, Ruby, Swift, Kotlin, Dart, Scala, Lua, Vue, Svelte, Markdown, SQL,
YAML, TOML, HCL, Dockerfile, Bash, Protobuf, GraphQL, CMake

Heuristic extraction captures symbols, imports, and best-effort intra-file call
edges via pattern matching; it does not resolve cross-file calls or type
hierarchies.

### Confidence tiers

| Tier | Score | Source |
|------|-------|--------|
| Generic | 0.3 | Regex-based extraction |
| Heuristic | 0.5 | Pattern matching with language awareness |
| TreeSitter | 0.7 | Full AST parsing |
| Semantic | 0.85 | Cross-reference resolved |
| Verified | 0.95 | Runtime-validated (via `ingest_traces`) |

## Extraction capability notes

Edge extraction timing is deliberately asymmetric between the parser layer
(cc-parsers) and the framework-resolver layer (cc-index):

- **Route edges** are extracted at parse time for Go, Python, and JS/TS
  (`crates/cc-parsers/src/{go.rs, python/mod.rs, jsts/mod.rs}`). Java has no
  parse-time route extraction: Spring routes are synthesized entirely by the
  framework resolver (`crates/cc-index/src/framework_resolvers/spring.rs`).
  Go routes are additionally enriched by `go_router.rs` (group/mount prefixes,
  cross-file handler UIDs).
- **Dispatch sites** are produced only by the Python, JS/TS, and Vue SFC
  parsers (`python/mod.rs`, `jsts/mod.rs`, `sfc.rs`).
- **Outbound HTTP call edges** come from AST extraction in Python and JS/TS,
  and from the shared conservative pattern matcher
  (`crates/cc-parsers/src/http_call_helpers.rs`) for Go, Java, and Rust.

## Semantic framework resolvers (16)

Resolvers attach routes and handlers to the code graph and, at the **full** tier,
resolve handler references across files.

### Full (15) — routes + handlers + cross-file resolution

| Language | Frameworks |
|----------|-----------|
| JavaScript / TypeScript | Express, NestJS, Hono, React, Vue, Svelte / SvelteKit |
| Python | Django, Flask, FastAPI |
| Go | Gin / Echo / Fiber / Chi / Gorilla (unified) |
| Java | Spring / Spring Boot |
| Rust | Actix-web, Axum |
| PHP | Laravel |
| Ruby | Rails |

### Partial (1) — handler UID resolution only

| Language | Framework |
|----------|-----------|
| C# | ASP.NET |

### Adding a framework resolver

Create `crates/cc-index/src/framework_resolvers/<framework>.rs`, implement the
`FrameworkResolver` trait, and register it with one `registry.register(...)`
line in `default_registry()`
([`framework_resolvers/mod.rs`](../crates/cc-index/src/framework_resolvers/mod.rs)).
[`fastapi.rs`](../crates/cc-index/src/framework_resolvers/fastapi.rs) is a
compact full-tier reference. For HTTP frameworks with mount/prefix semantics
(routers, blueprints, URL includes), declare a `MountSpec` and delegate
`resolve_cross_file` to the shared
[`mount_resolution.rs`](../crates/cc-index/src/framework_resolvers/mount_resolution.rs)
core instead of hand-writing the collect → prefix → bind-UID steps. See the
[Extension points](ARCHITECTURE.md#extension-points) catalog in
ARCHITECTURE.md for the other seams.

## Detected framework signals

Recognized via manifest files and import patterns but without a dedicated
resolver (detection-only, no semantic enrichment):

Koa, Fastify, Next.js, Nuxt, Angular, Rocket, Remix, Vue Router, net/http
