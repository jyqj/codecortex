#!/usr/bin/env python3
"""Read-only verification of the frozen product and imported evidence identities."""
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parent.parent
REPORT = ROOT / 'docs/checkpoints/2026-10-03-fixed-e3-integration'


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    product = json.loads((REPORT / 'production-identity.json').read_text())
    assert product['source_commit'] == 'e3c04fed903c4d0e3c26b5d7cf0e055a6f7c5207'
    expected = {x['path']: x for x in product['files']}
    actual = set(subprocess.check_output(
        ['git', 'ls-files', '--', 'crates', 'Cargo.toml', 'Cargo.lock'],
        cwd=ROOT, text=True).splitlines())
    assert actual == set(expected), 'tracked product path inventory differs'
    # Detect an extra untracked crate input as well as tracked additions/deletions.
    disk = {p.relative_to(ROOT).as_posix() for p in (ROOT / 'crates').rglob('*') if p.is_file()}
    assert disk == {p for p in expected if p.startswith('crates/')}, 'crate disk inventory differs'
    for path, row in expected.items():
        assert digest(ROOT / path) == row['sha256'], f'product bytes differ: {path}'
    imports = json.loads((REPORT / 'imported-identity.json').read_text())
    for row in imports['files']:
        assert digest(ROOT / row['path']) == row['sha256'], f'import bytes differ: {row["path"]}'
    tasks = json.loads((ROOT / 'docs/roadmap/code-index-v2/tasks.json').read_text())
    states = {t['id']: t['status'] for t in tasks['tasks']}
    for key, value in imports['preserved_task_states'].items():
        assert states[key] == value, f'overall task was prematurely closed: {key}'
    gates = json.loads((ROOT / 'docs/roadmap/code-index-v2/P7-REMAINING-GATES.json').read_text())
    assert len(gates['rows']) == 37
    assert gates['full_gate_status']['V19'] == 'open'
    assert gates['fixed_e3_local_attempt_width4_acceptance']['strict_causal_speedup'] is False
    assert gates['fixed_e3_local_attempt_width4_acceptance']['statistical_significance'] is False
    forbidden = 'artifacts/checkpoints/localwidth4-100k-paired-20261003'
    assert not (ROOT / forbidden).exists(), 'excluded historical paired directory present'
    subprocess.run(['python3', 'scripts/code_index_plan.py'], cwd=ROOT, check=True)
    print(json.dumps({'status': 'passed', 'production_source': product['source_commit'],
                      'production_paths': len(expected), 'crates_paths': product['crates_paths'],
                      'cargo_paths': product['cargo_paths'], 'imported_paths': len(imports['files']),
                      'production_identity_sha256': digest(REPORT / 'production-identity.json'),
                      'imported_identity_sha256': digest(REPORT / 'imported-identity.json'),
                      'full_P7': 'open', 'V19': 'open', 'fault_runtime_tests': 'not_run'}))


if __name__ == '__main__':
    main()
