#!/usr/bin/env python3
"""Offline config experiment. Official cc-eval computes every relevance score.
Never refresh frozen suites during measurement. Existing dev gold is preserved;
retrospective holdout is explicitly not an unseen D5 certification dataset.
"""
import argparse
import collections
import hashlib
import json
import os
from pathlib import Path
import random
import statistics
import subprocess

BASE = '58e20d08285303fc9e12ed8cdfa21d6afb84910b'
INTEGRATION = 'd6462c163abe6ca92d2ae0a2cf967b1ad2433e22'
SEED = 2719013
ARMS = {
    'baseline': {},
    'graph_vote_off': {'search': {'graph_weight': 0.0}},
    'graph_rerank_off': {'ranking': {'graph_rerank_weight': 0.0}},
    'lexical_votes_only': {'search': {'exact_weight': 0.0, 'path_weight': 0.0,
                                    'grep_weight': 0.0, 'graph_weight': 0.0},
                           'ranking': {'graph_rerank_weight': 0.0}},
    'rg_literal': {},
}
HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]


def digest(data):
    return hashlib.sha256(data).hexdigest()


def write(path, value):
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + '\n')


def git_bytes(path):
    return subprocess.check_output(['git', 'show', f'{BASE}:{path}'], cwd=REPO)


def split(rows):
    families = sorted({r['query_family'] for r in rows},
                      key=lambda f: digest(f'{SEED}:{f}'.encode()))
    assert len(families) == 7, 'Dataset changed; re-author a new experiment'
    held = set(families[:3])
    for row in rows:
        row['split'] = 'holdout' if row['query_family'] in held else 'dev'
    validate_partition(rows)
    return families


def validate_partition(rows):
    family_splits, paths = collections.defaultdict(set), collections.defaultdict(set)
    for row in rows:
        family_splits[row['query_family']].add(row['split'])
        for group in row['answers']:
            for alt in group['alternatives']:
                paths[row['split']].add(alt['path'])
    if any(len(s) != 1 for s in family_splits.values()):
        raise ValueError('Query-family leakage')
    if paths['dev'] & paths['holdout']:
        raise ValueError('Gold-module leakage')
    if collections.Counter(r['split'] for r in rows) != {'dev': 8, 'holdout': 6}:
        raise ValueError('Fixed quota violated')


def invoke(args, log):
    # Binary is built without semantic feature; no external provider is invoked.
    env = {k: v for k, v in os.environ.items()
           if not k.startswith(('CODECORTEX_', 'OPENAI_', 'ANTHROPIC_', 'OCE_'))}
    with log.open('wb') as out:
        result = subprocess.run(args, cwd=REPO, env=env, stdout=out, stderr=subprocess.STDOUT)
    return result.returncode


