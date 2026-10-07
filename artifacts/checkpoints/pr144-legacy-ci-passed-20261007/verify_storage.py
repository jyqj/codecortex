#!/usr/bin/env python3
"""Verify this stored CI evidence without executing products or tests."""
import argparse
import gzip
import hashlib
import json
from pathlib import Path
import tarfile

import audit_ci


ROOT = Path(__file__).resolve().parent
sha = lambda raw: hashlib.sha256(raw).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--repo', type=Path, required=True)
    args = parser.parse_args()
    index = json.loads((ROOT / 'evidence-sha256.json').read_bytes())
    assert {p.name for p in ROOT.iterdir() if p.is_file()} == set(index) | {'evidence-sha256.json'}
    for name, row in index.items():
        assert Path(name).name == name and not (ROOT / name).is_symlink()
        raw = (ROOT / name).read_bytes()
        assert len(raw) == row['bytes'] and sha(raw) == row['sha256'], name
    archived = json.loads((ROOT / 'raw-members.json').read_bytes())
    raw = (ROOT / archived['archive']).read_bytes()
    assert len(raw) == archived['archive_bytes'] and sha(raw) == archived['archive_sha256']
    payloads = {}
    with tarfile.open(ROOT / archived['archive'], 'r:gz') as tar:
        for member in tar.getmembers():
            assert member.isfile() and member.name not in payloads and member.name in archived['members']
            assert Path(member.name).name == member.name and 0 <= member.size <= 4 * 1024 * 1024
            data = tar.extractfile(member).read()
            expected = archived['members'][member.name]
            assert len(data) == expected['bytes'] and sha(data) == expected['sha256']
            payloads[member.name] = data
    assert set(payloads) == set(archived['members'])
    compressed = json.loads((ROOT / 'test-index-storage.json').read_bytes())
    raw = (ROOT / compressed['path']).read_bytes()
    assert len(raw) == compressed['compressed_bytes'] and sha(raw) == compressed['compressed_sha256']
    decoded = gzip.decompress(raw)
    assert len(decoded) == compressed['decoded_bytes'] and sha(decoded) == compressed['decoded_sha256']
    stored_tests = json.loads(decoded)
    run = json.loads(payloads['run-api-body.json'])
    jobs = json.loads(payloads['jobs-api-body.json'])
    checkout = json.loads(payloads['checkout-commit-api-body.json'])
    artifacts = json.loads(payloads['artifacts-api-body.json'])
    assert run['id'] == 37657882021 and run['run_attempt'] == 1
    assert run['status'] == 'completed' and run['conclusion'] == 'success'
    assert jobs['total_count'] == 3 and len(jobs['jobs']) == 3
    assert all(j['conclusion'] == 'success' and all(s['conclusion'] == 'success' for s in j['steps']) for j in jobs['jobs'])
    assert artifacts['total_count'] == 0
    fixed, workflow = audit_ci.source(args.repo, run['head_sha'])
    stored_source = json.loads((ROOT / 'git-source-identity.json').read_bytes())
    assert fixed['source_tree'] == checkout['tree']['sha'] == run['head_commit']['tree_id']
    assert fixed['inputs'] == stored_source['inputs'] and fixed['input_count'] == 776
    assert fixed['manifest_sha256'] == stored_source['manifest_sha256']
    check_job = next(j for j in jobs['jobs'] if j['name'] == 'check')
    actual_tests = audit_ci.parse_check(payloads['check-job.log'], check_job, workflow)
    assert actual_tests == stored_tests
    print(json.dumps({'status': 'passed', 'run_id': run['id'], 'source_commit': run['head_sha'],
                      'files': len(index), 'raw_members': len(payloads), 'source_inputs': 776,
                      'rust_target_executions': sum('summary' in g for g in actual_tests['rust_groups']),
                      'scope': 'stored evidence, actual log indexing and fixed Git source identity only'}))


if __name__ == '__main__':
    main()
