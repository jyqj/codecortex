# Versioned benchmark data

P0 introduces a development-only `cc-eval` binary. The old TOML corpus and in-process runner remain available. The production `codecortex` CLI does not change.

Two score profiles are deliberately separate: `oce-compat-v1` uses any-expected Top-1 and linear-gain file nDCG; `codecortex-native-v1` uses primary answer groups and optional exact symbol/source-span requirements. Graph-chain scoring and full-scale release certification are not implemented by the P0 scoring skeleton. Missing source evidence is unverified, not silently considered valid.

Only admitted source files are copied into a disposable directory. `.codecortex.json` is explicitly generated from the locked engine config with auto-index disabled. Gold files are stored outside this source directory. A real Git corpus requires clean HEAD plus byte-level locks; snapshot fixtures explicitly use a null Git commit. P0 refuses submodules and unresolved LFS pointers rather than pretending to lock them correctly.

`freeze` is an explicit authoring operation. Neither `validate` nor `run` refreshes a lock. Each raw run uses a new output directory. Repetitions are retained; they are averaged per query before quality aggregation. Small-sample latency percentiles are labelled as insufficient for a tail-latency claim. Resource samples distinguish runner/server/process tree and do not claim to capture every transient peak.

The default run gate captures a baseline and checks integrity/availability. It does not certify retrieval quality just because the process exits 0. The comparison policy is versioned separately. An inconclusive comparison is not a pass.

## External reference

Method reference: `oce-ai/oce-benchmark@d4f10554a18e31599d1e46d5d56da6588d4aa86c`. Implementation here is independent. Upstream code and the 200-question corpus are not bundled; users supply external files with appropriate rights. Import receipts should retain the external file digests, metadata target commit, actual question count and permission provenance. `must_contain` remains an annotation in compatible data.

## Data governance

Keep all translations/paraphrases in one `query_family` and one split. Curated code must be read before writing its gold. Conflicting answers enter `quarantine`; the scorer rejects quarantine rows. New gold and production ranking changes require separate review. A local smoke corpus is not a holdout set or the planned multi-repository quality suite. Never add corpus-specific terms or gold-path boosts to production retrieval.
