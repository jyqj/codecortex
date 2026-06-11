#!/usr/bin/env bash
set -euo pipefail

cd "$(git rev-parse --show-toplevel 2>/dev/null || dirname "$0"/..)"

echo "=== Collecting baselines ==="

SCHEMA_VERSION=$(grep 'pub const CURRENT_SCHEMA_VERSION' crates/cc-db/src/index_migrate.rs | grep -o '= [0-9]*' | grep -o '[0-9]*')
TOOL_COUNT=$(grep -c '#\[tool(' crates/cc-server/src/mcp.rs)
CORPUS_COUNT=$(ls crates/cc-eval/corpus/*.toml 2>/dev/null | wc -l | tr -d ' ')
# Count registered resolvers (not files: helper modules like
# mount_resolution.rs / python_patterns.rs are not resolvers).
RESOLVER_COUNT=$(grep -c 'registry.register' crates/cc-index/src/framework_resolvers/mod.rs)

TEST_OUTPUT=$(cargo test --workspace --all-targets 2>&1)
TOTAL_PASSED=$(echo "$TEST_OUTPUT" | grep '^test result:' | awk '{sum+=$4} END {print sum}')
TOTAL_IGNORED=$(echo "$TEST_OUTPUT" | grep '^test result:' | awk '{sum+=$8} END {print sum}')
TOTAL=$((TOTAL_PASSED + TOTAL_IGNORED))

# Pair each "Running ... (target/debug/deps/<binary>-<hash>)" line with its
# following "test result:" line, so counts are keyed by binary name instead of
# positional line numbers (which break whenever a test target is added).
RESULTS=$(echo "$TEST_OUTPUT" | awk '
    /^ *Running / {
        bin = $0
        sub(/.*deps\//, "", bin)
        sub(/-[0-9a-f]+\)$/, "", bin)
    }
    /^test result:/ { print bin, $4, $8 }
')
passed_for() { echo "$RESULTS" | awk -v b="$1" '$1 == b { print $2 }'; }
ignored_for() { echo "$RESULTS" | awk -v b="$1" '$1 == b { print $3 }'; }

DB_TESTS=$(passed_for cc_db)
EVAL_PASSED=$(passed_for cc_eval)
EVAL_IGNORED=$(ignored_for cc_eval)
INDEX_TESTS=$(passed_for cc_index)
MODEL_TESTS=$(passed_for cc_model)
PARSER_TESTS=$(passed_for cc_parsers)
SEARCH_TESTS=$(passed_for cc_search)
SERVER_TESTS=$(passed_for cc_server)

echo ""
echo "Schema version:      v${SCHEMA_VERSION}"
echo "MCP tools:           ${TOOL_COUNT}"
echo "Eval corpus cases:   ${CORPUS_COUNT}"
echo "Framework resolvers: ${RESOLVER_COUNT}"
echo "Total tests:         ${TOTAL} (${TOTAL_PASSED} passed + ${TOTAL_IGNORED} ignored)"
echo ""
echo "Per-crate breakdown:"
echo "  cc-db:      ${DB_TESTS}"
echo "  cc-eval:    ${EVAL_PASSED} passed + ${EVAL_IGNORED} ignored"
echo "  cc-index:   ${INDEX_TESTS}"
echo "  cc-model:   ${MODEL_TESTS}"
echo "  cc-parsers: ${PARSER_TESTS}"
echo "  cc-search:  ${SEARCH_TESTS}"
echo "  cc-server:  ${SERVER_TESTS}"

EXPECTED_DB=$(grep 'cc-db' docs/TEST_PLAN.md | grep -o '| [0-9]* |' | head -1 | tr -dc '0-9')
EXPECTED_PASSED=$(head -5 docs/TEST_PLAN.md | grep -o '[0-9]* passed' | head -1 | grep -o '[0-9]*')
EXPECTED_IGNORED=$(head -5 docs/TEST_PLAN.md | grep -o '[0-9]* ignored' | head -1 | grep -o '[0-9]*')
EXPECTED_CORPUS=$(grep -o '[0-9]* corpus cases' docs/TEST_PLAN.md | head -1 | grep -o '[0-9]*')

DRIFT=0
if [ "$DB_TESTS" != "$EXPECTED_DB" ] 2>/dev/null; then
    echo ""
    echo "DRIFT: cc-db tests changed: doc=${EXPECTED_DB} actual=${DB_TESTS}"
    DRIFT=1
fi
if [ "$TOTAL_PASSED" != "$EXPECTED_PASSED" ] 2>/dev/null; then
    echo ""
    echo "DRIFT: passed count changed: doc=${EXPECTED_PASSED} actual=${TOTAL_PASSED}"
    DRIFT=1
fi
if [ "$TOTAL_IGNORED" != "$EXPECTED_IGNORED" ] 2>/dev/null; then
    echo ""
    echo "DRIFT: ignored count changed: doc=${EXPECTED_IGNORED} actual=${TOTAL_IGNORED}"
    DRIFT=1
fi
if [ "$CORPUS_COUNT" != "$EXPECTED_CORPUS" ] 2>/dev/null; then
    echo ""
    echo "DRIFT: corpus case count changed: doc=${EXPECTED_CORPUS} actual=${CORPUS_COUNT}"
    DRIFT=1
fi

if [ "$DRIFT" -eq 0 ]; then
    echo ""
    echo "All baselines match docs/TEST_PLAN.md"
fi
