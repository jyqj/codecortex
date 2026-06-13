//! Deterministic synthetic repository generator for scale benchmarks.
//!
//! Generates a multi-module project in three languages (TypeScript, Python,
//! Rust) with a non-trivial call graph: intra-file fan-out (slot 0 calls
//! slots 1–3), cross-file bridge chains (slot 3 calls the next same-language
//! file's slot 0), per-language hub functions with bounded fan-in, and a few
//! same-file 3-cycles. Every 50 file slots add one YAML config file and one
//! Express-style routes file for shape variety.
//!
//! Everything is derived from `(seed, target_files)` only — repeated
//! generation produces byte-identical trees — and the returned
//! [`GroundTruth`] facts let scale benchmarks double as correctness checks
//! (e.g. "search for the needle symbol must rank it top-5", "impact of the
//! hub file must include known callers").

use std::fmt::Write as _;
use std::path::Path;

// ── Layout constants ───────────────────────────────────────────────

const FILES_PER_MODULE: usize = 8;
/// Every `EXTRA_PERIOD` slots, two slots become non-code variety files.
const EXTRA_PERIOD: usize = 50;
const CONFIG_SLOT: usize = 24;
const ROUTES_SLOT: usize = 49;
/// Per-language file positions hosting a same-file 3-cycle.
const CYCLE_EVERY: usize = 47;
const CYCLE_OFFSET: usize = 5;
/// Hub fan-in is bounded: roughly this many callers regardless of scale.
const HUB_TARGET_CALLERS: usize = 24;
/// Below this the ground-truth positions (chain, cycle, needle) collide.
const MIN_TARGET_FILES: usize = 60;

const FILLER_WORDS: &[&str] = &[
    "payload", "registry", "cursor", "bucket", "tally", "metric", "quota", "signal", "ledger",
    "anchor", "tracer", "harbor", "fathom", "garnet", "isotope", "jasper", "weave", "prism",
    "relay", "drift",
];

/// Rare words reserved for the needle body so hybrid (FTS) retrieval has a
/// unique target; they never appear in [`FILLER_WORDS`] content.
const NEEDLE_WORDS: &[&str] = &[
    "kestrel", "palisade", "quillon", "umbra", "verdant", "gossamer", "zephyr", "obsidian",
    "fennel", "yonder",
];

// ── Spec and outputs ───────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
pub struct SynthSpec {
    /// Total number of files to generate (code + config + routes).
    pub target_files: usize,
    /// PRNG seed; content is fully deterministic from `(seed, target_files)`.
    pub seed: u64,
}

/// One known call edge: `caller` (in `caller_file`) calls `callee` (in
/// `callee_file`). Usable directly in benchmark assertions.
#[derive(Debug, Clone)]
pub struct CallFact {
    pub caller: String,
    pub caller_file: String,
    pub callee: String,
    pub callee_file: String,
}

/// Ground-truth facts emitted alongside the generated tree.
#[derive(Debug, Clone)]
pub struct GroundTruth {
    /// Globally unique TypeScript function; symbol search must rank it top-5.
    pub needle_symbol: String,
    pub needle_file: String,
    /// Distinctive body phrase for hybrid (FTS) retrieval of the needle.
    pub needle_phrase: String,
    /// TypeScript hub with bounded fan-in; impact on `hub_file` must surface
    /// callers from `hub_callers`.
    pub hub_symbol: String,
    pub hub_file: String,
    pub hub_callers: Vec<CallFact>,
    /// Python hub — exercises cross-file resolution on the second language.
    pub py_hub_symbol: String,
    pub py_hub_callers: Vec<CallFact>,
    /// Consecutive TS hops: fn_a_0 → fn_a_3 → fn_b_0 → fn_b_3 → fn_c_0.
    /// `trace(chain[0].caller → chain[3].callee)` must find a path.
    pub chain: Vec<CallFact>,
    /// Same-file TypeScript 3-cycle: `[cyc_a, cyc_b, cyc_c]` with a→b→c→a.
    pub cycle_symbols: Vec<String>,
    pub cycle_file: String,
    /// Same-file Rust fan-out edge (fn_g_0 → fn_g_2) for parse coverage.
    pub rs_intra: CallFact,
}

