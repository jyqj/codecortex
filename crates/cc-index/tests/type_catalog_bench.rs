//! Benchmark evidence for architecture-review candidate C7:
//! `TypeCatalog::build_from_symbols` is rebuilt unconditionally from *all*
//! symbols on every build (including incremental builds) in
//! `indexer_phases.rs::phase_resolve`. Is it worth adding signature gating /
//! incremental merge? Threshold: only if it costs >10% of an incremental build.
//!
//! Run with:
//!   cargo test -p cc-index --release --test type_catalog_bench -- --ignored --nocapture
//!
//! NOT part of the regular suite (#[ignore]); do not commit conclusions without
//! the printed table.
//!
//! NOTE: `type_catalog` is `pub(crate)`, so this integration test compiles the
//! source file directly via `#[path]` instead of changing production
//! visibility. The module only depends on `cc_model`, which is a regular
//! dependency of `cc-index` and therefore visible to test targets.

#[path = "../src/type_catalog.rs"]
mod type_catalog;

use std::hint::black_box;
use std::time::{Duration, Instant};

use cc_model::symbol::{SymbolKind, SymbolRecord};
use cc_model::type_assign::{TypeAssignRecord, TypeAssignSource};
use cc_model::ParserTier;
use type_catalog::TypeCatalog;

const SIZES: &[usize] = &[10_000, 50_000, 100_000];
const WARMUP: usize = 1;
const ITERS: usize = 5;

// ---------------------------------------------------------------------------
// Deterministic synthetic symbol generation (no RNG: pure modular arithmetic)
// ---------------------------------------------------------------------------

fn base_symbol(idx: usize, name: String, kind: SymbolKind, qname: String) -> SymbolRecord {
    SymbolRecord {
        symbol_id: format!("sym-{idx:06}"),
        file_path: format!("src/pkg{}/file{}.rs", idx % 7, idx / 40),
        name,
        kind,
        container: None,
        start_line: 1,
        end_line: 10,
        start_col: 0,
        end_col: 0,
        signature: None,
        doc: None,
        parser_tier: ParserTier::Heuristic,
        parser_confidence: 0.6,
        qname: Some(qname),
        parent_symbol_id: None,
        scope_id: None,
        export_name: None,
        is_default_export: false,
        symbol_uid: Some(format!("uid-{idx:06}")),
        framework_role: None,
        receiver_type: None,
        param_types: None,
        return_type: None,
        param_count: None,
        base_types: None,
        implements: None,
    }
}

/// Generate `n` symbols with a realistic kind mix:
/// - 15% types (11% Class, 2% Interface, 2% Enum), nested qnames,
///   ~10% short-name collisions, half with base_types, a quarter with implements
/// - 50% methods/functions (30% Method with receiver_type, 20% Function),
///   ~3 same-named methods on average (overloads / common names like `get`)
/// - 2% TypeAlias chained onto real types
/// - 33% misc (Variable/Constant/Property) that hit the `_ => {}` arm
fn generate_symbols(n: usize) -> Vec<SymbolRecord> {
    let mut symbols = Vec::with_capacity(n);
    let num_types = (n * 15 / 100).max(1);
    let num_methods = (n * 30 / 100).max(1);
    let mut type_ordinal = 0usize;
    let mut method_ordinal = 0usize;
    let mut func_ordinal = 0usize;
    let mut alias_ordinal = 0usize;

    for idx in 0..n {
        let bucket = idx % 100;
        let sym = if bucket < 15 {
            // Types: Class / Interface / Enum
            let kind = match bucket {
                0..=10 => SymbolKind::Class,
                11 | 12 => SymbolKind::Interface,
                _ => SymbolKind::Enum,
            };
            let ord = type_ordinal;
            type_ordinal += 1;
            // ~10% of types reuse an earlier short name (qname stays unique)
            let short_ord = if ord % 10 == 9 { ord - 9 } else { ord };
            let name = format!("Type{short_ord}");
            let qname = format!("pkg{}.mod{}.{}", ord % 7, ord % 13, name);
            let mut sym = base_symbol(idx, name, kind, qname);
            if ord > 0 && ord.is_multiple_of(2) {
                sym.base_types = Some(format!("Type{}", (ord * 7) % ord));
            }
            if ord % 4 == 1 {
                sym.implements = Some(format!("Iface{}, Iface{}", ord % 31, ord % 17));
            }
            sym
        } else if bucket < 45 {
            // Methods with receiver types; ~3 entries per name on average
            let ord = method_ordinal;
            method_ordinal += 1;
            let name = format!("method{}", ord % (num_methods / 3).max(1));
            let receiver = format!("Type{}", ord % num_types);
            let qname = format!("{receiver}.{name}");
            let mut sym = base_symbol(idx, name, SymbolKind::Method, qname);
            sym.receiver_type = Some(receiver);
            sym.param_count = Some((ord % 5) as u32);
            sym
        } else if bucket < 65 {
            // Free functions, mostly unique names with ~10% duplicates
            let ord = func_ordinal;
            func_ordinal += 1;
            let nf = (n * 20 / 100).max(1);
            let name = format!("func{}", ord % (nf * 9 / 10).max(1));
            let qname = format!("pkg{}.{}", ord % 7, name);
            let mut sym = base_symbol(idx, name, SymbolKind::Function, qname);
            sym.param_count = Some((ord % 4) as u32);
            sym
        } else if bucket < 67 {
            // Type aliases chained onto real type names
            let ord = alias_ordinal;
            alias_ordinal += 1;
            let name = format!("Alias{ord}");
            let mut sym = base_symbol(idx, name.clone(), SymbolKind::TypeAlias, name);
            sym.base_types = Some(format!("Type{}", ord % num_types));
            sym
        } else {
            // Misc symbols: hit the `_ => {}` arm but are still iterated
            let kind = match bucket % 3 {
                0 => SymbolKind::Variable,
                1 => SymbolKind::Constant,
                _ => SymbolKind::Property,
            };
            let name = format!("var{idx}");
            let qname = format!("pkg{}.{}", idx % 7, name);
            base_symbol(idx, name, kind, qname)
        };
        symbols.push(sym);
    }
    symbols
}

