#!/usr/bin/env python3
"""Paired query comparisons, then repo-macro summaries; standard library only."""
from __future__ import annotations
import argparse
import json
import math
import random
from collections import defaultdict
from pathlib import Path
from statistics import mean

METRICS = ('recall_at_5', 'reciprocal_rank', 'ndcg_at_5')


def task_means(report: dict, metric: str) -> dict[tuple[str, str], float]:
    values = defaultdict(list)
    for row in report['observations']:
        if not row['no_answer']:
            # Tool/schema failures already have zero-valued metrics. Keep them.
            value = float(row['metrics'][metric])
            if not math.isfinite(value) or not 0.0 <= value <= 1.0:
                raise ValueError(f'invalid {metric} value')
            values[(row['repo'], row['task_id'])].append(value)
    return {key: mean(v) for key, v in values.items()}


def compare(base: dict, candidate: dict, seed: int = 917, draws: int = 2000) -> dict:
    for field in ('schema_version', 'manifest_git_blob', 'dataset_id', 'purpose'):
        if base[field] != candidate[field]:
            raise ValueError(f'incomparable runs: {field} differs')
    if base.get('ndcg_policy') != candidate.get('ndcg_policy'):
        raise ValueError('incomparable nDCG policies')
    if draws < 100:
        raise ValueError('bootstrap draws must be at least 100')
    results = {}
    for metric in METRICS:
        old, new = task_means(base, metric), task_means(candidate, metric)
        if not old or old.keys() != new.keys():
            raise ValueError('nonempty identical query sets are required')
        grouped = defaultdict(list)
        for key in sorted(old):
            grouped[key[0]].append(new[key] - old[key])
        repo_delta = {repo: mean(v) for repo, v in grouped.items()}
        delta = list(repo_delta.values())
        interval = None
        # Repetitions are not independent repositories. Never attach inferential
        # confidence intervals to authored regression fixtures.
        if base['purpose'] == 'held_out' and len(delta) >= 5:
            rng = random.Random(seed)
            boot = sorted(mean(rng.choices(delta, k=len(delta))) for _ in range(draws))
            interval = [boot[int(0.025 * (draws-1))], boot[int(0.975 * (draws-1))]]
        results[metric] = {
            'repo_macro_delta': mean(delta), 'per_repo_delta': repo_delta,
            'repo_cluster_bootstrap_95pct': interval,
            'paired_task_deltas': [{'repo': k[0], 'task_id': k[1], 'base': old[k],
                                    'candidate': new[k], 'delta': new[k]-old[k]} for k in sorted(old)]}
    return {'schema_version': 1, 'dataset_id': base['dataset_id'],
            'manifest_git_blob': base['manifest_git_blob'], 'purpose': base['purpose'],
            'base_commit': base['implementation_commit'], 'candidate_commit': candidate['implementation_commit'],
            'base_passed': base['passed'], 'candidate_passed': candidate['passed'],
            'base_config': base['effective_config'], 'candidate_config': candidate['effective_config'],
            'base_provenance': base.get('provenance'), 'candidate_provenance': candidate.get('provenance'),
            'seed': seed, 'bootstrap_draws': draws, 'metrics': results,
            'note': 'Descriptive only for regression fixtures. Held-out CIs require >=5 independent repositories; this is not a claim of statistical sufficiency.'}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('base', type=Path)
    parser.add_argument('candidate', type=Path)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    result = compare(json.loads(args.base.read_text()), json.loads(args.candidate.read_text()))
    args.output.write_text(json.dumps(result, indent=2, ensure_ascii=False) + '\n')
    print(json.dumps({name: value['repo_macro_delta'] for name, value in result['metrics'].items()}, indent=2))


if __name__ == '__main__':
    main()
