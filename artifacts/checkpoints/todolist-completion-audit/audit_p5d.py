#!/usr/bin/env python3
"""Independent current-scope evidence audit. No Cargo run, no source mutation."""
import datetime
import hashlib
import json
import re
import subprocess
import sys
import tarfile
from pathlib import Path

R = Path(__file__).resolve().parents[3]
B = R / 'artifacts/benchmarks/p5d-20260930-resume'
O = B / sys.argv[1]
def load(p):
    return json.loads(p.read_text())
def digest(p):
    return hashlib.sha256(p.read_bytes()).hexdigest()
def require(ok, reason):
    if not ok:
        raise AssertionError(reason)

V = load(O / 'validation.json')
M = load(O / 'source-manifest.json')
review = load(O / 'source-review.json')
require(V['status'] == 'passed_P5D_016_018_local_scope', 'validation not terminal accepted local scope')
require(V['source_unchanged'], 'verification source drift')
require(review['source_manifest_matches_current_content'], 'source review not accepted content')
require(review['source_digest_sha256'] == M['source_digest_sha256'] == V['source_digest_sha256'], 'review closure mismatch')
require(subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=R, text=True).strip() == V['head'], 'HEAD drift')
require(hashlib.sha256(json.dumps(M['files'], sort_keys=True, separators=(',', ':')).encode()).hexdigest()
        == M['source_digest_sha256'], 'manifest canonical digest mismatch')
for row in M['files']:
    p = R / row['path']
    require(p.is_file() and p.stat().st_size == row['bytes'] and digest(p) == row['sha256'], 'current source drift: ' + row['path'])
require(digest(O / 'source.tar.gz') == V['archive_sha256'], 'archive hash drift')
with tarfile.open(O / 'source.tar.gz') as archive:
    require({m.name for m in archive.getmembers()} == {r['path'] for r in M['files']}, 'archive membership mismatch')
    for row in M['files']:
        require(hashlib.sha256(archive.extractfile(row['path']).read()).hexdigest() == row['sha256'], 'archive content mismatch')

expected = {'format', 'module-architecture', 'source-architecture', 'release-cost', 'corpus-locks', 'baseline-contract'}
for tc in ['stable', '1.95.0']:
    expected.update(tc + '-' + group for group in ['strict', 'bins', 'workspace', 'http', 'focused', 'protocol', 'real-mcp', 'watcher'])
for dataset in ['source', 'smoke', 'exact', 'intents']:
    for variant in ['p5b', 'p5c']:
        expected.update({'paired/' + variant + '-' + dataset, 'paired/replay-' + variant + '-' + dataset})
commands = {c['label']: c for c in V['commands']}
require(len(commands) == len(V['commands']) and set(commands) == expected, 'full command matrix omitted/duplicated/unexpected')
for c in V['commands']:
    require('reused_from' not in c, 'old receipt substituted')
    require(digest(R / c['log']) == c['log_sha256'], 'log hash mismatch: ' + c['label'])
    require(c['exit_code'] in c['allowed_exit_codes'], 'command failure: ' + c['label'])
    require(c['environment'].get('CODECORTEX_BENCH_PROCESS_PROBE') == '0', 'probe policy omitted')
    if 'test' in c['argv']:
        text = (R / c['log']).read_text()
        rows = re.findall(r'test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;', text)
        actual = {k: sum(int(row[i]) for row in rows) for i, k in enumerate(['passed', 'failed', 'ignored'])}
        require(actual == c['tests'] and actual['passed'] > 0 and actual['failed'] == 0 and c['exit_code'] == 0,
                'actual test result mismatch: ' + c['label'])
for binaries in V['binaries'].values():
    for row in binaries.values():
        require(row['immutable_attempt_copy'] and digest(R / row['path']) == row['sha256'], 'binary hash mismatch')
require(digest(R / V['baseline']['binary']) == V['baseline']['binary_sha256'], 'baseline binary drift')

old = {row['tool']: row for row in load(O / 'baseline-contract.json')}
new = {row['tool']: row for row in load(O / 'stable-contract.json')}
require(len(old) == len(new) == 14 and load(O / 'stable-contract.json') == load(O / '1.95.0-contract.json'), 'tool inventory/toolchain disagreement')
for tool, row in new.items():
    candidate = json.loads(json.dumps(row))
    if tool in ['search', 'context']:
        schema = candidate['input_schema']
        field = schema['properties'].pop('retrieval_strategy')
        require('retrieval_strategy' not in schema.get('required', []) and all(x in field['description'] for x in ['local', 'semantic']), 'parameter not additive')
    require(candidate == old[tool], 'legacy tool contract drift: ' + tool)

paired = load(O / 'paired/summary.json')
require(paired['questions'] == 51 and paired['requests'] == 306, 'paired workload mismatch')
for field in ['case_regressions', 'new_gate_failures', 'new_budget_partial_requests', 'unexpected_status_transitions']:
    require(not paired[field], 'runtime-only completeness/ranking regression: ' + field)
require(all(row['invalid_hits'] == 0 and row['replay_identical'] for row in paired['rows']), 'paired invalid source/replay')
for row in paired['rows']:
    if row['variant'] == 'p5c':
        before = next(x for x in paired['rows'] if x['variant'] == 'p5b' and x['dataset'] == row['dataset'])
        require(before['statuses'] == row['statuses'] and before['lane_reasons'] == row['lane_reasons'], 'completeness state drift')