#[derive(Debug, Clone)]
pub struct SynthRepo {
    pub files_written: usize,
    pub ts_files: usize,
    pub py_files: usize,
    pub rs_files: usize,
    /// Config + routes variety files.
    pub extra_files: usize,
    /// Total named functions emitted (excluding class methods).
    pub functions_planned: usize,
    /// Relative paths of all code files, in slot order — incremental-bench
    /// mutation targets.
    pub code_file_paths: Vec<String>,
    pub ground_truth: GroundTruth,
}

// ── Deterministic PRNG (no external dependency) ────────────────────

/// SplitMix64: tiny, stable, seedable.
struct SplitMix64(u64);

impl SplitMix64 {
    fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut mixed = self.0;
        mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        mixed ^ (mixed >> 31)
    }

    fn pick<'a>(&mut self, words: &'a [&'a str]) -> &'a str {
        words[(self.next_u64() % words.len() as u64) as usize]
    }
}

// ── Language plumbing ──────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lang {
    Ts,
    Py,
    Rs,
}

impl Lang {
    fn of_module(module: usize) -> Self {
        match module % 3 {
            0 => Lang::Ts,
            1 => Lang::Py,
            _ => Lang::Rs,
        }
    }

    fn file_stem(self, slot: usize) -> String {
        let prefix = match self {
            Lang::Ts => "ts",
            Lang::Py => "py",
            Lang::Rs => "rs",
        };
        format!("{}_{:05}", prefix, slot)
    }

    fn ext(self) -> &'static str {
        match self {
            Lang::Ts => "ts",
            Lang::Py => "py",
            Lang::Rs => "rs",
        }
    }

    fn hub_symbol(self) -> &'static str {
        match self {
            Lang::Ts => "hub_ts_dispatch",
            Lang::Py => "hub_py_dispatch",
            Lang::Rs => "hub_rs_dispatch",
        }
    }
}

fn fn_name(slot: usize, index: usize) -> String {
    format!("fn_{:05}_{}", slot, index)
}

fn cyc_name(slot: usize, leg: char) -> String {
    format!("cyc_{:05}_{}", slot, leg)
}

/// All generated files live exactly one module directory deep, so a TS
/// relative import is always `../<module>/<stem>`.
fn ts_import_specifier(rel_path: &str) -> String {
    format!("../{}", rel_path.trim_end_matches(".ts"))
}

/// Python dotted module path from a project-relative file path.
fn py_module_path(rel_path: &str) -> String {
    rel_path.trim_end_matches(".py").replace('/', ".")
}

// ── Generation ─────────────────────────────────────────────────────

#[derive(Debug)]
enum FileKind {
    Code { lang: Lang, lang_pos: usize },
    Config,
    Routes,
}

struct FilePlan {
    slot: usize,
    rel_path: String,
    kind: FileKind,
}

/// Per-language hub fan-in stride: every `step`-th file (skipping the hub
/// file itself) calls the hub from its slot-1 function.
fn hub_step(lang_count: usize) -> usize {
    (lang_count / HUB_TARGET_CALLERS).max(3)
}

