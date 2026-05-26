use crate::engine::CodeIndex;
use cc_model::config::{find_project_root, load_project_config, IndexPaths};
use cc_model::{CcError, CcResult, Intent};
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Parser)]
#[command(
    name = "codecortex",
    version,
    about = "Code indexing and code graph search"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Initialize project configuration
    #[command(name = "init-project", alias = "init")]
    InitProject {
        #[arg(long)]
        project_path: Option<PathBuf>,
    },
    /// Build or update the code index
    #[command(name = "index")]
    Index {
        #[arg(long)]
        project_path: Option<PathBuf>,
        #[arg(long)]
        full: bool,
    },
    /// Show index status and statistics
    #[command(name = "status")]
    Status {
        #[arg(long)]
        project_path: Option<PathBuf>,
    },
    /// Search indexed code
    #[command(name = "search")]
    Search {
        query: String,
        #[arg(long)]
        project_path: Option<PathBuf>,
        #[arg(short = 'k', default_value = "10")]
        top_k: usize,
        #[arg(long)]
        path_prefix: Option<String>,
        #[arg(long)]
        language: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Context-aware search (returns ContextEnvelope). Use --intent fix|trace|refactor|explain
    #[command(name = "search-in-context", alias = "context")]
    SearchInContext {
        query: String,
        #[arg(long)]
        project_path: Option<PathBuf>,
        #[arg(short = 'k', default_value = "10")]
        top_k: usize,
        #[arg(long)]
        intent: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Impact analysis
    #[command(name = "analyze-impact", alias = "impact")]
    Impact {
        #[arg(long)]
        project_path: Option<PathBuf>,
        #[arg(long)]
        base: Option<String>,
        #[arg(long)]
        files: Vec<String>,
    },
    /// Start MCP stdio server
    #[command(name = "mcp", alias = "serve")]
    Mcp {
        #[arg(long)]
        project_path: Option<PathBuf>,
    },
    /// Show parser capabilities
    #[command(name = "index-capabilities")]
    IndexCapabilities,
    /// Find symbol by name
    #[command(name = "find-symbol")]
    FindSymbol {
        name: String,
        #[arg(long)]
        project_path: Option<PathBuf>,
        #[arg(long)]
        exact: bool,
        #[arg(short = 'k', default_value = "20")]
        top_k: usize,
        #[arg(long)]
        json: bool,
    },
    /// Execute Cypher subset query on the code graph
    #[command(name = "graph-query")]
    GraphQuery {
        query: String,
        #[arg(long)]
        project_path: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// Get callers of a symbol
    #[command(name = "callers")]
    Callers {
        symbol: String,
        #[arg(long)]
        project_path: Option<PathBuf>,
        #[arg(short = 'n', default_value = "20")]
        limit: usize,
        #[arg(long)]
        json: bool,
    },
    /// Get callees of a symbol
    #[command(name = "callees")]
    Callees {
        symbol: String,
        #[arg(long)]
        project_path: Option<PathBuf>,
        #[arg(short = 'n', default_value = "20")]
        limit: usize,
        #[arg(long)]
        json: bool,
    },
    /// Trace call path from one symbol to another
    #[command(name = "trace-path")]
    TracePath {
        from: String,
        to: String,
        #[arg(long)]
        project_path: Option<PathBuf>,
        #[arg(long, default_value = "8")]
        max_depth: usize,
        #[arg(long)]
        json: bool,
    },
    /// Start file watcher and re-index on changes
    #[command(name = "watch")]
    Watch {
        #[arg(long)]
        project_path: Option<PathBuf>,
    },
    /// List indexed files
    #[command(name = "list-files")]
    ListFiles {
        #[arg(long)]
        project_path: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// List symbols in a file
    #[command(name = "file-symbols")]
    FileSymbols {
        file: String,
        #[arg(long)]
        project_path: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// List detected communities
    #[command(name = "list-communities")]
    ListCommunities {
        #[arg(long)]
        project_path: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// List detected frameworks
    #[command(name = "list-frameworks")]
    ListFrameworks {
        #[arg(long)]
        project_path: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// Delete .codecortex directory
    #[command(name = "clean")]
    Clean {
        #[arg(long)]
        project_path: Option<PathBuf>,
    },
    /// Get file summary
    #[command(name = "summarize-file")]
    SummarizeFile {
        file: String,
        #[arg(long)]
        project_path: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// Find references to a symbol
    #[command(name = "symbol-refs")]
    SymbolRefs {
        symbol: String,
        #[arg(long)]
        project_path: Option<PathBuf>,
        #[arg(short = 'n', default_value = "20")]
        limit: usize,
        #[arg(long)]
        json: bool,
    },
    /// Install CodeCortex MCP configuration for detected AI agents
    #[command(name = "install")]
    Install {
        #[arg(long)]
        force: bool,
    },
}

pub fn run(cli: Cli) -> CcResult<()> {
    match cli.command {
        Command::InitProject { project_path } => cmd_init(project_path),
        Command::Index { project_path, full } => cmd_index(project_path, full),
        Command::Status { project_path } => cmd_status(project_path),
        Command::Search {
            query,
            project_path,
            top_k,
            path_prefix,
            language,
            json,
        } => cmd_search(project_path, query, top_k, path_prefix, language, json),
        Command::SearchInContext {
            query,
            project_path,
            top_k,
            intent,
            json,
        } => cmd_prepare_intent_context(
            project_path,
            query,
            top_k,
            intent.map(|s| parse_intent(&s)),
            json,
        ),
        Command::Impact {
            project_path,
            base,
            files,
        } => cmd_impact(project_path, base, files),
        Command::Mcp { project_path } => cmd_serve(project_path),
        Command::IndexCapabilities => cmd_index_capabilities(),
        Command::FindSymbol {
            name,
            project_path,
            exact,
            top_k,
            json,
        } => cmd_find_symbol(project_path, name, exact, top_k, json),
        Command::GraphQuery {
            query,
            project_path,
            json,
        } => cmd_graph_query(project_path, query, json),
        Command::Callers {
            symbol,
            project_path,
            limit,
            json,
        } => cmd_callers(project_path, symbol, limit, json),
        Command::Callees {
            symbol,
            project_path,
            limit,
            json,
        } => cmd_callees(project_path, symbol, limit, json),
        Command::TracePath {
            from,
            to,
            project_path,
            max_depth,
            json,
        } => cmd_trace_path(project_path, from, to, max_depth, json),
        Command::Watch { project_path } => cmd_watch(project_path),
        Command::ListFiles { project_path, json } => cmd_list_files(project_path, json),
        Command::FileSymbols {
            file,
            project_path,
            json,
        } => cmd_file_symbols(project_path, file, json),
        Command::ListCommunities { project_path, json } => cmd_list_communities(project_path, json),
        Command::ListFrameworks { project_path, json } => cmd_list_frameworks(project_path, json),
        Command::Clean { project_path } => cmd_clean(project_path),
        Command::SummarizeFile {
            file,
            project_path,
            json,
        } => cmd_summarize_file(project_path, file, json),
        Command::SymbolRefs {
            symbol,
            project_path,
            limit,
            json,
        } => cmd_symbol_refs(project_path, symbol, limit, json),
        Command::Install { force } => cmd_install(force),
    }
}

fn make_runtime(path: Option<PathBuf>) -> CcResult<CodeIndex> {
    let project = path.unwrap_or_else(|| find_project_root(None));
    CodeIndex::new(Some(&project))
}

fn cmd_init(path: Option<PathBuf>) -> CcResult<()> {
    let project = path.unwrap_or_else(|| find_project_root(None));
    let config_path = project.join(".codecortex.json");
    if config_path.exists() {
        println!("Config already exists: {}", config_path.display());
        return Ok(());
    }
    let config = cc_model::ProjectConfig::default();
    std::fs::write(&config_path, serde_json::to_string_pretty(&config)?)?;
    println!("Created {}", config_path.display());
    Ok(())
}

fn cmd_index(path: Option<PathBuf>, full: bool) -> CcResult<()> {
    let mut rt = make_runtime(path)?;
    println!(
        "Indexing {}...",
        rt.project_path.as_ref().unwrap().display()
    );
    let report = rt.build_index(full)?;
    println!("Done in {}ms", report.elapsed_ms);
    println!("  scanned: {}", report.files_scanned);
    println!("  added:   {}", report.files_added);
    println!("  updated: {}", report.files_updated);
    println!("  removed: {}", report.files_removed);
    println!("  symbols: {}", report.symbols_total);
    println!("  chunks:  {}", report.chunks_total);
    Ok(())
}

fn cmd_status(path: Option<PathBuf>) -> CcResult<()> {
    let rt = make_runtime(path)?;
    let stats = rt.index_status()?;
    println!("Project:     {}", stats.project_path);
    println!("Files:       {}", stats.indexed_files);
    println!("Chunks:      {}", stats.indexed_chunks);
    println!("Symbols:     {}", stats.indexed_symbols);
    println!("Call edges:  {}", stats.indexed_call_edges);
    println!("Test edges:  {}", stats.indexed_test_edges);
    if let Some(ts) = &stats.last_indexed_at {
        println!("Last indexed: {}", ts);
    }
    Ok(())
}

fn cmd_search(
    path: Option<PathBuf>,
    query: String,
    top_k: usize,
    path_prefix: Option<String>,
    language: Option<String>,
    json: bool,
) -> CcResult<()> {
    let project = path.unwrap_or_else(|| find_project_root(None));
    let config = load_project_config(&project);
    let paths = IndexPaths::new(&project);
    if !paths.index_db.exists() {
        return Err(CcError::Config(
            "No index. Run `codecortex index` first.".into(),
        ));
    }
    let languages = language.map(|l| vec![cc_model::Language::from_name(&l)]);
    let db = Arc::new(cc_db::index_db::IndexDb::open(&paths.index_db)?);
    let engine = cc_search::SearchEngine::new(db, &config);
    let hits = engine.search(&cc_model::search::SearchRequest {
        query: query.clone(),
        top_k,
        include_grep: true,
        path_prefix,
        languages,
        ..Default::default()
    })?;
    if json {
        println!("{}", serde_json::to_string_pretty(&hits)?);
    } else if hits.is_empty() {
        println!("No results for: {}", query);
    } else {
        for (i, h) in hits.iter().enumerate() {
            println!(
                "{}. {} [{}:{}-{}] (score={:.3})",
                i + 1,
                h.file_path,
                h.breadcrumb,
                h.start_line,
                h.end_line,
                h.rerank_score
            );
            if let Some(ref n) = h.symbol_name {
                println!("   symbol: {}", n);
            }
            println!();
        }
    }
    Ok(())
}

fn cmd_prepare_intent_context(
    path: Option<PathBuf>,
    query: String,
    top_k: usize,
    intent: Option<Intent>,
    json: bool,
) -> CcResult<()> {
    let mut rt = make_runtime(path)?;
    let envelope = rt.search_in_context(&query, top_k, intent)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&envelope)?);
    } else {
        println!("Intent: {}", envelope.intent);
        println!("Nodes:  {}", envelope.nodes.len());
        println!("Tokens: ~{}", envelope.token_estimate);
        println!(
            "\n{}",
            envelope
                .rendered_prompt
                .chars()
                .take(2000)
                .collect::<String>()
        );
    }
    Ok(())
}

fn cmd_impact(path: Option<PathBuf>, base: Option<String>, files: Vec<String>) -> CcResult<()> {
    let rt = make_runtime(path)?;
    let report = if files.is_empty() {
        rt.analyze_impact(base.as_deref())?
    } else {
        rt.detect_impact(&files)?
    };
    println!("Changed files: {}", report.changed_files.len());
    println!("Impacted symbols: {}", report.impacted_symbols.len());
    println!("Suggested tests: {}", report.suggested_tests.len());
    println!(
        "Risk: critical={} high={} medium={} low={}",
        report.risk_summary.critical,
        report.risk_summary.high,
        report.risk_summary.medium,
        report.risk_summary.low
    );
    Ok(())
}

fn cmd_serve(project_path: Option<PathBuf>) -> CcResult<()> {
    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| CcError::Other(format!("tokio runtime: {}", e)))?;
    rt.block_on(crate::mcp::run_mcp_server(project_path))
}

fn cmd_watch(path: Option<PathBuf>) -> CcResult<()> {
    let project = path.unwrap_or_else(|| find_project_root(None));
    println!("Watching {}...", project.display());
    println!("Press Ctrl+C to stop.\n");

    let mut rt = make_runtime(Some(project.clone()))?;
    let watcher = crate::watcher::FileWatcher::start(&project)?;

    loop {
        std::thread::sleep(std::time::Duration::from_secs(2));
        let drain = watcher.drain_pending();
        if drain.is_empty() {
            continue;
        }
        if !drain.changed.is_empty() {
            println!("Changed: {} file(s)", drain.changed.len());
            for f in &drain.changed {
                println!("  {}", f);
            }
        }
        if !drain.removed.is_empty() {
            println!("Removed: {} file(s)", drain.removed.len());
            for f in &drain.removed {
                println!("  {}", f);
            }
        }
        match rt.build_index(false) {
            Ok(report) => {
                if report.files_added + report.files_updated + report.files_removed > 0 {
                    println!(
                        "  Re-indexed: +{} ~{} -{} ({}ms)\n",
                        report.files_added,
                        report.files_updated,
                        report.files_removed,
                        report.elapsed_ms
                    );
                }
            }
            Err(e) => eprintln!("  Index error: {}\n", e),
        }
    }
}

fn cmd_index_capabilities() -> CcResult<()> {
    let languages = [
        ("Python", "py", "TreeSitter"),
        ("JavaScript", "js, mjs, cjs", "TreeSitter"),
        ("TypeScript", "ts, mts, cts", "TreeSitter"),
        ("TSX", "tsx", "TreeSitter"),
        ("JSX", "jsx", "TreeSitter"),
        ("Java", "java", "TreeSitter"),
        ("Go", "go", "TreeSitter"),
        ("Rust", "rs", "TreeSitter"),
        ("Markdown", "md, mdx", "Generic"),
    ];
    println!("Supported languages:");
    for (name, exts, tier) in &languages {
        println!("  {:<14} ext=[{}]  tier={}", name, exts, tier);
    }
    Ok(())
}

fn cmd_find_symbol(
    path: Option<PathBuf>,
    name: String,
    exact: bool,
    top_k: usize,
    json: bool,
) -> CcResult<()> {
    let rt = make_runtime(path)?;
    let symbols = rt.find_symbol(&name, exact, top_k)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&symbols)?);
    } else if symbols.is_empty() {
        println!("No symbols found for: {}", name);
    } else {
        for s in &symbols {
            println!(
                "{} [{}] {}:{}-{}",
                s.name, s.kind, s.file_path, s.start_line, s.end_line
            );
            if let Some(ref c) = s.container {
                println!("  container: {}", c);
            }
            if let Some(ref sig) = s.signature {
                println!("  signature: {}", sig);
            }
        }
    }
    Ok(())
}