# Recheck actual source text independently, not merely the evidence_valid flag.
suites = {
    'source': R / 'artifacts/benchmarks/p0-g0-20260927/frozen-inputs/p0-codecortex-subset/suite.json',
    'smoke': R / 'artifacts/benchmarks/p0-g0-20260927/frozen-inputs/p0-smoke/suite.json',
    'exact': R / 'crates/cc-eval/benchmarks/manifests/p1b-exact.json',
    'intents': R / 'crates/cc-eval/benchmarks/manifests/p1c-intents.json',
}
source_checks = 0
for dataset, suite in suites.items():
    root = (suite.parent / load(suite)['source']['root']).resolve()
    for variant in ['p5b', 'p5c']:
        run = O / ('paired/' + variant + '-' + dataset)
        for row in map(json.loads, (run / 'normalized.jsonl').read_text().splitlines()):
            for hit in row['hits']:
                p = (root / hit['path']).resolve()
                require(p.is_relative_to(root), 'source path escape')
                raw = p.read_bytes()
                require(hit['evidence_valid'] is True, 'unverified source hit')
                span, text = hit.get('span'), hit.get('text')
                if hit.get('source_evidence'):
                    require(span is not None and 0 <= span['start'] < span['end'] <= len(raw), 'source span invalid')
                    require(raw[span['start']:span['end']].decode('utf-8') == text, 'source exact byte/text mismatch')
                elif text:
                    require(text.strip() in raw.decode('utf-8').replace('\r\n', '\n'), 'legacy source text not in actual corpus')
                source_checks += 1
        resources = [json.loads(x) for x in (run / 'resources.jsonl').read_text().splitlines()]
        require(resources and all(x['runner_rss_bytes'] is None and x['server_rss_bytes'] is None
                                 and x['server_tree_rss_bytes'] is None for x in resources), 'disabled RSS not null')

for tc in ['stable', '1.95.0']:
    lifecycle = load(O / ('observations/' + tc + '-focused/p5d-lifecycle.json'))
    require(all(lifecycle[k] for k in ['same_runtime_after_lru', 'idle_skips_pin', 'db_reclaimed_after_release'])
            and lifecycle['fake_calls'] == 1, 'inflight/LRU/final DB release failure')
    policy = load(O / ('observations/' + tc + '-real-mcp/p5d-public-policy.json'))
    require(policy['capabilities']['retrieval']['dense_state'] == 'disabled' and policy['semantic_unavailable_tested']
            and policy['legacy_modes_preserved'], 'capability false ready/compatibility failure')
lifecycle = load(O / 'observations/release-cost/p5d-idle-cost.json')
require(lifecycle['status'] == 'passed' and len(lifecycle['samples']) == 12
        and all(x['first_closed'] == 16 - x['pinned_views'] and x['released_closed'] == x['pinned_views']
                and x['all_db_resources_reclaimed'] for x in lifecycle['samples']), 'release reclamation sample failure')
require(len(load(O / 'observations/release-cost/p5a-cost.json')['samples']) == 60, 'lane cost sample omission')
packing_cost = load(O / 'observations/release-cost/p5c-final-evidence-cost.json')
require(packing_cost['status'] == 'passed' and len(packing_cost['samples']) == 30
        and all(x['bytes'] <= 16000 for x in packing_cost['samples']), 'packing cost/budget failure')
rounds = load(O / 'observations/release-cost/p5b-execution-cost.json')['rounds']
require(len(rounds) == 3 and all(x['accepted'] == 36 and x['rejected'] == 28 and x['peak_workers'] <= 4
                               and x['settled']['cpu_admitted'] == 0 for x in rounds), 'admission/resource drain failure')
for group in ['native_queries', 'historical_inputs']:
    for path, locked_hash in load(B / 'development/source-lock-refresh.json')[group].items():
        require(digest(R / path) == locked_hash, 'corpus/gold lock drift: ' + path)
summary = {
    'schema_version': 1, 'reviewer': 'acceptance_auditor', 'status': 'passed_P5D_016_018_local_scope',
    'observed_at_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(), 'head': V['head'],
    'source_digest_sha256': M['source_digest_sha256'], 'source_files': M['file_count'],
    'command_count': len(commands), 'source_archive_logs_binaries_equal': True,
    'old_tool_contracts_preserved': 14, 'paired_requests': 306, 'independent_actual_source_hit_checks': source_checks,
    'full_retrieval_gate': paired['full_retrieval_gate'], 'accepted_tasks': ['P5-016', 'P5-017', 'P5-018'],
    'limits': ['P5-019/020/G5/M2 remain incomplete; original Partial/S11 debt retained.',
               'No full provider/semantic publication/holdout/100k/tail/RSS/cross-platform/release certification.',
               'Both toolchains and all groups rechecked from actual frozen logs; reviewer did not rerun Cargo.',
               'Source bytes independently compared; BLAKE3 proof handled by reviewed Rust normalizer, not Python rehash.',
               'Mixed-load preparation tested as part of workspace, not whole P5-019 acceptance.'],
}
target = O / 'audit.json'
require(not target.exists(), 'never overwrite audit')
target.write_text(json.dumps(summary, ensure_ascii=False, indent=2) + '\n')
print(json.dumps(summary, ensure_ascii=False, indent=2))
