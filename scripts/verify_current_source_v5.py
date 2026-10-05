#!/usr/bin/env python3
"""Reconstruct v4 plus the immutable public C++ namespace identity delta.

Explicit source integrity only. Historical verifiers, evidence and CI remain
frozen. Neither the current checkout nor HEAD/latest authorizes new inputs.
"""
import argparse
import json
from pathlib import Path
import subprocess

if not __debug__:
    raise RuntimeError('source integrity requires Python assertions; omit -O')

import verify_current_source_v4 as v4

v1 = v4.v1
ROOT = Path(__file__).resolve().parents[1]
VERSION = 'cpp-namespace-identity-20261005-v5'
REGISTRY = ROOT / 'scripts/current-source-registry-v5.json'
REGISTRY_SHA256 = '027d882515bd9c33fce6c650942bc3154751d73d6e3c54952c423e9694dc7f01'
BASE = '1592c7c02ebe88c36ab396b9f705d0541b425383'
BASE_TREE = '905457aee94d7d89e762d02a1deefc8f245d476d'
SOURCE = '614da81becf3769ba7f7ccd4eaa07edf1ae879b9'
SOURCE_TREE = '9ca8682bebbabb9cef25affd2ee468d982bdc638'
DELTA_PATHS = {
    'crates/cc-db/src/index_migrate.rs',
    'crates/cc-index/src/dirty_reload_policy.rs',
    'crates/cc-index/src/project_model/mod.rs',
    'crates/cc-index/src/resolver/resolve_outcome.rs',
    'crates/cc-index/tests/cpp_namespace_identity_lifecycle.rs',
    'crates/cc-model/src/resolution.rs',
    'crates/cc-parsers/src/c_cpp.rs',
    'crates/cc-parsers/tests/c_cpp_declarator_boundaries.rs',
    'crates/cc-parsers/tests/cpp_namespace_functions.rs',
    'crates/cc-parsers/tests/fixtures/cpp_namespace_baseline.json',
    'crates/cc-server/tests/qname_identity_lifecycle.rs',
}
PUBLIC_CONCLUSION = 'docs/reviews/cpp-namespace-function-public-20261005/README.md'
CAPABILITIES = 'docs/internals/MODULE_CAPABILITIES.json'
PUBLIC_DOCUMENTS = {
    PUBLIC_CONCLUSION, CAPABILITIES,
    'docs/roadmap/code-index-v2/05-TODO.md',
    'docs/roadmap/code-index-v2/tasks.json',
}
DELIVERY_PATHS = DELTA_PATHS | PUBLIC_DOCUMENTS
PRESERVED_PATHS = v4.PRESERVED_PATHS | PUBLIC_DOCUMENTS | {
    'scripts/verify_current_source_v4.py',
    'scripts/current-source-registry-v4.json',
    'tests/source_integrity/test_current_source_v4.py',
    'docs/checkpoints/2026-10-05-member-source-v4/README.md',
}


def identities():
    return {
        'schema_version': 5, 'source_version': VERSION,
        'approved_base': BASE, 'approved_base_tree': BASE_TREE,
        'previous_source_version': v4.VERSION,
        'previous_registry_sha256': v4.REGISTRY_SHA256,
        'accepted_public_source': SOURCE, 'accepted_public_tree': SOURCE_TREE,
        'database_schema': 26, 'project_model_version': 3,
        'scope': 'source_integrity_only', 'quality_and_100k': 'not_inherited',
    }


def verify_identities(registry):
    for key, value in identities().items():
        assert registry[key] == value, 'wrong v5 identity: ' + key


def load_registry(path=REGISTRY):
    raw = v4.require_regular_path(path.parent, path.name).read_bytes()
    assert v1.sha(raw) == REGISTRY_SHA256, 'stale or altered v5 registry'
    registry = json.loads(raw)
    verify_identities(registry)
    return registry


def tree_entries(ref, *paths):
    entries = {}
    for row in v1.git('ls-tree', '-rz', ref, '--', *paths).split(b'\0'):
        if not row:
            continue
        info, path = row.decode().split('\t', 1)
        mode, kind, oid = info.split()
        assert kind == 'blob' and mode in {'100644', '100755'}, 'nonregular pinned input: ' + path
        entries[path] = (mode, oid)
    return entries


def source_entries():
    return tree_entries(SOURCE, 'crates', 'Cargo.toml', 'Cargo.lock')


def verify_public_delta(registry):
    verify_identities(registry)
    for ref, tree in [(BASE, BASE_TREE), (SOURCE, SOURCE_TREE)]:
        assert v1.git('rev-parse', ref + '^{tree}').decode().strip() == tree, 'fixed tree differs'
    assert v1.git('show', '-s', '--format=%P', SOURCE).decode().strip() == BASE, 'public parent differs'
    # --no-renames makes the inventory independent of local rename heuristics.
    paths = set(v1.git('diff', '--no-renames', '--name-only', BASE, SOURCE).decode().splitlines())
    assert paths == DELIVERY_PATHS, 'public delivery inventory differs'
    assert set(registry['public_delta']) == DELIVERY_PATHS, 'registered delivery inventory differs'
    before_entries = tree_entries(BASE, *sorted(DELIVERY_PATHS))
    after_entries = tree_entries(SOURCE, *sorted(DELIVERY_PATHS))
    assert set(after_entries) == DELIVERY_PATHS, 'public delivery entries differ'
    for path, row in registry['public_delta'].items():
        before = v1.blob(BASE, path) if path in before_entries else None
        assert row['before_sha256'] == (v1.sha(before) if before is not None else None), 'public before SHA differs: ' + path
        assert row['before_mode'] == (before_entries[path][0] if path in before_entries else None), 'public before mode differs: ' + path
        assert row['sha256'] == v1.sha(v1.blob(SOURCE, path)), 'public source SHA differs: ' + path
        assert row['mode'] == after_entries[path][0], 'public source mode differs: ' + path
    capabilities = json.loads(v1.blob(SOURCE, CAPABILITIES))
    assert capabilities['database_schema'] == 26, 'public database schema differs'
    assert capabilities['project_model_version'] == 3, 'public project model differs'


