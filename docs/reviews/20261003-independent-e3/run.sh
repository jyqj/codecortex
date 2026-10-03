#!/bin/bash
# Run this new fixture in a fresh source copy. No original test is selected.
set -euo pipefail
repo=${1:?usage: bash run.sh repository new-scratch-path candidate-or-base}
scratch=${2:?new scratch path required}
mode=${3:-candidate}
case "$mode" in
  candidate) ref=e3c04fed903c4d0e3c26b5d7cf0e055a6f7c5207; filter=independent_e3_review ;;
  base) ref=513a98c9a94b15ec77153df41af26fa3c8c0b5e8; filter=independent_e3_review::independent_e3_synthetic_perf_only ;;
  *) exit 2 ;;
esac
compiler_bin=${CC_REVIEW_COMPILER_BIN:-/workspace/.rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin}
export PATH="$compiler_bin:$PATH"
export CARGO_HOME=${CARGO_HOME:-/workspace/.cargo}
export CARGO_TARGET_DIR="$scratch/target"
mkdir "$scratch"
git -C "$repo" archive "$ref" crates Cargo.toml Cargo.lock | tar -x -C "$scratch"
script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
cp "$script_dir/fixture.rs" "$scratch/crates/cc-server/src/independent_e3_review.rs"
printf '\n#[cfg(all(test, feature = "semantic-http"))]\nmod independent_e3_review;\n' >> "$scratch/crates/cc-server/src/lib.rs"
cd "$scratch"
# If the preauthorized registry cache is incomplete, first use normal cargo fetch --locked.
# Neither lock nor dependency/compiler substitutions are permitted.
cargo test --locked --offline -p cc-server --features semantic-http --lib "$filter" -- --test-threads=1 --nocapture
