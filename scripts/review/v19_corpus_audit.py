#!/usr/bin/env python3
"""Read committed corpus/history evidence only; never freeze or modify gold."""
import argparse
import hashlib
import json
import subprocess
from pathlib import Path

BASE = '83a6b54ab1e71db033264e3b4e8d4f0a1d5319ad'
FREEZE = 'artifacts/benchmarks/p5e-g5-freeze-20261002'
TARGET = '0de7c890fcb2a152b4b21eafc4d8ad1a2c3885a6'
ROOT = Path(__file__).resolve().parents[2]


def git(*args):
    return subprocess.check_output(['git', *args], cwd=ROOT)


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def committed(path):
    return git('show', BASE + ':' + str(path))


def audit():
    manifests = []
    for path in git('ls-tree', '-r', '--name-only', BASE,
                    'crates/cc-eval/benchmarks/manifests').decode().splitlines():
        raw = committed(path)
        obj = json.loads(raw)
        if 'queries' not in obj:
            continue
        query_path = (ROOT / path).parent / obj['queries']
        relative = query_path.resolve().relative_to(ROOT)
        query_raw = committed(relative)
        rows = [json.loads(line) for line in query_raw.splitlines() if line.strip()]
        manifests.append({'manifest': path, 'sha256': sha(raw),
                          'query_path': str(relative), 'query_sha256': sha(query_raw),
                          'query_lock_matches': sha(query_raw) == obj['queries_digest'],
                          'queries': len(rows), 'holdout': sum(r.get('split') == 'holdout' for r in rows),
                          'splits': sorted({r.get('split', '') for r in rows}),
                          'families': sorted({r.get('query_family', '') for r in rows}),
                          'source': obj['source'], 'repetitions': obj['repetitions'],
                          'class': 'own-source-subset' if 'codecortex-subset' in path else 'synthetic-fixture'})
    prefix = 'artifacts/checkpoints/20261001-paused-github-sync/'
    index_raw = committed(prefix + 'evidence-index.json')
    index = json.loads(index_raw)
    related = [r for r in index['copied'] if 'original51-baseline' in r['original_path']]
    checks = [{'path': r['uploaded_path'], 'recorded_sha256': r['sha256'],
               'actual_sha256': sha(committed(r['uploaded_path']))} for r in related]
    omitted = [r for r in index['excluded_local'] if 'original51-baseline' in r['path']]
    snapshots = []
    for version in ['v1', 'v3']:
        root = prefix + 'evidence/artifacts/benchmarks/p5e-formal-runs-20261001-' + version + '/original51-baseline/'
        suites = []
        for n in range(4):
            path = root + f'suite-{n:03}/'
            mraw = committed(path + 'manifest.json')
            m = json.loads(mraw)
            metrics = json.loads(committed(path + 'metrics.json'))
            suites.append({'manifest': path + 'manifest.json', 'manifest_sha256': sha(mraw),
                           'questions': metrics['queries'], 'measured_rows': metrics['measured_rows'],
                           'normalized_digest_claim': m.get('normalized_digest'),
                           'query_snapshot_digest_claim': m.get('query_snapshot_digest'),
                           'engine': {k: v for k, v in m['engine'].items() if k in
                                      ['engine_head_observed', 'source_files_digest', 'binary_digest', 'tracked_diff_digest']},
                           'gate': json.loads(committed(path + 'gate.json'))})
        snapshots.append({'version': version, 'suites': suites,
                          'questions': sum(s['questions'] for s in suites),
                          'measured_rows': sum(s['measured_rows'] for s in suites)})
    objects = git('rev-list', '--objects', '--all', TARGET).decode().splitlines()
    hits = [r for r in objects if FREEZE in r or 'F0-FREEZE-RECEIPT' in r]
    native = json.loads(committed('crates/cc-eval/benchmarks/native/p4d-chunk-ablation.json'))
    gate_raw = committed('docs/roadmap/code-index-v2/P5-GATE.json')
    return {'schema': 1, 'base_sha': BASE, 'p5_gate_sha256': sha(gate_raw),
            'history_search': {'method': 'git rev-list --objects --all plus explicit historical target after fetching all authorized remote heads',
                               'object_rows': len(objects), 'missing_freeze_path': FREEZE,
                               'freeze_target': TARGET, 'matching_objects': hits,
                               'target_tree_paths': git('ls-tree', '-r', '--name-only', TARGET, '--', FREEZE).decode().splitlines(),
                               'path_history': git('log', '--all', TARGET, '--format=%H', '--', FREEZE).decode().splitlines(),
                               'conclusion': 'not recoverable from inspected committed history; not proof that original local/off-repository files never existed'},
            'curation': {'index_path': prefix + 'evidence-index.json', 'index_sha256': sha(index_raw),
                         'status': index['status'], 'checked_files': checks,
                         'all_102_hashes_match': all(x['recorded_sha256'] == x['actual_sha256'] for x in checks),
                         'excluded_records': omitted, 'older_snapshots': snapshots,
                         'warning': 'Earlier summary hash claims cannot reconstruct missing normalized or raw bytes and are not the 20261002 freeze.'},
            'inventory': manifests,
            'totals': {'manifest_questions': sum(m['queries'] for m in manifests),
                       'holdout_questions': sum(m['holdout'] for m in manifests),
                       'distinct_family_strings': len({f for m in manifests for f in m['families']}),
                       'legacy_fixture_toml_cases': len(git('ls-tree', '-r', '--name-only', BASE, 'crates/cc-eval/corpus').decode().split('.toml')) - 1,
                       'native_chunk_ablation_keys': list(native),
                       'completed_external_public_multirepo_reviewed_questions': 0,
                       'target': 600, 'remaining_target': 600},
            'scope': 'No production/gold/scorer/ledger edits; no live provider; no quality acceptance claim.'}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    result = audit()
    args.output.write_text(json.dumps(result, indent=2, ensure_ascii=False) + '\n')
    print(json.dumps({'totals': result['totals'], 'freeze_objects': len(result['history_search']['matching_objects']),
                      'retained_hashes_match': result['curation']['all_102_hashes_match']}))


if __name__ == '__main__':
    main()