fn cmd_graph_query(path: Option<PathBuf>, query: String, json: bool) -> CcResult<()> {
    let rt = make_runtime(path)?;
    let results = rt.graph_query(&query)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&results)?);
    } else if results.is_empty() {
        println!("No results.");
    } else {
        for (i, row) in results.iter().enumerate() {
            println!("{}. {}", i + 1, row);
        }
    }
    Ok(())
}

fn cmd_callers(path: Option<PathBuf>, symbol: String, limit: usize, json: bool) -> CcResult<()> {
    let rt = make_runtime(path)?;
    let edges = rt.callers(&symbol, limit)?;
    print_edges(&symbol, &edges, json, true)
}

fn cmd_callees(path: Option<PathBuf>, symbol: String, limit: usize, json: bool) -> CcResult<()> {
    let rt = make_runtime(path)?;
    let edges = rt.callees(&symbol, limit)?;
    print_edges(&symbol, &edges, json, false)
}

fn print_edges(
    symbol: &str,
    edges: &[cc_db::index_db::CallEdgeLite],
    json: bool,
    callers: bool,
) -> CcResult<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(edges)?);
    } else if edges.is_empty() {
        println!(
            "No {} found for: {}",
            if callers { "callers" } else { "callees" },
            symbol
        );
    } else {
        for e in edges {
            println!(
                "{}:{} {} -> {} [{}] (confidence={:.2})",
                e.file_path,
                e.line,
                e.caller_symbol.as_deref().unwrap_or("?"),
                e.callee_symbol,
                e.resolution_kind,
                e.confidence
            );
        }
    }
    Ok(())
}

