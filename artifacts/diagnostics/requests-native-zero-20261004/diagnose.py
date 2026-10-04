"""Read-only census of fixed public DEV evidence; never emits query/gold bodies.

Usage: python3 diagnose.py /tmp/requests-zero
Extract the fixed archive there first. Only this diagnostic directory is written.
The progressive counts are predicates, NOT replacement scores.
"""
import collections
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import importlib.util

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
FIXED = ROOT / 'artifacts/checkpoints/public-dev-paired-requests-20261004'
RAW = Path(sys.argv[1])
sha = lambda b: hashlib.sha256(b).hexdigest()
def blob(ref, path):
    return subprocess.check_output(['git', 'show', ref + ':' + path], cwd=ROOT)
def read(path):
    return json.loads(path.read_bytes())
def rows(path):
    return [json.loads(x) for x in path.read_bytes().splitlines()]
def save(name, value):
    (HERE / name).write_text(json.dumps(value, indent=2, sort_keys=True) + '\n')

manifest = read(FIXED / 'raw-artifact-manifest.json')
assert sha((FIXED / 'paired-requests-raw.tar.gz').read_bytes()) == manifest['archive_sha256']
for path, entry in manifest['files'].items():
    data = (RAW / path).read_bytes()
    assert len(data) == entry['bytes'] and sha(data) == entry['sha256'], path
verification = {'archive_sha256': manifest['archive_sha256'],
                'archive_files_verified': len(manifest['files']), 'bindings': {}}
loaderpath = ROOT / 'artifacts/checkpoints/public-dev-declaration-kind-admission-20261003/selector.py'
spec = importlib.util.spec_from_file_location('fixed_selector', loaderpath)
loader = importlib.util.module_from_spec(spec)
spec.loader.exec_module(loader)
receipt = read(loader.HERE / 'admission-receipt.json')
actual = {}
for entry, digest in receipt['source_bytes_sha256'].items():
    ref, path = entry.split(':', 1)
    data = blob(ref, path)
    assert sha(data) == digest
    actual[entry] = data
packages = loader.load_version(version=loader.VERSION, repositories=['requests', 'gin'], source_bytes=actual)
assert sha(packages['requests']['native']) == '7847fbe8326977121e964ee3729d47ce4855e7ce51b32c3918d8bd5ef77c3730'
verification['admitted_source_files_verified'] = len(actual)
verification['admitted_native_sha256'] = sha(packages['requests']['native'])
for side, ref in [('base', '88f2cf099c8b81f3acef485fd5ac9b01c63ce790'),
                  ('candidate', '37dd042eaa1209a86e0cafdcd92ae77e036e76f5')]:
    binding = read(FIXED / f'binding-{side}.json')
    assert binding['head'] == ref
    for path, digest in binding['source_sha256'].items():
        assert sha(blob(ref, path)) == digest, (side, path)
    verification['bindings'][side] = {'ref': ref, 'source_files_verified': len(binding['source_sha256']),
                                    'historical_binary_sha256': binding['binary_sha256'],
                                    'binary_rehash_here': 'not_available_in_archive'}
    for mode in ['native', 'compat']:
        run = RAW / side / mode
        m = read(run / 'manifest.json')
        for entry in m['input']['files']:
            body = blob('e49f9ada5826b4206d2128f3bfb8d31603ff42fa',
                        'crates/cc-eval/benchmarks/public-v19/requests/source/' + entry['path'])
            assert len(body) == entry['bytes']
        assert m['input']['source_digest'] == '1bdc380f176245109e8058c9abc3704ac7d29befb5a887665339ba4f9a977bc1'
    # The evaluator snapshot is reserialized JSON, unlike the original admitted bytes.
    assert rows(RAW / side / 'native/queries.jsonl') == [json.loads(x) for x in packages['requests']['native'].splitlines()]
    assert (RAW / side / 'native/queries.jsonl').read_bytes() == (RAW / 'candidate/native/queries.jsonl').read_bytes()
diff = subprocess.check_output(['git', 'diff', '37dd042eaa1209a86e0cafdcd92ae77e036e76f5',
                               '90858afae647a513537bf118932a7ba5020ee98b', '--',
                               'Cargo.lock', 'Cargo.toml'] + ['crates/' + c for c in
                               ['cc-db', 'cc-eval', 'cc-index', 'cc-model', 'cc-parsers', 'cc-search', 'cc-semantic', 'cc-server']], cwd=ROOT)
# Fixed combination may carry tests/docs. Compare production source/Cargo separately.
production = subprocess.check_output(['git', 'diff', '37dd042eaa1209a86e0cafdcd92ae77e036e76f5',
                                    '90858afae647a513537bf118932a7ba5020ee98b', '--',
                                    'Cargo.lock', 'Cargo.toml'] + sum(([f'crates/{c}/src', f'crates/{c}/Cargo.toml'] for c in
                                    ['cc-db', 'cc-eval', 'cc-index', 'cc-model', 'cc-parsers', 'cc-search', 'cc-semantic', 'cc-server']), []), cwd=ROOT)
assert not production
verification['candidate_product_source_diff_bytes'] = len(production)
save('verification.json', verification)

