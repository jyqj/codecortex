#!/usr/bin/env bash
set -euo pipefail
task_repo=$(git rev-parse --show-toplevel)
task_out="$task_repo/artifacts/diagnostics/requests-native-zero-20261004"
task_scratch=$(mktemp -d /tmp/requests-native-zero.XXXXXX)
tar xzf "$task_repo/artifacts/checkpoints/public-dev-paired-requests-20261004/paired-requests-raw.tar.gz" -C "$task_scratch"
python3 "$task_out/diagnose.py" "$task_scratch" > "$task_out/diagnose.log"
python3 - "$task_scratch" <<'PY'
import json, pathlib, subprocess, sys
scratch=pathlib.Path(sys.argv[1])
for file in json.loads((scratch/'candidate/native/manifest.json').read_bytes())['input']['files']:
    path=scratch/'source'/file['path'];path.parent.mkdir(parents=True,exist_ok=True)
    path.write_bytes(subprocess.check_output(['git','show','e49f9ada5826b4206d2128f3bfb8d31603ff42fa:crates/cc-eval/benchmarks/public-v19/requests/source/'+file['path']]))
PY
export PATH="/workspace/.cargo/bin:$PATH"
export RUSTUP_HOME=/workspace/.rustup CARGO_HOME=/workspace/.cargo
export REQUESTS_ZERO_PRODUCT_BINARY="$task_repo/target/debug/codecortex"
export REQUESTS_ZERO_OUTPUT="$task_out" REQUESTS_ZERO_RAW="$task_scratch" REQUESTS_ZERO_SOURCE="$task_scratch/source"
cargo +1.95.0 build --locked -p cc-server -p cc-eval > "$task_out/build.log" 2>&1
cargo +1.95.0 test --locked -p cc-eval --test requests_native_zero_analogue -- --nocapture > "$task_out/scoped-tests.log" 2>&1
# The final diagnostic seal is a historical record; reruns write new observations.
