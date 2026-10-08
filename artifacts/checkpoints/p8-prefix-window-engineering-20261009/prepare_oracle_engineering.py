#!/usr/bin/env python3
"""Add the reviewed oracle batching candidate to the isolated engineering branch."""
import ast
import hashlib
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
SOURCE = Path('/dev/shm/p8-oracle-batch-code/crates/cc-eval/src/benchmark/oracle/streaming.rs')
BEFORE_WORKFLOW = Path('/dev/shm/p8-doc-key-index-root/p8-prefix-window-engineering.yml')

def sha(raw):
    return hashlib.sha256(raw).hexdigest()

def main():
    source = SOURCE.read_bytes()
    assert sha(source) == '5a51a72e8cb49c033754f860a3f5dc6366748ffe45a440da55cf7776cde7da0c'
    original = BEFORE_WORKFLOW.read_bytes()
    assert sha(original) == 'fdf3d3bffb674e0c9be5fb59b095a03e80d11425ff7048b23a8f54d0185cac22'
    old = original.decode()
    anchor = "              ['cargo', 'test', '--locked', '--offline', '-p', 'cc-eval', '--test', 'p8_scale', 'bounded_python_forwarding_resume_keeps_all_targets_and_fifteen_table_parity', '--', '--exact'],\n"
    additions = (
        "              ['cargo', 'test', '--locked', '--offline', '-p', 'cc-eval', '--lib', 'benchmark::oracle::streaming::tests::', '--', '--nocapture'],\n"
        "              ['cargo', 'test', '--locked', '--offline', '-p', 'cc-eval', '--test', 'benchmark_oracle_streaming'],\n"
        "              ['cargo', 'test', '--locked', '--offline', '-p', 'cc-eval', '--test', 'p8_scale'],\n"
    )
    assert old.count(anchor) == 1
    new = old.replace(anchor, anchor + additions)
    assert new.replace(additions, '', 1) == old
    workflow = HERE / 'p8-prefix-window-engineering.yml'
    workflow.write_text(new)
    def commands(text):
        start = text.index('          commands = [')
        end = text.index('          record = {', start)
        node = ast.parse('\n'.join(line[10:] for line in text[start:end].splitlines())).body[0]
        return ast.literal_eval(node.value)
    before_commands, after_commands = commands(old), commands(new)
    assert len(before_commands) == 9 and len(after_commands) == 12
    assert after_commands[:9] == before_commands
    reviews = [
        ('/dev/shm/p8-oracle-batch-independent-acceptance/review.json', '630492440aac428b096843ede65a6d342533a9067f29ab42a7d7a371cd077c42'),
        ('/dev/shm/p8-oracle-batch-independent-pr-audit/audit.json', 'ad0f97754f7a8cfef7c64c25c01a5f628363cf7a21e87d69e62421737e7a3a8d')]
    for path, digest in reviews:
        assert sha(Path(path).read_bytes()) == digest
    report = {
        'schema_version': 1, 'scope': 'Isolated engineering CI admission; no source-gate or TODO acceptance',
        'engineering_parent': '728e795a572d355c283be69653e8828138a57dd5',
        'reviewed_source': 'bf5be9f14000cc746e0fdcd59739ee953616fe61',
        'reviewed_source_base': 'd2d492b329a5630bf4a5fa6bc4b6eac30d07b13c',
        'oracle_path': 'crates/cc-eval/src/benchmark/oracle/streaming.rs',
        'oracle_sha256': sha(source), 'oracle_git_blob': '3c05342f976f534861347c098c3229ce988dfb61',
        'workflow_before_sha256': sha(original), 'workflow_sha256': sha(new.encode()),
        'workflow_difference': 'Append exactly three original/new oracle and whole integration commands; all previous commands and workload parameters are byte-identical',
        'required_commands': after_commands, 'independent_reviews': [{'path': p, 'sha256': d} for p, d in reviews],
        'source_code_authored_by': '/root/scale_engineering', 'workflow_adaptation_by': '/root',
        'rust_tests_executed_here': False, 'performance_measured_here': False,
        'limits': [
            'Retains original 15-table canonical/Value equality and source/row/canonical/scratch limits. No original scale samples, timeouts, raw limits, release admission or CI/source guards change.',
            'At most 64 pending canonical rows and 64KiB pending body bytes; valid larger rows use the original single-row path. This is not a process memory bound.',
            'Batching delays SQLite writes, so competing faults can change the first reported error and failed internal byte count. Every encountered SQL error exits without certifying equality. No physical rollback guarantee is added to journal_mode=OFF.',
            'Engineering 1k/10k diagnostics remain repetition 0 only under original 30-repetition partition parameters; they cannot certify the full N30 matrix.',
            'The separate PR160 G3 cache/source proof and original G scale run remain untouched.'
        ], 'todo_closed': 0, 'todo_remaining': 29,
    }
    (HERE / 'adoption-review.json').write_text(json.dumps(report, sort_keys=True, indent=2) + '\n')
    print(json.dumps({k: report[k] for k in ('engineering_parent', 'oracle_sha256', 'workflow_sha256', 'todo_closed', 'todo_remaining')}))

if __name__ == '__main__':
    main()
