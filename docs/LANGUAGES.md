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

## Detected framework signals

Recognized via manifest files and import patterns but without a dedicated
resolver (detection-only, no semantic enrichment):

Koa, Fastify, Next.js, Nuxt, Angular, Rocket, Remix, Vue Router, net/http
