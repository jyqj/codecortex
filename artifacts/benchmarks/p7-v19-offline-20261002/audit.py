#!/usr/bin/env python3
"""Independent identity/partition/paired-cell checks; no replacement scorer."""
import collections
import hashlib
import importlib.util
import json
from pathlib import Path
import re
import sys

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('experiment', HERE / 'experiment.py')
e = importlib.util.module_from_spec(spec)
spec.loader.exec_module(e)
output = Path(sys.argv[1]).resolve()
e.verify()
rows = [json.loads(l) for l in (HERE / 'queries/native.jsonl').read_text().splitlines()]
plan = json.loads((HERE / 'plan.json').read_text())
receipt = json.loads((output / 'build-receipt.json').read_text())
assert receipt['engine_crates_tree'] == receipt['source_baseline_crates_tree']
seen = {}
status = collections.Counter()
packing_partial = 0
paired = {}
for profile in ('native', 'compat'):
    baseline = json.loads((output / f'{profile}-baseline/manifest.json').read_text())
    base_scores = json.loads((output / f'{profile}-baseline/metrics.json').read_text())['cases']
    for arm in e.ARMS:
        directory = output / f'{profile}-{arm}'
        manifest = json.loads((directory / 'manifest.json').read_text())
        for field in ('source', 'queries_digest', 'seed', 'repetitions', 'warmup', 'top_k', 'timeout_ms'):
            assert manifest['suite'][field] == baseline['suite'][field], (arm, field)
        normalized = [json.loads(l) for l in (directory / 'normalized.jsonl').read_text().splitlines()]
        assert len(normalized) == 42
        assert collections.Counter(r['case_id'] for r in normalized) == {r['id']: 3 for r in rows}
        status.update(r['status'] for r in normalized)
        for row in normalized:
            raw = json.loads((directory / row['raw_path']).read_text())
            packing = raw.get('evidence_summary', {}).get('packing', {})
            packing_partial += int(packing.get('partial', False))
        metrics = json.loads((directory / 'metrics.json').read_text())
        if arm != 'rg_literal':
            paired[f'{profile}/{arm}'] = {
                'per_query_score_equal_to_baseline': metrics['cases'] == base_scores,
                'invalid_hits': metrics['invalid_hits'], 'unverified_hits': metrics['unverified_hits']}
        seen[f'{profile}/{arm}'] = manifest['suite']['source']['digest']
assert len(set(seen.values())) == 1
# Deterministic candidates from admitted source only. This deliberately does not
# change gold or call these independently reviewed hard negatives.
documents = {p: (HERE / 'source' / p).read_text() for p in plan['source_files']}
distractors = []
for row in rows:
    gold = {a['path'] for g in row['answers'] for a in g['alternatives']}
    tokens = set(re.findall(r'[A-Za-z_][A-Za-z_0-9]*', row['query'].lower()))
    candidates = []
    for path, source in documents.items():
        if path not in gold:
            overlap = sorted(tokens & set(re.findall(r'[A-Za-z_][A-Za-z_0-9]*', source.lower())))
            candidates.append({'path': path, 'token_overlap_count': len(overlap), 'tokens': overlap})
    candidates.sort(key=lambda c: (-c['token_overlap_count'], c['path']))
    distractors.append({'id': row['id'], 'candidate': candidates[0],
                        'certified_hard_negative': False, 'review_status': 'not_independently_reviewed'})
result = {
    'checks': 'passed: identical admitted corpus, paired query locks, all repetitions retained, crates tree binding, frozen input hashes',
    'source_sha': e.BASE, 'corpus_repository_visibility': 'public; verified GitHub get_repo jyqj/codecortex 2026-10-02',
    'source_hash_changed_since_original_gold': [r['id'] for r in plan['gold_source_audit'] if not r['unchanged']],
    'gold_freshness_issue': {'id': 'R09', 'missing_anchor': 'compute_fingerprint_for_unit',
                           'path': 'cc-index/src/indexer_phases/dirty.rs',
                           'present': 'compute_fingerprint_for_unit' in documents['cc-index/src/indexer_phases/dirty.rs'],
                           'action': 'original query/gold retained; not certified as fresh gold'},
    'all_row_statuses': dict(status), 'packing_partial_raw_rows': packing_partial,
    'mcp_per_query_comparisons': paired, 'distractor_candidates': distractors,
    'hard_negative_gate': 'blocked: lexical overlap candidates are not independently reviewed gold',
    'formal_V19': 'blocked; offline mechanism observations only',
}
e.write(output / 'audit.json', result)
# Rebuilding the report must be deterministic from retained official scores.
before = (output / 'aggregate.json').read_bytes()
e.report(output)
assert before == (output / 'aggregate.json').read_bytes(), 'Report regeneration changed scores'
print(json.dumps({k: result[k] for k in ('checks', 'all_row_statuses', 'packing_partial_raw_rows', 'formal_V19')}, indent=2))