def prepare():
    if (HERE / 'plan.json').exists():
        raise RuntimeError('Frozen plan exists; use a new directory to author another experiment')
    source = HERE / 'source'
    source.mkdir()
    manifest = json.loads(git_bytes('crates/cc-eval/benchmarks/manifests/p0-codecortex-subset.json'))
    original = git_bytes('crates/cc-eval/benchmarks/native/p0-codecortex-subset.jsonl')
    (HERE / 'original-gold.jsonl').write_bytes(original)
    rows = [json.loads(line) for line in original.splitlines()]
    families = split(rows)
    records = {}
    for path in manifest['source']['files']:
        data = git_bytes('crates/' + path)
        target = source / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)
        records[path] = {'sha256': digest(data), 'bytes': len(data)}
    source_matches = []
    for row in rows:
        old = row['annotations']['source_digests_sha256']
        source_matches.append({'id': row['id'], 'original': old,
                               'current': {p: records[p]['sha256'] for p in old},
                               'unchanged': all(records[p]['sha256'] == h for p, h in old.items())})
    queries = HERE / 'queries'
    suites = HERE / 'suites'
    queries.mkdir()
    suites.mkdir()
    for profile in ('native', 'compat'):
        converted = json.loads(json.dumps(rows))
        if profile == 'compat':
            for row in converted:
                row['expected_files'] = list(dict.fromkeys(a['path'] for g in row['answers']
                                                          for a in g['alternatives']))
                row['answers'] = []
        (queries / f'{profile}.jsonl').write_text(''.join(json.dumps(r, sort_keys=True) + '\n' for r in converted))
        for arm, config in ARMS.items():
            suite = dict(manifest)
            suite.update(name=f'P7 V19 retrospective {profile}/{arm}', queries=f'../queries/{profile}.jsonl',
                         scoring='codecortex-native-v1' if profile == 'native' else 'oce-compat-v1',
                         queries_digest='0' * 64, seed=SEED, repetitions=3, warmup=0,
                         engine_config={'auto_index': {'enabled': False}, **config})
            suite['source'] = {'root': '../source', 'commit': None, 'digest': '0' * 64,
                               'files': manifest['source']['files']}
            path = suites / f'{profile}-{arm}.json'
            write(path, suite)
            code = invoke([str(REPO / 'target/debug/cc-eval'), 'freeze', '--suite', str(path)],
                          HERE / f'freeze-{profile}-{arm}.log')
            if code:
                raise RuntimeError(f'Authoring freeze failed: {path} ({code})')
    write(HERE / 'plan.json', {
        'integration_sha': INTEGRATION, 'input_sha': BASE, 'seed': SEED,
        'family_order_sha256_seeded': families, 'quota_families': {'dev': 4, 'holdout': 3},
        'quota_queries': {'dev': 8, 'holdout': 6}, 'source_files': records,
        'original_gold_sha256': digest(original), 'gold_source_audit': source_matches,
        'arms': ARMS, 'scoring': 'Official cc-eval only; compat is derived path gold, not upstream OCE',
        'holdout_kind': 'Retrospective family/module partition of already-used dev gold; NOT unseen D5',
        'measurement_profile': 'smoke', 'repetitions': 3, 'warmup': 0, 'top_k': 10, 'timeout_ms': 30000,
        'factor_semantics': {'graph_vote_off': 'Only search.graph_weight=0; graph traversal still executes',
                             'graph_rerank_off': 'Only ranking.graph_rerank_weight=0; traversal still executes',
                             'lexical_votes_only': 'Composite vote/rerank suppression; exact/path/grep/graph lanes and selector still execute',
                             'rg_literal': 'Official fixed-string whole-query rg baseline; not BM25'},
        'blocked': ['Unseen D5 holdout', '6-repo/600-query D2 corpus', 'Upstream 200-query OCE compat',
                    'Independently reviewed hard negatives and facet/span gold',
                    'Real dense-only/local+dense and semantic quality (D1/D2 authorization unchanged)',
                    'Selector-off: no public config switch, production edits forbidden',
                    'L4 performance certification: smoke sample count only'],
    })
    locks = {str(p.relative_to(HERE)): digest(p.read_bytes())
             for p in sorted(HERE.rglob('*')) if p.is_file() and p.name != 'locks.json'
             and '__pycache__' not in str(p)}
    write(HERE / 'locks.json', locks)


def verify():
    for rel, expected in json.loads((HERE / 'locks.json').read_text()).items():
        if digest((HERE / rel).read_bytes()) != expected:
            raise ValueError(f'Frozen input drift: {rel}')
    rows = [json.loads(l) for l in (HERE / 'queries/native.jsonl').read_text().splitlines()]
    validate_partition(rows)


