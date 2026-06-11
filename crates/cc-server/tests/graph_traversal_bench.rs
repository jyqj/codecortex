//! Benchmark evidence for architecture-review candidate 6:
//! should the Cypher executor's variable-length traversal (SQL `WITH RECURSIVE`
//! CTE over `call_edges`) get an in-memory adjacency fast path, or stay SQL-only?
//!
//! Run with:
//!   cargo test -p cc-server --test graph_traversal_bench --release -- --ignored --nocapture
//!
//! NOT part of the regular suite (#[ignore]); do not commit conclusions without
//! the printed table.

use cc_db::index_db::{EdgeLiteBfs, IndexDb};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

const NUM_SYMBOLS: usize = 50_000;
const NUM_EDGES: usize = 200_000;
const NUM_CHAIN_NODES: usize = 10_000; // 100 chains x 100 nodes
const CHAIN_LEN: usize = 100;
const NUM_HUBS: usize = 200; // fan-out 50..200 each
const WARMUP: usize = 3;
const ITERS: usize = 20;

/// Deterministic xorshift64* RNG (no external dep).
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn uid(i: usize) -> String {
    format!("uid_{i:05}")
}

/// Build the synthetic index DB: 50k symbols, ~200k call edges with a mix of
/// chains, fan-out hubs (degree 50-200), and uniform random edges.
fn build_synthetic_db(dir: &std::path::Path) -> IndexDb {
    let db = IndexDb::open(&dir.join("bench.db")).unwrap().0;
    let conn = db.reads().read_conn().unwrap();

    conn.execute(
        "INSERT INTO files(file_path, language, content_hash, mtime, size, summary, content_excerpt, parser_tier, parser_confidence, is_test_file, indexed_at)
         VALUES('src/bench.ts','TypeScript','hash',1.0,100,'','','tree_sitter',1.0,0,'2024-01-01T00:00:00Z')",
        [],
    )
    .unwrap();

    let tx = conn.unchecked_transaction().unwrap();
    {
        let mut sym_stmt = tx
            .prepare(
                "INSERT INTO symbols(symbol_id, file_path, name, kind, start_line, end_line, symbol_uid)
                 VALUES(?1, 'src/bench.ts', ?2, 'function', 1, 2, ?3)",
            )
            .unwrap();
        for i in 0..NUM_SYMBOLS {
            let name = match i {
                0 => "chain_root".to_string(),
                _ if i == NUM_CHAIN_NODES => "hub_root".to_string(),
                _ => format!("fn_{i}"),
            };
            sym_stmt
                .execute(rusqlite::params![format!("sym_{i:05}"), name, uid(i)])
                .unwrap();
        }

        let mut edge_stmt = tx
            .prepare(
                "INSERT INTO call_edges(edge_id, file_path, callee_symbol, line, caller_symbol_uid, callee_symbol_uid)
                 VALUES(?1, 'src/bench.ts', 'callee', 1, ?2, ?3)",
            )
            .unwrap();
        let mut edge_id = 0usize;
        fn insert_edge(
            stmt: &mut rusqlite::Statement<'_>,
            edge_id: &mut usize,
            from: usize,
            to: usize,
        ) {
            stmt.execute(rusqlite::params![format!("e{edge_id}"), uid(from), uid(to)])
                .unwrap();
            *edge_id += 1;
        }

        // 1. Chains: nodes 0..10_000, 100 chains of length 100 (~9_900 edges).
        for start in (0..NUM_CHAIN_NODES).step_by(CHAIN_LEN) {
            for offset in 0..CHAIN_LEN - 1 {
                insert_edge(
                    &mut edge_stmt,
                    &mut edge_id,
                    start + offset,
                    start + offset + 1,
                );
            }
        }

        // 2. Hubs: nodes 10_000..10_200, fan-out 50..200 to random targets.
        //    hub_root (node 10_000) gets exactly 200 targets.
        let mut rng = Rng(0xC0DE_C0DE_DEAD_BEEF);
        for hub_offset in 0..NUM_HUBS {
            let hub = NUM_CHAIN_NODES + hub_offset;
            let fanout = if hub_offset == 0 {
                200
            } else {
                50 + rng.below(151)
            };
            for _ in 0..fanout {
                let mut target = rng.below(NUM_SYMBOLS);
                if target == hub {
                    target = (target + 1) % NUM_SYMBOLS;
                }
                insert_edge(&mut edge_stmt, &mut edge_id, hub, target);
            }
        }

        // 3. Uniform random edges to fill up to NUM_EDGES total.
        while edge_id < NUM_EDGES {
            let from = rng.below(NUM_SYMBOLS);
            let mut to = rng.below(NUM_SYMBOLS);
            if to == from {
                to = (to + 1) % NUM_SYMBOLS;
            }
            insert_edge(&mut edge_stmt, &mut edge_id, from, to);
        }
    }
    tx.commit().unwrap();
    drop(conn);
    db
}

