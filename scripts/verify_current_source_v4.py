#!/usr/bin/env python3
"""Reconstruct v3 plus the fixed public, accepted member-ranking source delta.

Explicit source integrity only. Previous registries, evidence and CI stay frozen;
the current checkout, HEAD and latest never authorize new source or quality.
"""
import argparse
import json
from pathlib import Path
import stat
import subprocess

# The inherited historical guards intentionally use assertions. Never run them
# with Python optimization, which would silently disable their checks.
if not __debug__:
    raise RuntimeError('source integrity requires Python assertions; omit -O')

import verify_current_source_v3 as v3

v1 = v3.v2.v1
ROOT = Path(__file__).resolve().parents[1]
VERSION = 'contextual-member-ranking-20261005-v4'
REGISTRY = ROOT / 'scripts/current-source-registry-v4.json'
REGISTRY_SHA256 = '9bb97df425de4370d1e0d5f1ffcf7de51fe0239829a54cb1ef155aa0846c6ca0'
BASE = '886f90a542a6174a037c79eebbb4f74848fb1f53'
SOURCE = 'ef56a468ae4ee85be172db673c39e32c4f3c06a2'
SOURCE_TREE = 'c699d92d1845c28b25e58e5a731c7eb7876a37ff'
DELTA_PATHS = {
    'crates/cc-model/src/config.rs',
    'crates/cc-search/src/engine_cache.rs',
    'crates/cc-search/src/engine_lane_tests.rs',
    'crates/cc-search/src/plan.rs',
    'crates/cc-search/src/query_target.rs',
}
PUBLIC_CONCLUSION = 'artifacts/checkpoints/contextual-member-ranking-public-20261004/README.md'
PRESERVED_PATHS = {
    '.github/workflows/ci.yml',
    'scripts/current-source-registry-v1.json',
    'scripts/current-source-registry-v2.json',
    'scripts/current-source-registry-v3.json',
    'scripts/verify_current_source.py',
    'scripts/verify_current_source_v2.py',
    'scripts/verify_current_source_v3.py',
    'scripts/verify_fixed_e3_integration.py',
    'scripts/verify_packing_integration.py',
    'scripts/p0_historical_corpus.py',
    'tests/source_integrity/test_current_source.py',
    'tests/source_integrity/test_current_source_v2.py',
    'tests/source_integrity/test_current_source_v3.py',
    PUBLIC_CONCLUSION,
}


def identities():
    return {
        'schema_version': 4, 'source_version': VERSION,
        'approved_base': BASE, 'previous_source_version': v3.VERSION,
        'previous_registry_sha256': v3.REGISTRY_SHA256,
        'accepted_public_source': SOURCE, 'accepted_public_tree': SOURCE_TREE,
        'scope': 'source_integrity_only', 'quality_and_100k': 'not_inherited',
    }


def verify_identities(registry):
    for key, value in identities().items():
        assert registry[key] == value, 'wrong v4 identity: ' + key


def load_registry(path=REGISTRY):
    raw = path.read_bytes()
    assert v1.sha(raw) == REGISTRY_SHA256, 'stale or altered v4 registry'
    registry = json.loads(raw)
    verify_identities(registry)
    return registry


def require_regular_path(root, path):
    target = root / path
    for part in [target, *target.parents]:
        if part == root:
            break
        assert not part.is_symlink(), 'symlink input or directory: ' + path
    assert stat.S_ISREG(target.stat().st_mode), 'nonregular current input: ' + path
    return target


def verify_preserved_files(registry, root=ROOT):
    assert set(registry['preserved_files']) == PRESERVED_PATHS, 'preserved inventory differs'
    for path, row in registry['preserved_files'].items():
        ref = SOURCE if path == PUBLIC_CONCLUSION else BASE
        assert row['commit'] == ref, 'preserved pin differs: ' + path
        raw = v1.blob(ref, path)
        assert v1.sha(raw) == row['sha256'], 'preserved SHA differs: ' + path
        assert v1.blob(SOURCE, path) == raw, 'public preserved file differs: ' + path
        target = require_regular_path(root, path)
        assert target.read_bytes() == raw, 'preserved file changed: ' + path