fn cmd_trace_path(
    path: Option<PathBuf>,
    from: String,
    to: String,
    max_depth: usize,
    json: bool,
) -> CcResult<()> {
    let rt = make_runtime(path)?;
    let paths = rt.trace_path(&from, &to, max_depth)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&paths)?);
    } else if paths.is_empty() {
        println!("No path found from {} to {}", from, to);
    } else {
        for (i, p) in paths.iter().enumerate() {
            println!("Path {}: {}", i + 1, p.join(" -> "));
        }
    }
    Ok(())
}

fn cmd_list_files(path: Option<PathBuf>, json: bool) -> CcResult<()> {
    let rt = make_runtime(path)?;
    let files = rt.list_indexed_files()?;
    if json {
        println!("{}", serde_json::to_string_pretty(&files)?);
    } else if files.is_empty() {
        println!("No indexed files.");
    } else {
        for f in &files {
            println!(
                "{} [{}] size={} tier={} indexed_at={}",
                f.file_path, f.language, f.size, f.parser_tier, f.indexed_at
            );
        }
    }
    Ok(())
}

fn cmd_file_symbols(path: Option<PathBuf>, file: String, json: bool) -> CcResult<()> {
    let rt = make_runtime(path)?;
    let symbols = rt.file_symbols(&file)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&symbols)?);
    } else if symbols.is_empty() {
        println!("No symbols in: {}", file);
    } else {
        for s in &symbols {
            println!("{} [{}] {}-{}", s.name, s.kind, s.start_line, s.end_line);
            if let Some(ref c) = s.container {
                println!("  container: {}", c);
            }
            if let Some(ref sig) = s.signature {
                println!("  signature: {}", sig);
            }
        }
    }
    Ok(())
}

