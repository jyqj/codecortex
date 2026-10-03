#!/usr/bin/env python3
"""Independent audit of frozen author receipts, never consumes result.gold."""
import hashlib
import json
import pathlib

ROOT = pathlib.Path(__file__).resolve().parents[2]
BLOCK = ROOT / 'artifacts/benchmarks/p7-v16-2d48f26-9bdab23f-cosine-v1-20261002'
SCORES = {'scope/top.rs': .96, 'scope/tie_a.rs': .6, 'scope/tie_b.rs': .6,
          'scope/zero.rs': 0., 'scope/near.py': .8, 'outside/strong.rs': 1.}
CASES = {
    'filter_before_top1': {'scope/top.rs'},
    'hand_top3_tie': {'scope/top.rs', 'scope/tie_a.rs', 'scope/tie_b.rs'},
    'hand_orthogonal_zero': {'scope/top.rs', 'scope/tie_a.rs', 'scope/tie_b.rs', 'scope/zero.rs'},
    'language_path_file_intersection': {'scope/tie_b.rs'},
    'python_scope': {'scope/near.py'},
    'some_empty': set(),
}

def check(outcome, expected):
    candidates = outcome['actual']
    assert outcome['coverage']['complete'] is True
    paths = []
    ordered = []
    for candidate in candidates:
        chunk = candidate['legacy_chunk_id']
        assert chunk.startswith('chunk:') and chunk.endswith(':0')
        path = chunk[len('chunk:'):-2]
        paths.append(path)
        assert path in expected
        assert abs(candidate['raw_score'] - SCORES[path]) < 2e-6
        assert candidate['scoring_spec'] == 'cosine-exact-v1'
        ordered.append((-SCORES[path], candidate['document']['doc_key']))
    assert set(paths) == expected and len(paths) == len(expected)
    assert ordered == sorted(ordered), 'scores descend / document keys ascend in ties'

commands = json.loads((BLOCK / 'commands.json').read_text())
assert len(commands) == 5
case_count = 0
fixture_count = 0
for run, command in enumerate(commands, 1):
    raw = BLOCK / 'raw' / f'{run:02}.json'
    assert hashlib.sha256(raw.read_bytes()).hexdigest() == command['raw_sha256']
    assert command['exit'] == 0
    fixtures = json.loads(raw.read_text())
    assert [row['seed'] for row in fixtures] == [0, 1, 3]
    for row in fixtures:
        fixture_count += 1
        assert len(row['cases']) == 6
        assert {case['name'] for case in row['cases']} == set(CASES)
        for case in row['cases']:
            check(case['result'], CASES[case['name']])
            case_count += 1
        check(row['delete'], {'scope/tie_a.rs', 'scope/tie_b.rs'})
        for envelope in [row['cold_public'], row['after_delete_hydrate']]:
            retrieval = envelope['evidence_summary']['retrieval']
            receipts = retrieval.get('lane_receipts', retrieval.get('lanes'))
            assert receipts is not None
            lane = next(lane for lane in receipts if lane['lane_id'] == 'semantic')
            assert lane['status'] == 'complete' and lane['candidate_count'] > 0
            hits = envelope['machine_pack']['hits']
            assert hits
            assert any(term[0] == 'rrf:semantic' for hit in hits for term in hit['score_trace'])
            allowed = {'scope/top.rs', 'scope/tie_a.rs', 'scope/tie_b.rs', 'scope/zero.rs'}
            if envelope is row['after_delete_hydrate']:
                allowed.remove('scope/top.rs')
            assert all(hit['file_path'] in allowed for hit in hits)
        old = row['old_space_rejection']
        assert old['status'] == 'unavailable'
        assert old['truncation_reason'] == 'semantic_space_not_active'
        assert old['candidates'] == []
        assert row['query_posts'] == 2
assert fixture_count == 15 and case_count == 90
print(json.dumps({'fixtures_checked': fixture_count, 'case_receipts_checked': case_count,
                  'result_gold_consumed': False, 'recorded_query_posts': 30,
                  'query_counters_independently_reexecuted': False,
                  'independent_quality_questions': 0}, indent=2))
