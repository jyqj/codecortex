#!/bin/sh
# Reproduce the unchanged 5cce counterexample on PR117 without changing a live checkout.
set -eu
repair_repository=$(git rev-parse --show-toplevel)
repair_evidence="$repair_repository/artifacts/checkpoints/shared-provider-gate-repair-20261003"
repair_base=098ebd9c08031e0b652e7b021d8abbc9e8b19c3d
repair_root=$(mktemp -d /tmp/shared-provider-gate-base-XXXXXX)
git archive "$repair_base" | tar -x -C "$repair_root"
python3 - "$repair_root" "$repair_evidence/original-counterexample.rs" <<'PY'
from pathlib import Path
import sys
source = Path(sys.argv[1]) / 'crates/cc-server/src/semantic_runtime.rs'
text = source.read_text()
source.write_text(text[:text.rfind('\n}')] + Path(sys.argv[2]).read_text() + '\n}\n')
PY
cd "$repair_root"
export CARGO_HOME=/workspace/.cargo
export RUSTC=/workspace/.rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin/rustc
/workspace/.rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin/cargo test --locked -p cc-server --features semantic-http --lib semantic_runtime::tests::bounded_parallel_existing_first_wins_counterexample -- --exact --ignored --nocapture
# The one passing test asserts a violation (5/3); it is not a gate-contract pass.
