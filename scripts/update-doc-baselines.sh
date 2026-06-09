#!/usr/bin/env bash
set -euo pipefail

cd "$(git rev-parse --show-toplevel 2>/dev/null || dirname "$0"/..)"

echo "=== Collecting baselines ==="

SCHEMA_VERSION=$(grep 'pub const CURRENT_SCHEMA_VERSION' crates/cc-db/src/index_migrate.rs | grep -o '= [0-9]*' | grep -o '[0-9]*')
TOOL_COUNT=$(grep -c '#\[tool(' crates/cc-server/src/mcp.rs)
CORPUS_COUNT=$(ls crates/cc-eval/corpus/*.toml 2>/dev/null | wc -l | tr -d ' ')
RESOLVER_COUNT=$(ls crates/cc-index/src/framework_resolvers/*.rs 2>/dev/null | grep -cv mod.rs)

TEST_OUTPUT=$(cargo test --workspace --all-targets 2>&1)
TOTAL_PASSED=$(echo "$TEST_OUTPUT" | grep '^test result:' | awk '{sum+=$4} END {print sum}')
TOTAL_IGNORED=$(echo "$TEST_OUTPUT" | grep '^test result:' | awk '{sum+=$8} END {print sum}')
TOTAL=$((TOTAL_PASSED + TOTAL_IGNORED))

DB_TESTS=$(echo "$TEST_OUTPUT" | grep '^test result:' | sed -n '1p' | awk '{print $4}')
EVAL_PASSED=$(echo "$TEST_OUTPUT" | grep '^test result:' | sed -n '2p' | awk '{print $4}')
EVAL_IGNORED=$(echo "$TEST_OUTPUT" | grep '^test result:' | sed -n '2p' | awk '{print $8}')
INDEX_TESTS=$(echo "$TEST_OUTPUT" | grep '^test result:' | sed -n '3p' | awk '{print $4}')
MODEL_TESTS=$(echo "$TEST_OUTPUT" | grep '^test result:' | sed -n '4p' | awk '{print $4}')
PARSER_TESTS=$(echo "$TEST_OUTPUT" | grep '^test result:' | sed -n '5p' | awk '{print $4}')
SEARCH_TESTS=$(echo "$TEST_OUTPUT" | grep '^test result:' | sed -n '6p' | awk '{print $4}')
SERVER_TESTS=$(echo "$TEST_OUTPUT" | grep '^test result:' | sed -n '7p' | awk '{print $4}')

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

if [ "$DRIFT" -eq 0 ]; then
    echo ""
    echo "All baselines match docs/TEST_PLAN.md"
fi