out = {}
for side in ['base', 'candidate']:
    run = RAW / side / 'native'
    qs = {q['id']: q for q in rows(run / 'queries.jsonl')}
    rr = rows(run / 'normalized.jsonl')
    stages = ['path', 'proof', 'name', 'kind', 'span', 'qname']
    pairs, hitcounts, rowcounts, groupcounts = (collections.Counter() for _ in range(4))
    queries_at_stage = {k:set() for k in stages}
    firstfail = collections.Counter()
    masks = collections.Counter()
    qnamespace = collections.Counter()
    module_only_pairs = module_only_hits = module_only_rows = 0
    raw_identity_mismatches = 0
    for r in rr:
        q = qs[r['case_id']]
        raw = read(run / r['raw_path'])
        rawhits = raw.get('machine_pack', {}).get('hits', raw.get('nodes', []))
        # machine_pack is the exact authoritative normalizer source for this run.
        assert 'hits' in raw.get('machine_pack', {})
        assert len(rawhits) == len(r['hits'])
        row_pass = set()
        row_module = False
        for h, rh in zip(r['hits'], rawhits):
            meta = rh.get('metadata') or {}
            for field, value in [('path', rh.get('file_path')),
                                 ('symbol_name', rh.get('symbol_name') or meta.get('symbol_name')),
                                 ('qname', rh.get('qname') or meta.get('qname')),
                                 ('kind', rh.get('kind') or rh.get('symbol_kind') or meta.get('symbol_kind'))]:
                raw_identity_mismatches += h.get(field) != value
            hit_pass = set()
            hit_module = False
            for gi, g in enumerate(q['answers']):
                for a in g['alternatives']:
                    s = a.get('symbol') or {}
                    gs, hs = a.get('span'), h.get('span')
                    pred = {'path': h['path'] == a['path'], 'proof': h.get('evidence_valid') is not False,
                            'name': not s or h.get('symbol_name') == s['name'],
                            'kind': not s.get('kind') or h.get('kind') == s['kind'],
                            'span': not gs or bool(hs and max(gs['start'], hs['start']) < min(gs['end'], hs['end'])),
                            'qname': not s.get('qname') or h.get('qname') == s['qname']}
                    mask = ','.join(k for k in stages if not pred[k]) or 'strict_match'
                    masks[mask] += 1
                    # Exact scorer early-return order, separate from diagnostic location-first progression.
                    fail = next((k for k in ['path', 'proof', 'name', 'qname', 'kind', 'span'] if not pred[k]), 'strict_match')
                    firstfail[fail] += 1
                    passing = True
                    for stage in stages:
                        passing &= pred[stage]
                        if passing:
                            pairs[stage] += 1
                            hit_pass.add(stage)
                            row_pass.add(stage)
                            groupcounts[(r['case_id'], r['repetition'], gi, stage)] = 1
                    if all(pred[k] for k in stages if k != 'qname'):
                        if h.get('qname') is None:
                            qnamespace['missing'] += 1
                        elif pred['qname']:
                            qnamespace['equal'] += 1
                        else:
                            mod = a['path'].removeprefix('src/').removesuffix('.py').replace('/', '.')
                            # Only classification, never remove a prefix or rewrite a hit/gold/score.
                            module_equal = s.get('qname') == mod + '.' + h['qname']
                            qnamespace['module_plus_lexical' if module_equal else 'other_mismatch'] += 1
                            if module_equal:
                                module_only_pairs += 1
                                hit_module = row_module = True
            hitcounts.update(hit_pass)
            module_only_hits += hit_module
        rowcounts.update(row_pass)
        for stage in row_pass:
            queries_at_stage[stage].add(r['case_id'])
        module_only_rows += row_module
    alternatives = [a for q in qs.values() for g in q['answers'] for a in g['alternatives']]
    noanswer = [r for r in rr if qs[r['case_id']]['no_answer']]
    out[side] = {'rows': len(rr), 'unique_queries': len(qs), 'alternatives': len(alternatives),
                 'hits': sum(len(r['hits']) for r in rr), 'qname_nonnull': sum(h.get('qname') is not None for r in rr for h in r['hits']),
                 'status': dict(collections.Counter(r['status'] for r in rr)),
                 'answer_rows': len(rr)-len(noanswer), 'no_answer_rows': len(noanswer),
                 'no_answer_empty_rows': sum(not r['hits'] for r in noanswer),
                 'gold_qname_required_alternatives': sum(bool((a.get('symbol') or {}).get('qname')) for a in alternatives),
                 'cumulative_order': stages, 'pair_counts': dict(pairs), 'hit_counts': dict(hitcounts),
                 'row_counts': dict(rowcounts),
                 'unique_query_counts': {k:len(v) for k,v in queries_at_stage.items()},
                 'answer_groups_across_rows':sum(len(qs[r['case_id']]['answers']) for r in rr),
                 'all_hits_source_verified':sum(h.get('evidence_valid') is True for r in rr for h in r['hits']),
                 'all_hits_span_present':sum(h.get('span') is not None for r in rr for h in r['hits']),
                 'group_counts': dict(collections.Counter(key[3] for key in groupcounts)),
                 'first_fail_scorer_order_pairs': dict(firstfail), 'failure_mask_pairs': dict(masks),
                 'qname_at_other_fields_pass_pairs': dict(qnamespace),
                 'module_namespace_only': {'pairs': module_only_pairs, 'hits': module_only_hits, 'rows': module_only_rows},
                 'raw_to_normalized_identity_mismatches': raw_identity_mismatches}
    assert raw_identity_mismatches == 0
save('aggregate.json', out)
print(json.dumps(out, indent=2))
