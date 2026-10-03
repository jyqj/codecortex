"""Only replay archived PR126 logs; never imports or executes a historical driver."""
import gzip
import hashlib
import json
from pathlib import Path

HEAD = 'da8b618cbf8b67683c6b16880419089f7159c15e'
SOURCES = {'baseline': '513a98c9a94b15ec77153df41af26fa3c8c0b5e8',
           'candidate': 'e3c04fed903c4d0e3c26b5d7cf0e055a6f7c5207'}


def analyze(root):
    root = Path(root)
    result = {'evidence_head': HEAD, 'sources': SOURCES, 'inputs_sha256': {}, 'runs': {}}
    def read(relative, compressed=False):
        path = root / relative
        result['inputs_sha256'][relative] = hashlib.sha256(path.read_bytes()).hexdigest()
        opener = gzip.open if compressed else open
        with opener(path, 'rt') as f:
            return json.load(f)
    for variant in SOURCES:
        rows_path = f'{variant}/live/n100000/resources.jsonl.gz'
        path = root / rows_path
        result['inputs_sha256'][rows_path] = hashlib.sha256(path.read_bytes()).hexdigest()
        phases = {}
        keys = set()
        root_peak = None
        with gzip.open(path, 'rt') as f:
            for line in f:
                row = json.loads(line)
                keys.update(row)
                current = row['cgroup_memory_current']
                phase = phases.setdefault(row['phase'], {
                    'first_sample_time_ns': row['time_ns'],
                    'first_sample_cgroup_bytes': current, 'sampled_max_cgroup_bytes': current,
                    'exact_phase_start_ns': None, 'phase_start_composition': None, 'samples': 0})
                phase['samples'] += 1
                phase['sampled_max_cgroup_bytes'] = max(phase['sampled_max_cgroup_bytes'], current)
                if row['time_ns'] < phase['first_sample_time_ns']:
                    phase['first_sample_time_ns'] = row['time_ns']
                    phase['first_sample_cgroup_bytes'] = current
                rss = (row.get('product') or {}).get('rss_bytes')
                if rss is not None:
                    root_peak = max(root_peak or rss, rss)
        build_path = f'{variant}/build-resources.jsonl.gz'
        path = root / build_path
        result['inputs_sha256'][build_path] = hashlib.sha256(path.read_bytes()).hexdigest()
        build_keys = set()
        first, last = None, None
        with gzip.open(path, 'rt') as f:
            for line in f:
                row = json.loads(line)
                build_keys.update(row)
                first = first or row
                last = row
        environment = read(f'{variant}/environment.json')
        summary = read(f'{variant}/summary.json.gz', True)
        result['runs'][variant] = {
            'runtime_top_level_keys': sorted(keys), 'build_top_level_keys': sorted(build_keys),
            'environment_keys': sorted(environment), 'phases': phases,
            'historical_composition_bytes': {'anon': None, 'file': None, 'slab': None},
            'historical_group_path_inode_and_members': None,
            'build_first_sample': first, 'build_last_sample': last,
            'product_root_sampled_rss_peak_bytes': root_peak,
            'guard_stop': summary.get('guard_stop'), 'status': summary['status'],
            'drain_start_ns': summary['drain_start_ns'],
            'cleanup_tail_elapsed_drain_seconds': summary['cleanup_tail_db']['elapsed_drain_seconds'],
        }
    baseline, candidate = (result['runs'][v] for v in SOURCES)
    result['observed_candidate_minus_baseline_bytes'] = {
        'startup_first_sample_cgroup': candidate['phases']['startup']['first_sample_cgroup_bytes'] - baseline['phases']['startup']['first_sample_cgroup_bytes'],
        'startup_sampled_max_cgroup': candidate['phases']['startup']['sampled_max_cgroup_bytes'] - baseline['phases']['startup']['sampled_max_cgroup_bytes'],
        'product_root_sampled_rss_peak': candidate['product_root_sampled_rss_peak_bytes'] - baseline['product_root_sampled_rss_peak_bytes'],
    }
    result['original_timeline'] = read('event-timeline.json')
    result['limits'] = [
        'Missing memory.stat and phase-start samples: anon/file/slab and exact boundary composition unknown.',
        'memory.current measures shared cgroup including build/cache/harness; not product RSS or proof of leak.',
        'Single baseline-first pair without cache isolation: differences are observations, causal order effect unknown.',
        'Product root RSS is non-atomic sampled RSS; complete tree/PSS/lifetime peak unknown.',
        'Both original ready gates failed. Candidate guard termination does not become ready or full-window throughput.',
        'Guard signal timestamp not logged; threshold samples/HTTP timestamps are only observed bounds.',
    ]
    return result


if __name__ == '__main__':
    import argparse
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('evidence_root', type=Path)
    args = parser.parse_args()
    print(json.dumps(analyze(args.evidence_root), ensure_ascii=False, indent=2))
