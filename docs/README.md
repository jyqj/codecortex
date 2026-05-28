# CodeCortex Documentation

Start at the project [README](../README.md) for a quick overview, then dive in:

| Doc | What's inside |
|-----|---------------|
| [../DESIGN.md](../DESIGN.md) | Design charter — dependency overview, principles, and explicit non-goals. |
| [ARCHITECTURE.md](ARCHITECTURE.md) | Crate-by-crate breakdown, data flow, SQLite schema, and key internal components (CodeIndex, ImpactAnalyzer, FileWatcher). |
| [MCP_TOOLS.md](MCP_TOOLS.md) | Reference for all 14 MCP tools, the recommended agent usage path, anti-patterns, and CLI commands. |
| [CONFIGURATION.md](CONFIGURATION.md) | `.codecortex.json` schema, embedding providers (incl. the `none` circuit break), repo size tiers, and environment overrides. |
| [LANGUAGES.md](LANGUAGES.md) | Language extraction tiers and the 16 semantic framework resolvers. |
| [CYPHER.md](CYPHER.md) | The read-only Cypher subset used by `graph_query`. |
| [TEST_PLAN.md](TEST_PLAN.md) | Unit test layout, the eval corpus, assertion types, and the fixture project. |
| [BENCHMARK.md](BENCHMARK.md) | Target metrics and how to run the fixture and real-workspace benchmarks. |
| [../CONTRIBUTING.md](../CONTRIBUTING.md) | Build, test, lint, MSRV, and pre-commit commands. |

Generated benchmark reports live under [benchmarks/](benchmarks/).