/// Generate the synthetic repository under `root`. Returns counters and the
/// ground-truth facts for benchmark assertions.
pub fn generate(root: &Path, spec: &SynthSpec) -> Result<SynthRepo, String> {
    if spec.target_files < MIN_TARGET_FILES {
        return Err(format!(
            "synth target_files must be >= {}, got {}",
            MIN_TARGET_FILES, spec.target_files
        ));
    }

    // Pass 1: plan every file slot. Module = slot / FILES_PER_MODULE;
    // language cycles per module so each language forms long bridge chains.
    let mut plans: Vec<FilePlan> = Vec::with_capacity(spec.target_files);
    // (slot, rel_path) of code files per language, in chain order.
    let mut lang_files: [Vec<(usize, String)>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    for slot in 0..spec.target_files {
        let module = slot / FILES_PER_MODULE;
        let module_dir = format!("m_{:04}", module);
        let (rel_path, kind) = if slot % EXTRA_PERIOD == CONFIG_SLOT {
            (
                format!("{}/config_{:05}.yaml", module_dir, slot),
                FileKind::Config,
            )
        } else if slot % EXTRA_PERIOD == ROUTES_SLOT {
            (
                format!("{}/routes_{:05}.ts", module_dir, slot),
                FileKind::Routes,
            )
        } else {
            let lang = Lang::of_module(module);
            let rel = format!("{}/{}.{}", module_dir, lang.file_stem(slot), lang.ext());
            let lang_pos = lang_files[lang as usize].len();
            lang_files[lang as usize].push((slot, rel.clone()));
            (rel, FileKind::Code { lang, lang_pos })
        };
        plans.push(FilePlan {
            slot,
            rel_path,
            kind,
        });
    }

    let ts = &lang_files[Lang::Ts as usize];
    let py = &lang_files[Lang::Py as usize];
    let rs = &lang_files[Lang::Rs as usize];

    // Ground-truth positions (all derived from the plan, not from content).
    let needle_pos = ts.len() / 2;
    let chain_base = 2; // ts positions 2,3,4 — clear of the hub (0)
    let cycle_pos = CYCLE_OFFSET;
    let mut phrase_rng = SplitMix64::new(spec.seed);
    let mut phrase_words: Vec<&str> = Vec::with_capacity(4);
    while phrase_words.len() < 4 {
        let word = phrase_rng.pick(NEEDLE_WORDS);
        if !phrase_words.contains(&word) {
            phrase_words.push(word);
        }
    }
    let needle_phrase = phrase_words.join(" ");
    // The symbol name shares a phrase token so realistic retrieval signals
    // (symbol trigram preselect + content FTS) both see the needle.
    let needle_symbol = format!("needle_{}_{:08x}", phrase_words[0], spec.seed & 0xFFFF_FFFF);

    let hub_caller_facts = |files: &[(usize, String)], hub: &str| -> Vec<CallFact> {
        let step = hub_step(files.len());
        files
            .iter()
            .enumerate()
            .filter(|(pos, _)| *pos > 0 && pos % step == 0)
            .map(|(_, (slot, rel))| CallFact {
                caller: fn_name(*slot, 1),
                caller_file: rel.clone(),
                callee: hub.to_string(),
                callee_file: files[0].1.clone(),
            })
            .collect()
    };

    let chain_fact = |from: (usize, &str, usize), to: (usize, &str, usize)| CallFact {
        caller: fn_name(from.0, from.2),
        caller_file: from.1.to_string(),
        callee: fn_name(to.0, to.2),
        callee_file: to.1.to_string(),
    };
    let (slot_a, file_a) = (&ts[chain_base].0, ts[chain_base].1.as_str());
    let (slot_b, file_b) = (&ts[chain_base + 1].0, ts[chain_base + 1].1.as_str());
    let (slot_c, file_c) = (&ts[chain_base + 2].0, ts[chain_base + 2].1.as_str());
    let chain = vec![
        chain_fact((*slot_a, file_a, 0), (*slot_a, file_a, 3)),
        chain_fact((*slot_a, file_a, 3), (*slot_b, file_b, 0)),
        chain_fact((*slot_b, file_b, 0), (*slot_b, file_b, 3)),
        chain_fact((*slot_b, file_b, 3), (*slot_c, file_c, 0)),
    ];

    let (cycle_slot, cycle_file) = (&ts[cycle_pos].0, ts[cycle_pos].1.clone());
    let (rs_slot, rs_file) = (&rs[1].0, rs[1].1.as_str());
    let ground_truth = GroundTruth {
        needle_symbol: needle_symbol.clone(),
        needle_file: ts[needle_pos].1.clone(),
        needle_phrase: needle_phrase.clone(),
        hub_symbol: Lang::Ts.hub_symbol().to_string(),
        hub_file: ts[0].1.clone(),
        hub_callers: hub_caller_facts(ts, Lang::Ts.hub_symbol()),
        py_hub_symbol: Lang::Py.hub_symbol().to_string(),
        py_hub_callers: hub_caller_facts(py, Lang::Py.hub_symbol()),
        chain,
        cycle_symbols: vec![
            cyc_name(*cycle_slot, 'a'),
            cyc_name(*cycle_slot, 'b'),
            cyc_name(*cycle_slot, 'c'),
        ],
        cycle_file,
        rs_intra: CallFact {
            caller: fn_name(*rs_slot, 0),
            caller_file: rs_file.to_string(),
            callee: fn_name(*rs_slot, 2),
            callee_file: rs_file.to_string(),
        },
    };

    // Pass 2: emit content. Per-file PRNG is keyed on (seed, slot) only, so
    // content hashing is stable regardless of generation order.
    let mut functions_planned = 0usize;
    let mut code_file_paths: Vec<String> = Vec::with_capacity(spec.target_files);
    let mut extra_files = 0usize;
    let mut last_module_dir = String::new();
    for plan in &plans {
        let module_dir = &plan.rel_path[..plan.rel_path.find('/').unwrap_or(0)];
        if module_dir != last_module_dir {
            std::fs::create_dir_all(root.join(module_dir))
                .map_err(|e| format!("create module dir {}: {}", module_dir, e))?;
            last_module_dir = module_dir.to_string();
        }
        let mut rng = SplitMix64::new(spec.seed ^ (plan.slot as u64).wrapping_mul(0x9E37_79B9));
        let (content, fns) = match &plan.kind {
            FileKind::Config => (config_yaml(plan.slot, &mut rng), 0),
            FileKind::Routes => (routes_ts(plan.slot), 2),
            FileKind::Code { lang, lang_pos } => {
                let needle = (*lang == Lang::Ts && *lang_pos == needle_pos)
                    .then_some((needle_symbol.as_str(), needle_phrase.as_str()));
                code_file(
                    spec,
                    plan.slot,
                    *lang,
                    *lang_pos,
                    &lang_files[*lang as usize],
                    needle,
                    &mut rng,
                )
            }
        };
        std::fs::write(root.join(&plan.rel_path), content)
            .map_err(|e| format!("write {}: {}", plan.rel_path, e))?;
        functions_planned += fns;
        match plan.kind {
            FileKind::Code { .. } => code_file_paths.push(plan.rel_path.clone()),
            _ => extra_files += 1,
        }
    }

    Ok(SynthRepo {
        files_written: plans.len(),
        ts_files: ts.len(),
        py_files: py.len(),
        rs_files: rs.len(),
        extra_files,
        functions_planned,
        code_file_paths,
        ground_truth,
    })
}

// ── Content builders ───────────────────────────────────────────────

/// Shared shape for one code file:
/// - slot 0: entry, fan-out to slots 1–3
/// - slot 1: hub call (on hub-caller files) or local compute
/// - slot 2: local compute with filler words
/// - slot 3: bridge to the next same-language file's slot 0 (chain edge)
///
/// plus optional hub / needle / cycle / class extras.
#[allow(clippy::too_many_arguments)]
fn code_file(
    spec: &SynthSpec,
    slot: usize,
    lang: Lang,
    lang_pos: usize,
    files: &[(usize, String)],
    needle: Option<(&str, &str)>,
    rng: &mut SplitMix64,
) -> (String, usize) {
    let next = files.get(lang_pos + 1);
    let hub_file = &files[0].1;
    let is_hub = lang_pos == 0;
    let is_hub_caller = !is_hub && lang_pos.is_multiple_of(hub_step(files.len()));
    let has_cycle = lang_pos % CYCLE_EVERY == CYCLE_OFFSET;
    let has_class = slot % 4 == 1;
    let label = format!(
        "{} {} {}",
        rng.pick(FILLER_WORDS),
        rng.pick(FILLER_WORDS),
        rng.pick(FILLER_WORDS)
    );
    let mult = 31 + (rng.next_u64() % 97) * 2; // odd multiplier
    let modulo = 1009 + rng.next_u64() % 4001;

    let mut out = String::new();
    let mut fns = 4usize;
    let f = |i: usize| fn_name(slot, i);
    match lang {
        Lang::Ts => {
            let _ = writeln!(
                out,
                "// Auto-generated synthetic module (seed {:#018x}, file {}).",
                spec.seed, slot
            );
            if let Some((next_slot, next_rel)) = next {
                let _ = writeln!(
                    out,
                    "import {{ {} }} from '{}';",
                    fn_name(*next_slot, 0),
                    ts_import_specifier(next_rel)
                );
            }
            if is_hub_caller {
                let _ = writeln!(
                    out,
                    "import {{ {} }} from '{}';",
                    lang.hub_symbol(),
                    ts_import_specifier(hub_file)
                );
            }
            let _ = writeln!(out);
            if is_hub {
                let _ = writeln!(out, "/** Shared dispatch hub with bounded fan-in. */");
                let _ = writeln!(
                    out,
                    "export function {}(input: number): number {{\n  return (input * 2654435761) % 4093;\n}}\n",
                    lang.hub_symbol()
                );
                fns += 1;
            }
            // The needle leads the file so its beacon lands inside the
            // file-level content excerpt (first chunks) used by preselect.
            if let Some((needle_name, phrase)) = needle {
                let _ = writeln!(out, "/** Needle beacon: {}. */", phrase);
                let _ = writeln!(
                    out,
                    "export function {}(input: number): string {{\n  const beacon = '{}';\n  return `${{beacon}}:${{input}}`;\n}}\n",
                    needle_name, phrase
                );
                fns += 1;
            }
            let _ = writeln!(out, "/** Entry point for record batch {}. */", slot);
            let _ = writeln!(
                out,
                "export function {}(seedValue: number): number {{\n  const left = {}(seedValue + 3);\n  const right = {}(left);\n  return {}(right);\n}}\n",
                f(0), f(1), f(2), f(3)
            );
            let body1 = if is_hub_caller {
                format!("return {}(input) + 7;", lang.hub_symbol())
            } else {
                format!("return (input * {}) % {};", mult, modulo)
            };
            let _ = writeln!(
                out,
                "export function {}(input: number): number {{\n  {}\n}}\n",
                f(1),
                body1
            );
            let _ = writeln!(
                out,
                "export function {}(input: number): number {{\n  const label = '{}';\n  return input + label.length;\n}}\n",
                f(2),
                label
            );
            let body3 = match next {
                Some((next_slot, _)) => format!("return {}(input * 2);", fn_name(*next_slot, 0)),
                None => "return input;".to_string(),
            };
            let _ = writeln!(
                out,
                "/** Bridge into the next module in the chain. */\nexport function {}(input: number): number {{\n  {}\n}}\n",
                f(3),
                body3
            );
            if has_cycle {
                for (leg, next_leg, base) in [('a', 'b', 0), ('b', 'c', 1), ('c', 'a', 2)] {
                    let _ = writeln!(
                        out,
                        "export function {}(depth: number): number {{\n  return depth <= 0 ? {} : {}(depth - 1);\n}}\n",
                        cyc_name(slot, leg),
                        base,
                        cyc_name(slot, next_leg)
                    );
                }
                fns += 3;
            }
            if has_class {
                let _ = writeln!(
                    out,
                    "export class Worker{:05} {{\n  process(input: number): number {{\n    return {}(input);\n  }}\n\n  describe(): string {{\n    return 'worker {}';\n  }}\n}}",
                    slot,
                    f(2),
                    slot
                );
            }
        }
        Lang::Py => {
            let _ = writeln!(
                out,
                "\"\"\"Auto-generated synthetic module (seed {:#018x}, file {}).\"\"\"",
                spec.seed, slot
            );
            if let Some((next_slot, next_rel)) = next {
                let _ = writeln!(
                    out,
                    "from {} import {}",
                    py_module_path(next_rel),
                    fn_name(*next_slot, 0)
                );
            }
            if is_hub_caller {
                let _ = writeln!(
                    out,
                    "from {} import {}",
                    py_module_path(hub_file),
                    lang.hub_symbol()
                );
            }
            let _ = writeln!(out);
            if is_hub {
                let _ = writeln!(
                    out,
                    "\ndef {}(value):\n    \"\"\"Shared dispatch hub with bounded fan-in.\"\"\"\n    return (value * 2654435761) % 4093\n",
                    lang.hub_symbol()
                );
                fns += 1;
            }
            let _ = writeln!(
                out,
                "\ndef {}(seed_value):\n    \"\"\"Entry point for record batch {}.\"\"\"\n    left = {}(seed_value + 3)\n    right = {}(left)\n    return {}(right)\n",
                f(0), slot, f(1), f(2), f(3)
            );
            let body1 = if is_hub_caller {
                format!("return {}(value) + 7", lang.hub_symbol())
            } else {
                format!("return (value * {}) % {}", mult, modulo)
            };
            let _ = writeln!(out, "\ndef {}(value):\n    {}\n", f(1), body1);
            let _ = writeln!(
                out,
                "\ndef {}(value):\n    label = \"{}\"\n    return value + len(label)\n",
                f(2),
                label
            );
            let body3 = match next {
                Some((next_slot, _)) => format!("return {}(value * 2)", fn_name(*next_slot, 0)),
                None => "return value".to_string(),
            };
            let _ = writeln!(out, "\ndef {}(value):\n    {}\n", f(3), body3);
            if has_cycle {
                for (leg, next_leg, base) in [('a', 'b', 0), ('b', 'c', 1), ('c', 'a', 2)] {
                    let _ = writeln!(
                        out,
                        "\ndef {}(depth):\n    if depth <= 0:\n        return {}\n    return {}(depth - 1)\n",
                        cyc_name(slot, leg),
                        base,
                        cyc_name(slot, next_leg)
                    );
                }
                fns += 3;
            }
            if has_class {
                let _ = writeln!(
                    out,
                    "\nclass Worker{:05}:\n    \"\"\"Synthetic worker for batch {}.\"\"\"\n\n    def process(self, value):\n        return {}(value)\n\n    def describe(self):\n        return \"worker {}\"",
                    slot,
                    slot,
                    f(2),
                    slot
                );
            }
        }
        Lang::Rs => {
            let _ = writeln!(
                out,
                "//! Auto-generated synthetic module (seed {:#018x}, file {}).",
                spec.seed, slot
            );
            let _ = writeln!(out);
            if is_hub {
                let _ = writeln!(
                    out,
                    "/// Shared dispatch hub with bounded fan-in.\npub fn {}(value: i64) -> i64 {{\n    value.wrapping_mul(2654435761) % 4093\n}}\n",
                    lang.hub_symbol()
                );
                fns += 1;
            }
            // Cross-file Rust calls rely on globally unique names (resolver's
            // unique-global fallback) — no `use` statements needed.
            let _ = writeln!(
                out,
                "/// Entry point for record batch {}.\npub fn {}(seed_value: i64) -> i64 {{\n    let left = {}(seed_value + 3);\n    let right = {}(left);\n    {}(right)\n}}\n",
                slot, f(0), f(1), f(2), f(3)
            );
            let body1 = if is_hub_caller {
                format!("{}(value) + 7", lang.hub_symbol())
            } else {
                format!("(value * {}) % {}", mult, modulo)
            };
            let _ = writeln!(
                out,
                "pub fn {}(value: i64) -> i64 {{\n    {}\n}}\n",
                f(1),
                body1
            );
            let _ = writeln!(
                out,
                "pub fn {}(value: i64) -> i64 {{\n    let label = \"{}\";\n    value + label.len() as i64\n}}\n",
                f(2),
                label
            );
            let body3 = match next {
                Some((next_slot, _)) => format!("{}(value * 2)", fn_name(*next_slot, 0)),
                None => "value".to_string(),
            };
            let _ = writeln!(
                out,
                "/// Bridge into the next module in the chain.\npub fn {}(value: i64) -> i64 {{\n    {}\n}}\n",
                f(3),
                body3
            );
            if has_cycle {
                for (leg, next_leg, base) in [('a', 'b', 0), ('b', 'c', 1), ('c', 'a', 2)] {
                    let _ = writeln!(
                        out,
                        "pub fn {}(depth: i64) -> i64 {{\n    if depth <= 0 {{\n        {}\n    }} else {{\n        {}(depth - 1)\n    }}\n}}\n",
                        cyc_name(slot, leg),
                        base,
                        cyc_name(slot, next_leg)
                    );
                }
                fns += 3;
            }
            if has_class {
                let _ = writeln!(
                    out,
                    "pub struct Worker{:05} {{\n    pub tally: i64,\n}}\n\nimpl Worker{:05} {{\n    pub fn process(&self, value: i64) -> i64 {{\n        {}(value) + self.tally\n    }}\n}}",
                    slot,
                    slot,
                    f(2)
                );
            }
        }
    }
    (out, fns)
}

fn config_yaml(slot: usize, rng: &mut SplitMix64) -> String {
    format!(
        "# Auto-generated synthetic service config (file {}).\nservice: svc_{:05}\nport: {}\ntimeout_ms: {}\nfeatures:\n  tracing: {}\n  retry: {}\n",
        slot,
        slot,
        8000 + slot % 1000,
        500 + rng.next_u64() % 4500,
        rng.next_u64().is_multiple_of(2),
        rng.next_u64().is_multiple_of(2),
    )
}

fn routes_ts(slot: usize) -> String {
    format!(
        "// Auto-generated synthetic route registrations (file {slot}).\nimport {{ Router }} from 'express';\n\nexport const apiRouter{slot:05} = Router();\n\napiRouter{slot:05}.get('/api/v{slot}/items', handleList{slot:05});\napiRouter{slot:05}.post('/api/v{slot}/items', handleCreate{slot:05});\n\nfunction handleList{slot:05}(req: any, res: any): void {{\n  res.json({{ page: {slot} }});\n}}\n\nfunction handleCreate{slot:05}(req: any, res: any): void {{\n  res.status(201).json({{ created: true }});\n}}\n"
    )
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(root: &Path) -> Vec<(String, String)> {
        let mut entries = Vec::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).expect("read synth dir") {
                let path = entry.expect("read synth entry").path();
                if path.is_dir() {
                    stack.push(path);
                } else {
                    let rel = path
                        .strip_prefix(root)
                        .expect("strip synth root")
                        .to_string_lossy()
                        .replace('\\', "/");
                    let content = std::fs::read_to_string(&path).expect("read synth file");
                    entries.push((rel, content));
                }
            }
        }
        entries.sort();
        entries
    }

    #[test]
    fn synth_generation_is_deterministic() {
        let spec = SynthSpec {
            target_files: 120,
            seed: 7,
        };
        let dir_a = tempfile::tempdir().expect("tempdir a");
        let dir_b = tempfile::tempdir().expect("tempdir b");
        let repo_a = generate(dir_a.path(), &spec).expect("generate a");
        let repo_b = generate(dir_b.path(), &spec).expect("generate b");

        assert_eq!(repo_a.files_written, 120);
        assert_eq!(repo_a.files_written, repo_b.files_written);
        assert_eq!(snapshot(dir_a.path()), snapshot(dir_b.path()));

        // A different seed must change content (filler words, needle name).
        let dir_c = tempfile::tempdir().expect("tempdir c");
        let spec_c = SynthSpec {
            target_files: 120,
            seed: 8,
        };
        let repo_c = generate(dir_c.path(), &spec_c).expect("generate c");
        assert_ne!(
            repo_a.ground_truth.needle_symbol,
            repo_c.ground_truth.needle_symbol
        );
        assert_ne!(snapshot(dir_a.path()), snapshot(dir_c.path()));
    }

    #[test]
    fn synth_ground_truth_facts_match_emitted_content() {
        let spec = SynthSpec {
            target_files: 120,
            seed: 42,
        };
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = generate(dir.path(), &spec).expect("generate");
        let gt = &repo.ground_truth;

        assert_eq!(
            repo.ts_files + repo.py_files + repo.rs_files + repo.extra_files,
            repo.files_written
        );
        assert_eq!(
            repo.code_file_paths.len(),
            repo.files_written - repo.extra_files
        );
        assert!(repo.functions_planned >= repo.code_file_paths.len() * 4);

        let read = |rel: &str| -> String {
            std::fs::read_to_string(dir.path().join(rel))
                .unwrap_or_else(|e| panic!("read {}: {}", rel, e))
        };

        // Needle: symbol and phrase exist exactly where claimed.
        let needle_src = read(&gt.needle_file);
        assert!(needle_src.contains(&gt.needle_symbol));
        assert!(needle_src.contains(&gt.needle_phrase));

        // Every recorded call fact: caller file mentions both endpoints, and
        // the callee file defines the callee.
        let all_facts = gt
            .hub_callers
            .iter()
            .chain(gt.py_hub_callers.iter())
            .chain(gt.chain.iter())
            .chain(std::iter::once(&gt.rs_intra));
        for fact in all_facts {
            let caller_src = read(&fact.caller_file);
            assert!(
                caller_src.contains(&fact.caller) && caller_src.contains(&fact.callee),
                "caller file {} must contain {} calling {}",
                fact.caller_file,
                fact.caller,
                fact.callee
            );
            assert!(
                read(&fact.callee_file).contains(&fact.callee),
                "callee file {} must define {}",
                fact.callee_file,
                fact.callee
            );
        }
        assert!(
            gt.hub_callers.len() >= 3,
            "hub fan-in should have at least 3 known callers, got {}",
            gt.hub_callers.len()
        );
        assert!(!gt.py_hub_callers.is_empty());

        // Cycle: all three legs live in the cycle file and close the loop.
        let cycle_src = read(&gt.cycle_file);
        for leg in &gt.cycle_symbols {
            assert!(cycle_src.contains(leg), "cycle file must define {}", leg);
        }
    }
}