// ---------------------------------------------------------------------------
// In-memory traversal (mirrors GraphReadModel::call_adjacency + graph_walk BFS)
// ---------------------------------------------------------------------------

type Adjacency = HashMap<String, Vec<EdgeLiteBfs>>;

/// Mirrors `GraphReadModel::call_adjacency`: bulk-load edges, group by caller.
fn build_adjacency(db: &IndexDb) -> Adjacency {
    let edges = db.reads().call_uid_edges_lite().unwrap();
    let mut adj: Adjacency = HashMap::new();
    for edge in edges {
        adj.entry(edge.caller_uid.clone()).or_default().push(edge);
    }
    adj
}

/// Level-by-level BFS collecting the set of nodes reachable in 1..=max_depth
/// hops, early-stopping at `limit` results (matching the Cypher LIMIT).
fn bfs_reach<F>(seed: &str, max_depth: usize, limit: Option<usize>, mut neighbors: F) -> Vec<String>
where
    F: FnMut(&str) -> Vec<String>,
{
    let mut visited: HashSet<String> = HashSet::new();
    visited.insert(seed.to_string());
    let mut frontier = vec![seed.to_string()];
    let mut results = Vec::new();
    for _depth in 1..=max_depth {
        let mut next = Vec::new();
        for node in &frontier {
            for callee in neighbors(node) {
                if visited.insert(callee.clone()) {
                    results.push(callee.clone());
                    if let Some(cap) = limit {
                        if results.len() >= cap {
                            return results;
                        }
                    }
                    next.push(callee);
                }
            }
        }
        if next.is_empty() {
            break;
        }
        frontier = next;
    }
    results
}

/// Full equivalent of the Cypher query through the in-memory path: resolve the
/// seed name to a UID (SQL), BFS the adjacency, then resolve result names (SQL),
/// because real handlers must return names, not UIDs.
fn mem_query(
    db: &IndexDb,
    adj: &Adjacency,
    seed_name: &str,
    depth: usize,
    limit: Option<usize>,
) -> usize {
    let seed_uid = resolve_seed(db, seed_name);
    let reach = bfs_reach(&seed_uid, depth, limit, |node| {
        adj.get(node)
            .map(|edges| edges.iter().map(|e| e.callee_uid.clone()).collect())
            .unwrap_or_default()
    });
    resolve_names(db, &reach)
}

/// Lazy per-node variant mirroring `GraphReadModel::neighbors()` cache misses:
/// each newly visited node costs one SQL query; results memoized in a local map.
fn mem_query_lazy(db: &IndexDb, seed_name: &str, depth: usize, limit: Option<usize>) -> usize {
    let seed_uid = resolve_seed(db, seed_name);
    let mut memo: HashMap<String, Vec<String>> = HashMap::new();
    let reach = bfs_reach(&seed_uid, depth, limit, |node| {
        if let Some(cached) = memo.get(node) {
            return cached.clone();
        }
        let callees: Vec<String> = db
            .reads()
            .call_edges_from_uid_lite(node)
            .unwrap_or_default()
            .into_iter()
            .map(|e| e.callee_uid)
            .collect();
        memo.insert(node.to_string(), callees.clone());
        callees
    });
    resolve_names(db, &reach)
}

fn resolve_seed(db: &IndexDb, seed_name: &str) -> String {
    let rows = db
        .reads()
        .query_json(
            "SELECT symbol_uid FROM symbols WHERE name = ?1 LIMIT 1",
            &[seed_name.to_string()],
        )
        .unwrap();
    rows[0]
        .get("symbol_uid")
        .and_then(|v| v.as_str())
        .unwrap()
        .to_string()
}

fn resolve_names(db: &IndexDb, uids: &[String]) -> usize {
    let mut names = 0usize;
    for batch in uids.chunks(900) {
        if batch.is_empty() {
            continue;
        }
        let placeholders = (1..=batch.len())
            .map(|i| format!("?{i}"))
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!("SELECT name FROM symbols WHERE symbol_uid IN ({placeholders})");
        names += db.reads().query_json(&sql, batch).unwrap().len();
    }
    names
}

// ---------------------------------------------------------------------------
// Measurement helpers
// ---------------------------------------------------------------------------

struct Stats {
    p50: Duration,
    p95: Duration,
    rows: usize,
}

fn measure<F>(mut run: F) -> Stats
where
    F: FnMut() -> usize,
{
    for _ in 0..WARMUP {
        run();
    }
    let mut samples = Vec::with_capacity(ITERS);
    let mut rows = 0;
    for _ in 0..ITERS {
        let start = Instant::now();
        rows = run();
        samples.push(start.elapsed());
    }
    samples.sort();
    Stats {
        p50: samples[(ITERS - 1) / 2],
        p95: samples[((ITERS - 1) * 95) / 100],
        rows,
    }
}

