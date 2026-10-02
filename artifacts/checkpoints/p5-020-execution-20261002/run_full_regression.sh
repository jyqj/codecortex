#!/bin/bash
# P5-020 evidence item 5: frozen-source dual-toolchain full regression.
# Command argv verbatim from playbook section 3.3 step 5 (= p5d final-v3 validation.json argv).
set -u
export SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX15.4.sdk
LOGDIR=/Users/jin/Desktop/codecortex-rust/artifacts/benchmarks/p5e-g5-freeze-20261002/regression
mkdir -p "$LOGDIR"
SUMMARY="$LOGDIR/validation.json"

run() {
  local label="$1"; shift
  local start=$(python3 -c 'import time;print(time.time())')
  "$@" > "$LOGDIR/$label.log" 2>&1
  local rc=$?
  local end=$(python3 -c 'import time;print(time.time())')
  python3 - "$label" "$rc" "$start" "$end" "$LOGDIR" <<'EOF'
import json, sys, os, time
label, rc, start, end, logdir = sys.argv[1], int(sys.argv[2]), float(sys.argv[3]), float(sys.argv[4]), sys.argv[5]
log = open(os.path.join(logdir, label + '.log'), errors='replace').read()
results = [l.strip() for l in log.splitlines() if l.startswith('test result:')]
passed = sum(int(r.split('passed')[0].split()[-1]) for r in results if 'passed' in r)
failed = sum(int(r.split('failed')[0].split()[-1]) for r in results if 'failed' in r)
ignored = sum(int(r.split('ignored')[0].split()[-1]) for r in results if 'ignored' in r)
entry = {"label": label, "exit_code": rc, "seconds": round(end - start, 3),
         "passed": passed, "failed": failed, "ignored": ignored,
         "result_lines": results,
         "finished_utc": time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime(end))}
path = os.path.join(logdir, 'validation.json')
db = json.load(open(path)) if os.path.exists(path) else {"commands": []}
db["commands"] = [c for c in db["commands"] if c["label"] != label] + [entry]
json.dump(db, open(path, 'w'), indent=2)
print(f"{label}: rc={rc} secs={entry['seconds']} passed={passed} failed={failed} ignored={ignored}")
EOF
}

run stable-clippy   cargo +stable clippy --workspace --all-targets --features cc-eval/eval-http --locked --offline -- -D warnings
run stable-workspace cargo +stable test --workspace --no-fail-fast --locked --offline -- --test-threads=1
run stable-http     cargo +stable test -p cc-eval --features eval-http --locked --offline -- --test-threads=1
run 1950-clippy     cargo +1.95.0 clippy --workspace --all-targets --features cc-eval/eval-http --locked --offline -- -D warnings
run 1950-workspace  cargo +1.95.0 test --workspace --no-fail-fast --locked --offline -- --test-threads=1
run 1950-http       cargo +1.95.0 test -p cc-eval --features eval-http --locked --offline -- --test-threads=1
run release-cost    cargo +stable test -p cc-eval --test p5a_cost --test p5b_cost --test p5c_cost --test p5d_cost --release --locked --offline -- --ignored --nocapture
echo "ALL_REGRESSION_DONE"