/// Deterministic type assignment records (roughly 1 per 10 symbols, matching
/// local-variable density in real outcomes).
fn generate_type_assigns(n_symbols: usize) -> Vec<TypeAssignRecord> {
    let n = n_symbols / 10;
    let num_types = (n_symbols * 15 / 100).max(1);
    (0..n)
        .map(|idx| TypeAssignRecord {
            file_path: format!("src/pkg{}/file{}.rs", idx % 7, idx / 4),
            enclosing_symbol_uid: Some(format!("uid-{idx:06}")),
            var_name: format!("localVar{}", idx % 50),
            type_name: format!("Type{}", idx % num_types),
            line: (idx % 500) as u32 + 1,
            confidence: 0.8,
            source: TypeAssignSource::Constructor,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Measurement helpers
// ---------------------------------------------------------------------------

fn p50<F>(mut run: F) -> Duration
where
    F: FnMut(),
{
    for _ in 0..WARMUP {
        run();
    }
    let mut samples = Vec::with_capacity(ITERS);
    for _ in 0..ITERS {
        let start = Instant::now();
        run();
        samples.push(start.elapsed());
    }
    samples.sort();
    samples[(ITERS - 1) / 2]
}

fn fmt(d: Duration) -> String {
    let micros = d.as_micros();
    if micros >= 10_000 {
        format!("{:.2} ms", d.as_secs_f64() * 1e3)
    } else {
        format!("{micros} us")
    }
}

// ---------------------------------------------------------------------------
// The benchmark
// ---------------------------------------------------------------------------

#[test]
#[ignore = "benchmark only: cargo test -p cc-index --release --test type_catalog_bench -- --ignored --nocapture"]
fn bench_type_catalog_build_from_symbols() {
    eprintln!();
    eprintln!("=== type_catalog_bench: TypeCatalog::build_from_symbols full rebuild cost ===");
    eprintln!(
        "kind mix: 15% types / 30% methods / 20% functions / 2% aliases / 33% misc | \
         warmup={WARMUP} iters={ITERS} p50 | release required"
    );
    eprintln!();
    eprintln!(
        "| {:<10} | {:>22} | {:>22} | {:>18} |",
        "symbols", "build_from_symbols p50", "all_symbols clone p50", "add_type_assigns"
    );
    eprintln!("|{:-<12}|{:-<24}|{:-<24}|{:-<20}|", "", "", "", "");

    for &n in SIZES {
        let symbols = generate_symbols(n);
        let assigns = generate_type_assigns(n);

        // The cost under question: unconditional full rebuild in phase_resolve.
        let build = p50(|| {
            let catalog = TypeCatalog::build_from_symbols(black_box(&symbols));
            black_box(catalog.has_methods());
        });

        // In-phase reference: phase_resolve also materializes `all_symbols` by
        // cloning every SymbolRecord (persisted + write units) right before the
        // catalog build, so this is a fair neighbor-step yardstick.
        let clone_collect = p50(|| {
            let cloned: Vec<SymbolRecord> = black_box(&symbols).to_vec();
            black_box(cloned.len());
        });

        // Secondary fill: add_type_assigns (public, fed from ParseOutcomes).
        let assigns_cost = p50(|| {
            let mut catalog = TypeCatalog::build_from_symbols(black_box(&[]));
            catalog.add_type_assigns(black_box(&assigns));
            black_box(catalog.resolve_var_type("src/pkg0/file0.rs", "localVar0"));
        });

        eprintln!(
            "| {:<10} | {:>22} | {:>22} | {:>18} |",
            n,
            fmt(build),
            fmt(clone_collect),
            format!("{} ({} rec)", fmt(assigns_cost), assigns.len()),
        );
    }

    eprintln!();
    eprintln!("legend: build_from_symbols = the unconditional rebuild in phase_resolve (4b)");
    eprintln!("        all_symbols clone  = the Vec<SymbolRecord> clone-collect feeding it (4b input)");
    eprintln!("        add_type_assigns   = phase 4b-1 secondary fill at ~1 assign per 10 symbols");
}
