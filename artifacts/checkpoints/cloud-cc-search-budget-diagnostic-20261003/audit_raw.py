"""Read frozen archives via git; emit aggregates only, never queries/gold/body."""
import collections, hashlib, io, json, re, subprocess, tarfile
from pathlib import Path
ROOT = Path(__file__).parent
SOURCES = [
 ('pygo', 'fff0931e5b470aa3b9111d6ce55444f9b40f98cf', 'public-dev-current-group-pygo-20261003/public-development-current-raw.tar.gz'),
 ('js', '535ff1b13b841af8021346a660c83c57919525e2', 'public-dev-current-group-js-20261003/public-development-current-group-js-raw.tar.gz'),
]
token = re.compile(r'[A-Za-z0-9_]+|[\u4e00-\u9fff]{1,8}')
result = {}
for group, rev, suffix in SOURCES:
    payload = subprocess.check_output(['git', 'show', rev + ':artifacts/checkpoints/' + suffix])
    archive = tarfile.open(fileobj=io.BytesIO(payload))
    counters = collections.defaultdict(collections.Counter)
    rows = 0
    for entry in archive:
        if '/raw/' not in entry.name or not entry.name.endswith('.json') or entry.name.startswith('pilot-current01/'):
            continue
        raw = json.load(archive.extractfile(entry))
        rows += 1
        q = raw.get('query', '')
        tokens = token.findall(q)
        counters['query_token_count'][str(len(tokens))] += 1
        retrieval = raw.get('evidence_summary', {}).get('retrieval', {})
        budget = retrieval.get('scope', {}).get('budget', {})
        counters['observed_budget'][json.dumps(budget, sort_keys=True)] += 1
        lanes = retrieval.get('lane_receipts', retrieval.get('lanes', []))
        if not lanes:
            counters['receipt_presence']['absent'] += 1
        else:
            counters['receipt_presence']['present'] += 1
        for lane in lanes:
            key = lane['lane_id'] + '::' + lane['status'] + '::' + str(lane.get('truncation_reason'))
            counters['lane_status_reason'][key] += 1
            counters['candidate_count_by_lane'][lane['lane_id'] + '::' + str(lane['candidate_count'])] += 1
            if lane['lane_id'] == 'graph':
                counters['graph_partial_vs_token_omission'][str((lane['status'] == 'partial', len(tokens) > 5))] += 1
            if lane['lane_id'] == 'lexical':
                cap = budget.get('lexical_candidates')
                counters['lexical_reason_vs_candidate_watermark'][str((lane.get('truncation_reason'), lane['candidate_count'], cap))] += 1
        counters['effective_policy'][retrieval.get('policy', {}).get('effective', 'absent')] += 1
        counters['semantic_state'][retrieval.get('policy', {}).get('semantic_state', 'absent')] += 1
    result[group] = {'source_commit': rev, 'archive_sha256': hashlib.sha256(payload).hexdigest(),
                     'rows': rows, 'aggregates': {k: dict(sorted(v.items())) for k, v in counters.items()}}
(ROOT / 'raw-aggregate.json').write_text(json.dumps(result, indent=2) + '\n')
print(json.dumps({k: {'rows': v['rows'], 'graph': v['aggregates']['graph_partial_vs_token_omission'],
                      'lexical': v['aggregates']['lexical_reason_vs_candidate_watermark']} for k,v in result.items()}, indent=2))