def preserved_entries():
    return tree_entries(SOURCE, *sorted(PRESERVED_PATHS))


def preserved_index_entries(root):
    raw = subprocess.check_output(['git', 'ls-files', '--stage', '-z', '--',
                                   *sorted(PRESERVED_PATHS)], cwd=root)
    entries = {}
    for row in raw.split(b'\0'):
        if not row:
            continue
        info, path = row.decode().split('\t', 1)
        mode, oid, stage = info.split()
        assert stage == '0' and path not in entries, 'unmerged or duplicate preserved input: ' + path
        entries[path] = (mode, oid)
    return entries


def verify_preserved_files(registry, root=ROOT, tracked=None):
    assert set(registry['preserved_files']) == PRESERVED_PATHS, 'preserved inventory differs'
    entries = preserved_entries()
    assert set(entries) == PRESERVED_PATHS, 'pinned preserved inventory differs'
    if tracked is None:
        tracked = preserved_index_entries(root)
    assert tracked == entries, 'preserved index inventory, mode or blob differs'
    for path, row in registry['preserved_files'].items():
        ref = SOURCE if path in PUBLIC_DOCUMENTS else BASE
        assert row['commit'] == ref, 'preserved pin differs: ' + path
        raw = v1.blob(ref, path)
        assert v1.sha(raw) == row['sha256'], 'preserved SHA differs: ' + path
        assert v1.blob(SOURCE, path) == raw, 'public preserved file differs: ' + path
        assert row['mode'] == entries[path][0], 'preserved mode differs: ' + path
        target = v4.require_regular_path(root, path)
        assert bool(target.stat().st_mode & 0o111) == (entries[path][0] == '100755'), 'preserved file mode differs: ' + path
        assert target.read_bytes() == raw, 'preserved file changed: ' + path


def apply_cpp_delta(registry, previous):
    """Apply exactly eleven fixed public inputs to the unchanged v4 union."""
    verify_public_delta(registry)
    before_entries = tree_entries(BASE, 'crates', 'Cargo.toml', 'Cargo.lock')
    assert before_entries == v4.source_entries(), 'fixed v4 base modes or blobs differ'
    assert set(previous) == set(before_entries), 'fixed base inventory differs'
    assert all(raw == v1.blob(BASE, p) for p, raw in previous.items()), 'fixed base bytes differ'
    assert v4.v3.v2.changed_paths(BASE, SOURCE) == DELTA_PATHS, 'public source delta inventory differs'
    expected = dict(previous)
    for path in sorted(DELTA_PATHS):
        row = registry['public_delta'][path]
        before = expected.get(path)
        assert (v1.sha(before) if before is not None else None) == row['before_sha256'], 'cpp base SHA differs: ' + path
        expected[path] = v1.blob(SOURCE, path)
    entries = source_entries()
    assert set(expected) == set(entries), 'accepted union inventory differs'
    assert all(raw == v1.blob(SOURCE, p) for p, raw in expected.items()), 'accepted union bytes differ'
    assert {p: v1.sha(b) for p, b in expected.items()} == registry['complete_inputs'], 'complete manifest differs'
    assert {p: row[0] for p, row in entries.items()} == registry['complete_modes'], 'complete mode manifest differs'
    return expected


def approved_union(registry, root=ROOT):
    verify_identities(registry)
    v1.ensure_refs([BASE, SOURCE])
    # Verify unchanged executable guards and public evidence before invoking the
    # historical reconstruction. No new private acceptance object is fetched.
    verify_preserved_files(registry, root=root)
    previous = v4.approved_union(v4.load_registry(), root=root)
    return apply_cpp_delta(registry, previous)


def verify_current_tree(root, expected, tracked=None):
    entries = source_entries()
    assert set(entries) == set(expected), 'pinned entry inventory differs'
    if tracked is None:
        tracked = v4.index_entries(root)
    assert set(tracked) == set(entries), 'current tracked input inventory differs'
    assert tracked == entries, 'current index mode or blob differs'
    # Preserve the corrected v4 inventory: do not hide or open special files.
    disk = {p.relative_to(root).as_posix() for p in (root / 'crates').rglob('*')
            if p.is_symlink() or not p.is_dir()}
    assert disk == {p for p in expected if p.startswith('crates/')}, 'current disk input inventory differs'
    for path in expected:
        target = v4.require_regular_path(root, path)
        assert bool(target.stat().st_mode & 0o111) == (entries[path][0] == '100755'), 'current file mode differs: ' + path
    v1.verify_tree(root, expected, tracked)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source-version', required=True, choices=[VERSION])
    parser.parse_args()
    registry = load_registry()
    expected = approved_union(registry)
    verify_current_tree(ROOT, expected)
    print(json.dumps(dict(identities(), status='passed', complete_inputs=len(expected),
                          public_delta_paths=len(DELIVERY_PATHS),
                          preserved_files=len(PRESERVED_PATHS), registry_sha256=REGISTRY_SHA256)))


if __name__ == '__main__':
    main()
