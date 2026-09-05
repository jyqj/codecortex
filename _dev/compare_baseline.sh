#!/usr/bin/env bash
# Development-only comparison. Base production source stays byte-for-byte at SHA.
set -euo pipefail
candidate=$PWD
base_sha=4514630dcd26481cf6dbc2aff38824ed71ef06da
base_dir=$(mktemp -d)
git fetch --depth=1 origin "$base_sha"
git worktree add --detach "$base_dir" "$base_sha"
trap 'git worktree remove --force "$base_dir"' EXIT
mkdir -p "$base_dir/crates/cc-eval/examples" "$base_dir/crates/cc-eval/benchmarks"
cp crates/cc-eval/src/quality.rs "$base_dir/crates/cc-eval/src/quality.rs"
cp crates/cc-eval/examples/quality_*.rs "$base_dir/crates/cc-eval/examples/"
cp crates/cc-eval/benchmarks/quality_smoke.json "$base_dir/crates/cc-eval/benchmarks/"
printf '\npub mod quality;\n' >> "$base_dir/crates/cc-eval/src/lib.rs"
# Same flags for both measured variants. Candidate warnings remain denied by
# the separate clippy/workspace jobs; the historical base has dead-code warnings.
export RUSTFLAGS='-D warnings -A dead_code'
export CARGO_TARGET_DIR="$candidate/target/paired-benchmark"
set +e
(cd "$base_dir" && cargo run -p cc-eval --locked --example quality_run -- crates/cc-eval/benchmarks/quality_smoke.json "$candidate/target/quality-base" 3) > /tmp/quality-base.log 2>&1
base_status=$?
set -e
# A benchmark gate failure is expected; a compile/startup failure is not.
test -f target/quality-base/report.json || { tail -100 /tmp/quality-base.log; exit 1; }
cargo run -p cc-eval --locked --example quality_run -- crates/cc-eval/benchmarks/quality_smoke.json target/quality-paired 3 > /tmp/quality-paired.log 2>&1 || { tail -100 /tmp/quality-paired.log; exit 1; }
python3 scripts/compare_quality.py target/quality-base/report.json target/quality-paired/report.json target/quality-comparison.json
python3 - "$base_status" <<'PY'
import json,sys
for name in ('base','paired'):
 r=json.load(open(f'target/quality-{name}/report.json'))
 print(name, json.dumps({k:r[k] for k in ('summary','gate_failures','latency')},indent=2))
print('Baseline exit status:',sys.argv[1])
PY