def run(output):
    verify()
    output.mkdir()  # Never overwrite prior raw runs.
    binary, evaluator = REPO / 'target/debug/codecortex', REPO / 'target/debug/cc-eval'
    write(output / 'build-receipt.json', {
        'workspace_head': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=REPO, text=True).strip(),
        'engine_crates_tree': subprocess.check_output(['git', 'rev-parse', 'HEAD:crates'], cwd=REPO, text=True).strip(),
        'source_baseline_crates_tree': subprocess.check_output(['git', 'rev-parse', f'{BASE}:crates'], cwd=REPO, text=True).strip(),
        'command': 'cargo build -p cc-server --bin codecortex -p cc-eval --bin cc-eval --locked --offline',
        'features': 'default; no semantic feature', 'profile': 'dev', 'jobs': 5,
        'binaries': {str(p.relative_to(REPO)): digest(p.read_bytes()) for p in (binary, evaluator)},
        'rustc': subprocess.check_output(['/workspace/.cargo/bin/rustc', '-Vv'], text=True),
        'cargo': subprocess.check_output(['/workspace/.cargo/bin/cargo', '-V'], text=True).strip(),
        'rg': subprocess.check_output(['rg', '--version'], text=True).splitlines()[0],
        'external_provider_calls': 0, 'fake_model_vectors': False,
    })
    for suite in sorted((HERE / 'suites').glob('*.json')):
        arm = suite.stem
        backend = 'rg' if arm.endswith('rg_literal') else 'mcp-stdio'
        args = [str(evaluator), 'run', '--suite', str(suite), '--backend', backend,
                '--output', str(output / arm), '--profile', 'smoke']
        if backend == 'mcp-stdio':
            args += ['--binary', str(binary)]
        code = invoke(args, output / f'{arm}.log')
        write(output / f'{arm}-exit.json', {'exit_code': code, 'command': args})
        print(arm, 'exit', code, flush=True)
        if code not in (0, 1):
            raise RuntimeError(f'Invalid measurement retained at {output / arm}: {code}')
        replay = invoke([str(evaluator), 'replay', '--run', str(output / arm)],
                        output / f'{arm}-replay.log')
        if replay not in (0, 1):
            raise RuntimeError(f'Replay failure {arm}: {replay}')
    report(output)


def interval(values):
    if not values:
        return None
    rng = random.Random(SEED)
    draws = sorted(statistics.mean(rng.choices(values, k=len(values))) for _ in range(2000))
    return {'low': draws[49], 'high': draws[1949], 'family_n': len(values),
            'draws': 2000, 'seed': SEED, 'method': 'family-cluster percentile bootstrap of family mean; repetitions averaged first',
            'conclusive': len(values) >= 10}


def summarize(cases):
    families, categories = collections.defaultdict(list), collections.defaultdict(list)
    for case in cases:
        families[case['family']].append(case['ndcg10'])
        categories[case['category']].append(case['ndcg10'])
    family_means = [statistics.mean(v) for v in families.values()]
    return {'query_n': len(cases), 'repo_n': 1,
            'micro_ndcg10': statistics.mean(c['ndcg10'] for c in cases),
            'micro_top1': statistics.mean(c['top1'] for c in cases),
            'macro_repo_ndcg10': statistics.mean(c['ndcg10'] for c in cases),
            'macro_category_ndcg10': statistics.mean(statistics.mean(v) for v in categories.values()),
            'category_ndcg10': {k: statistics.mean(v) for k, v in categories.items()},
            'macro_family_ndcg10': statistics.mean(family_means), 'family_ci': interval(family_means)}