/// Cold variant: no warmup reuse — every iteration pays the full cost.
fn measure_cold<F>(mut run: F) -> Stats
where
    F: FnMut() -> usize,
{
    let mut samples = Vec::with_capacity(ITERS);
    let mut rows = 0;
    for _ in 0..ITERS {
        let start = Instant::now();
        rows = run();
        samples.push(start.elapsed());
    }
    samples.sort();
    Stats {
        p50: samples[(ITERS - 1) / 2],
        p95: samples[((ITERS - 1) * 95) / 100],
        rows,
    }
}

fn fmt(d: Duration) -> String {
    let micros = d.as_micros();
    if micros >= 10_000 {
        format!("{:.2} ms", d.as_secs_f64() * 1e3)
    } else {
        format!("{micros} us")
    }
}

fn row(label: &str, stats: &Stats) {
    eprintln!(
        "| {:<44} | {:>10} | {:>10} | {:>6} |",
        label,
        fmt(stats.p50),
        fmt(stats.p95),
        stats.rows
    );
}

// ---------------------------------------------------------------------------
// The benchmark
// ---------------------------------------------------------------------------

#[test]
#[ignore = "benchmark only: cargo test -p cc-server --test graph_traversal_bench --release -- --ignored --nocapture"]
fn bench_cypher_cte_vs_inmemory_adjacency() {
    let tmp = tempfile::TempDir::new().unwrap();
    eprintln!(
        "building synthetic DB: {NUM_SYMBOLS} symbols, {NUM_EDGES} call_edges (chains + hubs + random)..."
    );
    let build_start = Instant::now();
    let db = build_synthetic_db(tmp.path());
    eprintln!("DB build took {}", fmt(build_start.elapsed()));

    let cypher = |seed: &str, depth: usize, limit: usize| {
        format!(
            "MATCH (a:Function)-[:CALLS*1..{depth}]->(b:Function) \
             WHERE a.name = '{seed}' RETURN b.name LIMIT {limit}"
        )
    };

    // Scenario matrix: (label, seed, depth, limit). limit=100_000 forces full
    // expansion (no early termination), modeling "count everything reachable".
    let scenarios: Vec<(&str, &str, usize, usize)> = vec![
        ("hub   *1..3 LIMIT 50", "hub_root", 3, 50),
        ("hub   *1..4 LIMIT 50", "hub_root", 4, 50),
        ("chain *1..3 LIMIT 50", "chain_root", 3, 50),
        ("chain *1..4 LIMIT 50", "chain_root", 4, 50),
        ("hub   *1..3 full (LIMIT 100000)", "hub_root", 3, 100_000),
        ("hub   *1..4 full (LIMIT 100000)", "hub_root", 4, 100_000),
    ];

    // One-off adjacency build cost at this scale (the cost the cache amortizes).
    let adj_build = measure_cold(|| build_adjacency(&db).len());
    let adj = build_adjacency(&db);

    eprintln!();
    eprintln!("=== graph_traversal_bench: Cypher recursive CTE vs in-memory adjacency BFS ===");
    eprintln!(
        "{} symbols, {} call_edges | warmup={} iters={} | release required",
        NUM_SYMBOLS, NUM_EDGES, WARMUP, ITERS
    );
    eprintln!();
    eprintln!(
        "| {:<44} | {:>10} | {:>10} | {:>6} |",
        "case", "p50", "p95", "rows"
    );
    eprintln!("|{:-<46}|{:-<12}|{:-<12}|{:-<8}|", "", "", "", "");
    row("adjacency build alone (call_uid_edges_lite)", &adj_build);

    for (label, seed, depth, limit) in &scenarios {
        let query = cypher(seed, *depth, *limit);
        let cte = measure(|| {
            cc_search::cypher::cypher_query(&query, &db)
                .unwrap()
                .rows
                .len()
        });
        row(&format!("cypher CTE   | {label}"), &cte);

        let warm = measure(|| mem_query(&db, &adj, seed, *depth, Some(*limit)));
        row(&format!("mem warm BFS | {label}"), &warm);

        let lazy = measure_cold(|| mem_query_lazy(&db, seed, *depth, Some(*limit)));
        row(&format!("mem lazy-cold| {label}"), &lazy);
    }

    // Cold bulk path for one representative scenario: build + BFS each iteration.
    let cold = measure_cold(|| {
        let fresh = build_adjacency(&db);
        mem_query(&db, &fresh, "hub_root", 3, Some(50))
    });
    row("mem cold (build+BFS) | hub *1..3 LIMIT 50", &cold);

    eprintln!();
    eprintln!(
        "legend: cypher CTE = cc_search::cypher::cypher_query (WITH RECURSIVE over call_edges)"
    );
    eprintln!("        mem warm   = prebuilt adjacency HashMap BFS + seed/name SQL lookups");
    eprintln!("        mem lazy-cold = per-node call_edges_from_uid_lite (GraphReadModel::neighbors miss path)");
    eprintln!("        mem cold   = full adjacency rebuild + BFS per query");
}