def apply_member_delta(registry, previous):
    """Apply only the fixed five-file delta to an independently verified v3 union."""
    verify_identities(registry)
    assert v1.git('rev-parse', SOURCE + '^{tree}').decode().strip() == SOURCE_TREE, 'public tree differs'
    assert v1.git('show', '-s', '--format=%P', SOURCE).decode().strip() == BASE, 'public parent differs'
    assert set(previous) == v1.inputs(BASE), 'fixed base inventory differs'
    assert all(raw == v1.blob(BASE, p) for p, raw in previous.items()), 'fixed base bytes differ'
    assert set(registry['member_ranking_delta']) == DELTA_PATHS, 'member delta inventory differs'
    assert v3.v2.changed_paths(BASE, SOURCE) == DELTA_PATHS, 'public delta inventory differs'
    expected = dict(previous)
    for path, row in registry['member_ranking_delta'].items():
        assert v1.sha(expected[path]) == row['before_sha256'], 'member base SHA: ' + path
        raw = v1.blob(SOURCE, path)
        assert v1.sha(raw) == row['sha256'], 'member source SHA: ' + path
        expected[path] = raw
    assert set(expected) == v1.inputs(SOURCE), 'accepted union inventory differs'
    assert all(raw == v1.blob(SOURCE, p) for p, raw in expected.items()), 'accepted union bytes differ'
    assert {p: v1.sha(b) for p, b in expected.items()} == registry['complete_inputs'], 'complete manifest differs'
    return expected


def approved_union(registry, root=ROOT):
    verify_identities(registry)
    v1.ensure_refs([BASE, SOURCE])
    # Check historical verifier files and the fixed public conclusion before
    # invoking the historical reconstruction. Existing rejected evidence is not
    # relabelled or replaced, and unpublished records are not fetch dependencies.
    verify_preserved_files(registry, root=root)
    for path in [v1.REGISTRY, v3.v2.REGISTRY, v3.REGISTRY]:
        for record in json.loads(path.read_bytes())['records']:
            require_regular_path(root, record)
    previous = v3.approved_union(v3.load_registry(), root=root)
    return apply_member_delta(registry, previous)


def source_entries():
    entries = {}
    for row in v1.git('ls-tree', '-rz', SOURCE, '--', 'crates', 'Cargo.toml', 'Cargo.lock').split(b'\0'):
        if not row:
            continue
        info, path = row.decode().split('\t', 1)
        mode, kind, oid = info.split()
        assert kind == 'blob' and mode in {'100644', '100755'}, 'nonregular pinned input: ' + path
        entries[path] = (mode, oid)
    return entries


def index_entries(root=ROOT):
    raw = subprocess.check_output(['git', 'ls-files', '--stage', '-z', '--',
                                   'crates', 'Cargo.toml', 'Cargo.lock'], cwd=root)
    entries = {}
    for row in raw.split(b'\0'):
        if not row:
            continue
        info, path = row.decode().split('\t', 1)
        mode, oid, stage = info.split()
        assert stage == '0' and path not in entries, 'unmerged or duplicate input: ' + path
        entries[path] = (mode, oid)
    return entries


def verify_current_tree(root, expected, tracked=None):
    entries = source_entries()
    assert set(entries) == set(expected), 'pinned entry inventory differs'
    if tracked is None:
        tracked = index_entries(root)
    assert set(tracked) == set(entries), 'current tracked input inventory differs'
    assert tracked == entries, 'current index mode or blob differs'
    # Count every nondirectory entry, not just regular files. In particular an
    # unknown FIFO/socket must not disappear from the inventory or be opened.
    disk = {p.relative_to(root).as_posix() for p in (root / 'crates').rglob('*')
            if p.is_symlink() or not p.is_dir()}
    assert disk == {p for p in expected if p.startswith('crates/')}, 'current disk input inventory differs'
    for path in expected:
        target = require_regular_path(root, path)
        mode = target.stat().st_mode
        expected_executable = entries[path][0] == '100755'
        assert bool(mode & 0o111) == expected_executable, 'current file mode differs: ' + path
    v1.verify_tree(root, expected, tracked)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source-version', required=True, choices=[VERSION])
    parser.parse_args()
    registry = load_registry()
    expected = approved_union(registry)
    verify_current_tree(ROOT, expected)
    print(json.dumps(dict(identities(), status='passed', complete_inputs=len(expected),
                          preserved_files=len(registry['preserved_files']), registry_sha256=REGISTRY_SHA256)))


if __name__ == '__main__':
    main()