fn cmd_list_communities(path: Option<PathBuf>, json: bool) -> CcResult<()> {
    let rt = make_runtime(path)?;
    let communities = rt.list_communities()?;
    if json {
        println!("{}", serde_json::to_string_pretty(&communities)?);
    } else if communities.is_empty() {
        println!("No communities detected.");
    } else {
        for c in &communities {
            println!(
                "Community {} [{}] members={}",
                c.community_id, c.label, c.member_count
            );
        }
    }
    Ok(())
}

fn cmd_list_frameworks(path: Option<PathBuf>, json: bool) -> CcResult<()> {
    let rt = make_runtime(path)?;
    let frameworks = rt.list_frameworks()?;
    if json {
        println!("{}", serde_json::to_string_pretty(&frameworks)?);
    } else if frameworks.is_empty() {
        println!("No frameworks detected.");
    } else {
        for (name, confidence) in &frameworks {
            println!("{} (confidence={:.2})", name, confidence);
        }
    }
    Ok(())
}

fn cmd_clean(path: Option<PathBuf>) -> CcResult<()> {
    let project = path.unwrap_or_else(|| find_project_root(None));
    let cc_dir = project.join(".codecortex");
    if !cc_dir.exists() {
        println!("No .codecortex directory found at {}", project.display());
        return Ok(());
    }
    eprint!("Delete {} ? [y/N] ", cc_dir.display());
    let mut answer = String::new();
    std::io::stdin()
        .read_line(&mut answer)
        .map_err(|e| CcError::Other(format!("read stdin: {}", e)))?;
    if answer.trim().eq_ignore_ascii_case("y") {
        std::fs::remove_dir_all(&cc_dir)?;
        println!("Deleted {}", cc_dir.display());
    } else {
        println!("Aborted.");
    }
    Ok(())
}