def report(output):
    verify()
    rows = [json.loads(l) for l in (HERE / 'queries/native.jsonl').read_text().splitlines()]
    partitions = {name: {r['id'] for r in rows if name == 'all' or r['split'] == name}
                  for name in ('all', 'dev', 'holdout')}
    summaries, comparisons = {}, {}
    for profile in ('native', 'compat'):
        scores = {}
        for arm in ARMS:
            directory = output / f'{profile}-{arm}'
            metrics = json.loads((directory / 'metrics.json').read_text())
            normalized = [json.loads(l) for l in (directory / 'normalized.jsonl').read_text().splitlines()]
            scores[arm] = metrics['cases']
            summaries[f'{profile}/{arm}'] = {
                'gate': json.loads((directory / 'gate.json').read_text()),
                'invalid_hits': metrics['invalid_hits'], 'unverified_hits': metrics['unverified_hits'],
                'partitions': {name: {**summarize([c for c in metrics['cases'] if c['id'] in ids]),
                                      'row_statuses': dict(collections.Counter(r['status'] for r in normalized if r['case_id'] in ids))}
                               for name, ids in partitions.items()}}
        for arm in ARMS:
            if arm == 'baseline':
                continue
            baseline = {c['id']: c for c in scores['baseline']}
            deltas = [{**c, 'ndcg10': c['ndcg10'] - baseline[c['id']]['ndcg10'],
                       'top1': c['top1'] - baseline[c['id']]['top1']} for c in scores[arm]]
            comparisons[f'{profile}/{arm}-minus-baseline'] = {
                name: summarize([c for c in deltas if c['id'] in ids]) for name, ids in partitions.items()}
    write(output / 'aggregate.json', {'status': 'offline_mechanism_observations_only; V19 remains blocked',
                                     'summaries': summaries, 'paired_deltas': comparisons,
                                     'quality_independent_query_n': 14, 'measured_repeated_rows': 420,
                                     'cross_profile_pooling': False, 'blocked': json.loads((HERE / 'plan.json').read_text())['blocked']})
    lines = ['# P7 V19 offline retrospective experiment', '',
             'V19 remains **blocked**. These are real local MCP and rg observations on historical development gold; no real-model quality claim.', '',
             f'Inputs: integration `{INTEGRATION}` + PR14 `{BASE}`. Seed `{SEED}`, 9 source files, 14 queries / 7 families, 8 dev / 6 retrospective holdout queries. Three repetitions, 10 cells, 420 rows. Repetitions are not independent quality samples.', '',
             'Compat is derived file-only gold, not upstream OCE. Gold answers and original annotations are unchanged in native; only split is re-authored. Gold-module sets are disjoint, but all questions were previously development questions. Incidental distractors are not certified hard negatives.', '',
             '| Profile/arm | All nDCG | Holdout nDCG | Holdout Top1 | Status counts (all) |',
             '|---|---:|---:|---:|---|']
    for key, value in summaries.items():
        all_, held = value['partitions']['all'], value['partitions']['holdout']
        lines.append(f"| {key} | {all_['micro_ndcg10']:.6f} | {held['micro_ndcg10']:.6f} | {held['micro_top1']:.6f} | {all_['row_statuses']} |")
    lines += ['', 'Official cc-eval scores and gates are retained for every cell, including Partial/failed cases. aggregate.json contains micro, macro repo/category/family, strata, and paired family bootstrap CIs. Only 3 holdout families: intervals are descriptive and marked inconclusive.', '',
              'graph_vote_off and graph_rerank_off each change one public scoring knob; traversal still executes. lexical_votes_only suppresses multiple votes and graph reranking, while selector, overlap, exact bonuses and physical lanes remain. rg is the official literal whole-query baseline. This does not implement dense-only or selector-off.', '',
              '## Remaining blockers', '', *['- ' + x for x in json.loads((HERE / 'plan.json').read_text())['blocked']], '',
              '## Reproduce', '', 'From repository root, set CARGO_HOME=/workspace/.cargo, RUSTUP_HOME=/workspace/.rustup and PATH=/workspace/.cargo/bin:$PATH. Build with the receipt command. Run `python3 artifacts/benchmarks/p7-v19-offline-20261002/experiment.py verify`, then `.../experiment.py run --output /tmp/p7-v19-new-run` (new directory only). Report regeneration: `.../experiment.py report --output <run>`. No freeze occurs during run. Each cell is replayed by official cc-eval with raw digest verification.', '']
    (output / 'REPORT.md').write_text('\n'.join(lines))


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('action', choices=('prepare', 'verify', 'run', 'report'))
    parser.add_argument('--output', type=Path)
    args = parser.parse_args()
    if args.action == 'prepare':
        prepare()
    elif args.action == 'verify':
        verify()
    elif args.action == 'run':
        run(args.output.resolve())
    else:
        report(args.output.resolve())
