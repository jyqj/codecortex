#!/usr/bin/env bash
set -eu
cd /workspace/codecortex
export PATH=/workspace/.cargo/bin:$PATH CARGO_HOME=/workspace/.cargo RUSTUP_HOME=/workspace/.rustup
export INDEPENDENT_EVIDENCE=/workspace/codecortex/artifacts/checkpoints/independent-go-call-fix-20261003
export INDEPENDENT_GIN="$INDEPENDENT_EVIDENCE/context.go" INDEPENDENT_NAMES="$INDEPENDENT_EVIDENCE/gin-base-callees.json"
python3 "$INDEPENDENT_EVIDENCE/prepare.py"
INDEPENDENT_OLD=1 CARGO_TARGET_DIR=/tmp/independent-go-call-fix-20261003/target-base cargo test --manifest-path /tmp/independent-go-call-fix-20261003/base/Cargo.toml -p cc-index --test independent_go_call_fix --locked --offline -- --nocapture > "$INDEPENDENT_EVIDENCE/base-test.log" 2>&1
CARGO_TARGET_DIR=/tmp/independent-go-call-fix-20261003/target-fixed cargo test --manifest-path /tmp/independent-go-call-fix-20261003/fixed/Cargo.toml -p cc-index --test independent_go_call_fix --locked --offline -- --nocapture > "$INDEPENDENT_EVIDENCE/fixed-test.log" 2>&1