fn cmd_summarize_file(path: Option<PathBuf>, file: String, json: bool) -> CcResult<()> {
    let rt = make_runtime(path)?;
    let summary = rt.summarize_file(&file)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&summary)?);
    } else {
        println!(
            "File:     {}",
            summary["file_path"].as_str().unwrap_or(&file)
        );
        println!(
            "Language: {}",
            summary["language"].as_str().unwrap_or("unknown")
        );
        println!("Symbols:  {}", summary["symbols_count"]);
        println!("Chunks:   {}", summary["chunks_count"]);
        if let Some(s) = summary["summary"].as_str() {
            println!("\n{}", s);
        }
    }
    Ok(())
}

fn cmd_symbol_refs(
    path: Option<PathBuf>,
    symbol: String,
    limit: usize,
    json: bool,
) -> CcResult<()> {
    let rt = make_runtime(path)?;
    let refs = rt.symbol_refs(&symbol, limit)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&refs)?);
    } else if refs.is_empty() {
        println!("No references found for: {}", symbol);
    } else {
        println!("References to '{}' ({}):", symbol, refs.len());
        for r in &refs {
            println!(
                "  {}:{} {} [{}] (confidence={:.2})",
                r.file_path, r.line, r.symbol_name, r.resolution_kind, r.confidence
            );
        }
    }
    Ok(())
}

fn cmd_install(force: bool) -> CcResult<()> {
    let binary_path = std::env::current_exe().unwrap_or_default();
    let report = crate::installer::install_all(&binary_path, force);
    for agent in &report.agents_configured {
        println!("  Configured {}", agent);
    }
    for hook in &report.hooks_installed {
        println!("  Installed hook: {}", hook);
    }
    for err in &report.errors {
        eprintln!("  Error: {}", err);
    }
    println!("\n{} agent(s) configured.", report.agents_configured.len());
    Ok(())
}

fn parse_intent(s: &str) -> Intent {
    match s.to_lowercase().as_str() {
        "fix" => Intent::Fix,
        "refactor" => Intent::Refactor,
        "trace" => Intent::Trace,
        "locate" => Intent::Locate,
        "test" => Intent::Test,
        "patch" => Intent::Patch,
        "explain" => Intent::Explain,
        _ => Intent::Default,
    }
}
