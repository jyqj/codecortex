#!/usr/bin/env python3
"""Read-only manifest validation plus lock negatives in isolated synthetic copies."""
import argparse
import copy
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tempfile


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--binary', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[3]
    binary = args.binary.resolve()
    suite_dir = root / 'crates/cc-eval/benchmarks/manifests'
    suites = [suite_dir / (name + '.json') for name in (
        'p0-smoke', 'p0-codecortex-subset', 'p1b-exact',
        'p1c-intents', 'p1c-bm25', 'p1c-softscope')]
    result = subprocess.run([str(binary), 'validate'] +
                            [arg for p in suites for arg in ('--suite', str(p))],
                            capture_output=True, text=True, check=False)
    assert result.returncode == 0, result.stderr
    assert result.stdout.count('locks valid') == 6, result.stdout
    observations = [{'case': 'all-six-valid', 'exit_code': result.returncode,
                     'stdout': result.stdout, 'stderr': result.stderr}]
    original = json.loads(suites[1].read_text())
    source_root = (suite_dir / original['source']['root']).resolve()
    query_bytes = (suite_dir / original['queries']).read_bytes()
    before = {str(p.relative_to(root)): sha256(p.read_bytes()) for p in suites}
    before['queries'] = sha256(query_bytes)
    for fault in ('valid-copy', 'source-byte-drift', 'query-text-drift',
                  'gold-answer-drift', 'old-source-lock', 'old-queries-lock'):
        with tempfile.TemporaryDirectory(prefix='p0-lock-negative-') as tmp:
            tmp = Path(tmp)
            for path in original['source']['files']:
                target = tmp / 'source' / path
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(source_root / path, target)
            query_path = tmp / 'queries.jsonl'
            query_path.write_bytes(query_bytes)
            suite = copy.deepcopy(original)
            suite['source']['root'] = 'source'
            suite['queries'] = 'queries.jsonl'
            if fault == 'source-byte-drift':
                path = tmp / 'source' / original['source']['files'][0]
                path.write_bytes(path.read_bytes() + b'\n// synthetic lock negative\n')
            elif fault in ('query-text-drift', 'gold-answer-drift'):
                rows = [json.loads(line) for line in query_bytes.decode().splitlines()]
                if fault == 'query-text-drift':
                    rows[0]['query'] += ' synthetic lock negative'
                else:
                    rows[0]['answers'][0]['grade'] = 1
                query_path.write_text('\n'.join(json.dumps(row, separators=(',', ':'))
                                                for row in rows) + '\n')
            elif fault == 'old-source-lock':
                suite['source']['digest'] = '47ac4cea726aee72f3ab0a70b53437fc24208786c125059fb0c1c97df8e4af23'
            elif fault == 'old-queries-lock':
                suite['queries_digest'] = '0' * 64
            manifest = tmp / 'suite.json'
            manifest.write_text(json.dumps(suite, indent=2) + '\n')
            manifest_before = sha256(manifest.read_bytes())
            query_before = sha256(query_path.read_bytes())
            run = subprocess.run([str(binary), 'validate', '--suite', str(manifest)],
                                 capture_output=True, text=True, check=False)
            expected = 0 if fault == 'valid-copy' else 2
            assert run.returncode == expected, (fault, run.returncode, run.stdout, run.stderr)
            if expected:
                assert 'source or query content lock drift' in run.stderr, (fault, run.stderr)
            assert sha256(manifest.read_bytes()) == manifest_before, 'validate rewrote a manifest'
            assert sha256(query_path.read_bytes()) == query_before, 'validate rewrote gold/query'
            observations.append({'case': fault, 'exit_code': run.returncode,
                                 'stdout': run.stdout, 'stderr': run.stderr,
                                 'validator_mutated_input': False})
    after = {str(p.relative_to(root)): sha256(p.read_bytes()) for p in suites}
    after['queries'] = sha256((suite_dir / original['queries']).read_bytes())
    assert before == after, 'regression altered repository inputs'
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps({'schema_version': 1, 'binary_sha256': sha256(binary.read_bytes()),
                                     'observations': observations, 'repository_inputs_unchanged': True},
                                    indent=2) + '\n')
    print('6 suite validations; valid isolated copy; 5 rejected lock negatives; inputs unchanged')


if __name__ == '__main__':
    main()
