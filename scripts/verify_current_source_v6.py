#!/usr/bin/env python3
"""Reconstruct immutable v5, then the ordered public B1 and cv/ref+FTS deltas.

Explicit source integrity only. Historical guards, evidence and CI stay frozen.
The only current-document exception is the byte-exact schema 27 to 28 correction.
No HEAD/latest admission, runtime execution or quality/scale claim is implied.
"""
import argparse
from contextlib import contextmanager
import json
from pathlib import Path
import subprocess
import tempfile

if not __debug__:
    raise RuntimeError('source integrity requires Python assertions; omit -O')

import verify_current_source_v5 as v5

v4 = v5.v4
v1 = v5.v1
ROOT = Path(__file__).resolve().parents[1]
VERSION = 'cpp-b1-cvref-fts-20261006-v6'
REGISTRY = ROOT / 'scripts/current-source-registry-v6.json'
REGISTRY_SHA256 = '86664d754041fbbf28bb292fb597adcdd5e9de78444c8a2654956fe6029e078b'
V5_SOURCE = '614da81becf3769ba7f7ccd4eaa07edf1ae879b9'
V5_SOURCE_TREE = '9ca8682bebbabb9cef25affd2ee468d982bdc638'
V5_DELIVERY = '9641add263f18dc840562e46cd388afaea6796a4'
V5_DELIVERY_TREE = 'e1e655734f2161c54d35a11ef81c498cb024d197'
B1_SOURCE = '13bd9dbbe3f96b390f6adf34a95bb398da89bf4d'
B1_TREE = '5c4c2d55422e99ea2aa53b4768ffee0af06b6926'
SOURCE = 'bae3bcc21b89d722d0e676c620f418947b24dc84'
SOURCE_TREE = 'e77455661078b9728a4bc372f3384f2dc2a5ac4c'
CAPABILITIES = v5.CAPABILITIES
METADATA_BEFORE_SHA256 = '3dbb4f4d6d1093f1496c3842461dc352fb41826d4b9d88d453db92f6ea146a09'
METADATA_AFTER_SHA256 = '23314bc80894ba0be913d7c9d53a29cd30150216bf479da73b6430bed0dc3503'
METADATA_PATCH_SHA256 = '23c5ef1be3ede46e5d2426a1249aae885294fb21a9569ca889bc6749b9f545e6'
METADATA_AFTER_BLOB = 'e61ce41723ac62754dbb1d06d86b9c5b009db646'
V5_DELIVERY_PATHS = {
    'docs/checkpoints/2026-10-05-cpp-namespace-source-v5/README.md',
    'scripts/current-source-registry-v5.json',
    'scripts/verify_current_source_v5.py',
    'tests/source_integrity/test_current_source_v5.py',
}
B1_DELIVERY_PATHS = {
    'crates/cc-db/src/cpp_qualified_owner_tests.rs',
    'crates/cc-db/src/index_db_graph.rs',
    'crates/cc-db/src/index_db_multi_insert.rs',
    'crates/cc-db/src/index_db_query.rs',
    'crates/cc-db/src/index_db_retrieval.rs',
    'crates/cc-db/src/index_db_types.rs',
    'crates/cc-db/src/index_db_write_batch.rs',
    'crates/cc-db/src/index_migrate.rs',
    'crates/cc-db/src/lib.rs',
    'crates/cc-db/src/resolution_dependency_store.rs',
    'crates/cc-db/src/rows.rs',
    'crates/cc-db/src/seed_symbol_cache.rs',
    'crates/cc-db/src/signature_agg.rs',
    'crates/cc-db/src/sql/index_v1.sql',
    'crates/cc-db/src/sql_util.rs',
    'crates/cc-db/src/symbol_identity_store.rs',
    'crates/cc-db/tests/incremental_write_bench.rs',
    'crates/cc-index/src/dirty_reload_policy.rs',
    'crates/cc-index/src/documents/symbol_identity.rs',
    'crates/cc-index/src/framework_resolvers/express.rs',
    'crates/cc-index/src/framework_resolvers/go_router.rs',
    'crates/cc-index/src/framework_resolvers/mount_resolution.rs',
    'crates/cc-index/src/framework_resolvers/react.rs',
    'crates/cc-index/src/framework_resolvers/spring.rs',
    'crates/cc-index/src/framework_resolvers/svelte.rs',
    'crates/cc-index/src/framework_resolvers/vue.rs',
    'crates/cc-index/src/hierarchy.rs',
    'crates/cc-index/src/indexer_phases/config_link.rs',
    'crates/cc-index/src/indexer_phases/dirty.rs',
    'crates/cc-index/src/indexer_phases/postprocess.rs',
    'crates/cc-index/src/indexer_phases/resolve.rs',
    'crates/cc-index/src/indexer_phases/snapshot.rs',
    'crates/cc-index/src/indexer_phases/write.rs',
    'crates/cc-index/src/infra_pass.rs',
    'crates/cc-index/src/project_model/mod.rs',
    'crates/cc-index/src/resolver/catalog.rs',
    'crates/cc-index/src/resolver/cpp_qualified_owner_tests.rs',
    'crates/cc-index/src/resolver/evidence.rs',
    'crates/cc-index/src/resolver/mod.rs',
    'crates/cc-index/src/resolver/resolve_core.rs',
    'crates/cc-index/src/resolver/resolve_outcome.rs',
    'crates/cc-index/src/resolver/route_resolve.rs',
    'crates/cc-index/src/resolver/types.rs',
    'crates/cc-index/src/synthesis_symbol_resolver.rs',
    'crates/cc-index/src/type_catalog.rs',
    'crates/cc-index/tests/cpp_namespace_identity_lifecycle.rs',
    'crates/cc-index/tests/cpp_qualified_owner_lifecycle.rs',
    'crates/cc-index/tests/fixtures/cpp-qualified-v26/README.md',
    'crates/cc-index/tests/fixtures/cpp-qualified-v26/expected-b1-deltas.json',
    'crates/cc-index/tests/fixtures/cpp-qualified-v26/generation-v26.json',
    'crates/cc-index/tests/fixtures/cpp-qualified-v26/index-v26.dbfixture',
    'crates/cc-index/tests/fixtures/cpp-qualified-v26/source-manifest.json',
    'crates/cc-index/tests/type_catalog_bench.rs',
    'crates/cc-model/src/cpp_owner.rs',
    'crates/cc-model/src/lib.rs',
    'crates/cc-model/src/parse.rs',
    'crates/cc-model/src/resolution.rs',
    'crates/cc-model/src/symbol.rs',
    'crates/cc-parsers/src/c_cpp.rs',
    'crates/cc-parsers/src/cpp_owner.rs',
    'crates/cc-parsers/src/dataflow_common.rs',
    'crates/cc-parsers/src/go.rs',
    'crates/cc-parsers/src/java.rs',
    'crates/cc-parsers/src/jsts/symbols.rs',
    'crates/cc-parsers/src/lib.rs',
    'crates/cc-parsers/src/python/mod.rs',
    'crates/cc-parsers/src/rust.rs',
    'crates/cc-parsers/src/sfc.rs',
    'crates/cc-parsers/src/spec_driven.rs',
    'crates/cc-parsers/tests/cpp_namespace_functions.rs',
    'crates/cc-parsers/tests/cpp_qualified_owner_red.rs',
    'crates/cc-search/src/engine.rs',
    'crates/cc-search/src/engine_lane_tests.rs',
    'crates/cc-search/src/engine_test_support.rs',
    'crates/cc-search/src/enrich.rs',
    'crates/cc-server/src/engine.rs',
    'docs/internals/MODULE_CAPABILITIES.json',
    'docs/reviews/cpp-qualified-owner-public-20261005/README.md',
    'docs/roadmap/code-index-v2/05-TODO.md',
    'docs/roadmap/code-index-v2/tasks.json',
}

CVREF_FTS_DELIVERY_PATHS = {
    'crates/cc-db/src/index_migrate.rs',
    'crates/cc-db/src/sql/index_v1.sql',
    'crates/cc-db/tests/symbols_fts_consistency.rs',
    'crates/cc-index/tests/cpp_cvref_identity_lifecycle.rs',
    'crates/cc-index/tests/fixtures/cpp-cvref-v27/README.md',
    'crates/cc-index/tests/fixtures/cpp-cvref-v27/baseline-audit.json',
    'crates/cc-index/tests/fixtures/cpp-cvref-v27/index-v27.dbfixture',
    'crates/cc-index/tests/fixtures/cpp-cvref-v27/provenance.json',
    'crates/cc-index/tests/fixtures/cpp-cvref-v27/source-manifest.json',
    'crates/cc-parsers/src/cpp_owner.rs',
    'crates/cc-parsers/tests/cpp_b1_cvref_identity.rs',
    'docs/reviews/cpp-cvref-identity-public-20261006/README.md',
    'docs/roadmap/code-index-v2/05-TODO.md',
    'docs/roadmap/code-index-v2/tasks.json',
}

B1_DELTA_PATHS = {p for p in B1_DELIVERY_PATHS if v1.scoped(p)}
CVREF_FTS_DELTA_PATHS = {p for p in CVREF_FTS_DELIVERY_PATHS if v1.scoped(p)}
PUBLIC_DOCUMENTS = (B1_DELIVERY_PATHS | CVREF_FTS_DELIVERY_PATHS) - (B1_DELTA_PATHS | CVREF_FTS_DELTA_PATHS)


def identities():
    return {
        'schema_version': 6, 'source_version': VERSION,
        'previous_source_version': v5.VERSION,
        'previous_registry_sha256': v5.REGISTRY_SHA256,
        'historical_source': V5_SOURCE, 'historical_source_tree': V5_SOURCE_TREE,
        'approved_base': V5_DELIVERY, 'approved_base_tree': V5_DELIVERY_TREE,
        'accepted_b1_source': B1_SOURCE, 'accepted_b1_tree': B1_TREE,
        'accepted_public_source': SOURCE, 'accepted_public_tree': SOURCE_TREE,
        'database_schema': 28, 'project_model_version': 3,
        'scope': 'source_integrity_only', 'quality_and_100k': 'not_inherited',
        'ci_source_version': v4.v3.VERSION,
    }


def verify_identities(registry):
    for key, value in identities().items():
        assert registry[key] == value, 'wrong v6 identity: ' + key


def load_registry(path=REGISTRY):
    raw = v4.require_regular_path(path.parent, path.name).read_bytes()
    assert v1.sha(raw) == REGISTRY_SHA256, 'stale or altered v6 registry'
    registry = json.loads(raw)
    verify_identities(registry)
    return registry


def historical_paths():
    """Validate frozen registry bytes before deriving their historical records."""
    paths = set(v5.PRESERVED_PATHS)
    for module in (v1, v4.v3.v2, v4.v3):
        raw = v4.require_regular_path(ROOT, module.REGISTRY.relative_to(ROOT).as_posix()).read_bytes()
        assert v1.sha(raw) == module.REGISTRY_SHA256, 'historical registry changed'
        paths.update(json.loads(raw)['records'])
    paths.update({
        'docs/checkpoints/2026-10-03-packing-integration/original-tests/qname_db_independent_review.rs',
        v1.B + 'test-only-lint-fix/engine_lane_tests.rs.original',
        v1.P + 'python_identity_independent.rs.original',
    })
    return paths


def preserved_paths():
    return historical_paths() | V5_DELIVERY_PATHS | PUBLIC_DOCUMENTS


def preserved_ref(path):
    if path in CVREF_FTS_DELIVERY_PATHS:
        return SOURCE
    if path in B1_DELIVERY_PATHS:
        return B1_SOURCE
    return V5_DELIVERY


def tree_entries(ref, *paths):
    entries = {}
    for row in v1.git('ls-tree', '-rz', ref, '--', *paths).split(b'\0'):
        if not row:
            continue
        info, path = row.decode().split('\t', 1)
        mode, kind, oid = info.split()
        assert kind == 'blob' and mode in {'100644', '100755'}, 'nonregular pinned input: ' + path
        assert path not in entries, 'duplicate pinned input: ' + path
        entries[path] = (mode, oid)
    return entries


def source_entries(ref=SOURCE):
    return tree_entries(ref, 'crates', 'Cargo.toml', 'Cargo.lock')


def verify_commit(ref, tree, parent):
    # Porcelain ancestry intentionally hides parents of shallow objects. Inspect
    # the immutable raw commit header instead; missing objects never mean roots.
    header = v1.git('cat-file', 'commit', ref).split(b'\n\n', 1)[0].decode().splitlines()
    assert [line[5:] for line in header if line.startswith('tree ')] == [tree], 'fixed tree differs'
    assert [line[7:] for line in header if line.startswith('parent ')] == [parent], 'public parent differs'
    assert v1.git('rev-parse', ref + '^{tree}').decode().strip() == tree, 'fixed tree differs'


def verify_delivery(registry, key, before_ref, after_ref, paths):
    rows = registry[key]
    assert set(rows) == paths, 'registered delivery inventory differs: ' + key
    actual = set(v1.git('diff', '--no-renames', '--name-only', before_ref, after_ref).decode().splitlines())
    assert actual == paths, 'public delivery inventory differs: ' + key
    before_entries = tree_entries(before_ref, *sorted(paths))
    after_entries = tree_entries(after_ref, *sorted(paths))
    assert set(after_entries) == paths, 'public delivery entries differ'
    for path, row in rows.items():
        before = v1.blob(before_ref, path) if path in before_entries else None
        assert row['before_sha256'] == (v1.sha(before) if before is not None else None), 'public before SHA differs: ' + path
        assert row['before_mode'] == (before_entries[path][0] if path in before_entries else None), 'public before mode differs: ' + path
        assert row['sha256'] == v1.sha(v1.blob(after_ref, path)), 'public source SHA differs: ' + path
        assert row['mode'] == after_entries[path][0], 'public source mode differs: ' + path


def verify_public_deltas(registry):
    verify_identities(registry)
    for ref, tree, parent in [
        (V5_SOURCE, V5_SOURCE_TREE, v5.BASE),
        (V5_DELIVERY, V5_DELIVERY_TREE, V5_SOURCE),
        (B1_SOURCE, B1_TREE, V5_DELIVERY),
        (SOURCE, SOURCE_TREE, B1_SOURCE),
    ]:
        verify_commit(ref, tree, parent)
    verify_delivery(registry, 'v5_delivery_delta', V5_SOURCE, V5_DELIVERY, V5_DELIVERY_PATHS)
    verify_delivery(registry, 'b1_public_delta', V5_DELIVERY, B1_SOURCE, B1_DELIVERY_PATHS)
    verify_delivery(registry, 'cvref_fts_public_delta', B1_SOURCE, SOURCE, CVREF_FTS_DELIVERY_PATHS)
    assert source_entries(V5_SOURCE) == source_entries(V5_DELIVERY), 'v5 delivery crate/Cargo modes or blobs differ'
    for ref, schema in [(V5_SOURCE, 26), (V5_DELIVERY, 26), (B1_SOURCE, 27), (SOURCE, 27)]:
        capability = json.loads(v1.blob(ref, CAPABILITIES))
        assert capability['database_schema'] == schema, 'public database schema differs'
        assert capability['project_model_version'] == 3, 'public project model differs'


def metadata_exception():
    before = v1.blob(SOURCE, CAPABILITIES)
    assert v1.sha(before) == METADATA_BEFORE_SHA256, 'metadata before SHA differs'
    old, new = b'"database_schema": 27', b'"database_schema": 28'
    assert before.count(old) == 1, 'metadata replacement count differs'
    after = before.replace(old, new)
    assert v1.sha(after) == METADATA_AFTER_SHA256, 'metadata after SHA differs'
    assert len(before) == len(after) == 2160, 'metadata byte count differs'
    a, b = json.loads(before), json.loads(after)
    assert a.pop('database_schema') == 27 and b.pop('database_schema') == 28 and a == b, 'metadata exception exceeds schema field'
    return after


def metadata_identity():
    return {
        'path': CAPABILITIES, 'source_commit': SOURCE,
        'before_sha256': METADATA_BEFORE_SHA256, 'sha256': METADATA_AFTER_SHA256,
        'before_mode': '100644', 'mode': '100644', 'bytes': 2160,
        'patch_sha256': METADATA_PATCH_SHA256, 'patch_bytes': 464,
        'before_database_schema': 27, 'database_schema': 28,
        'replacement': 'database_schema_only', 'after_git_blob': METADATA_AFTER_BLOB,
    }


def preserved_entries():
    entries = tree_entries(SOURCE, *sorted(preserved_paths()))
    assert set(entries) == preserved_paths(), 'pinned preserved inventory differs'
    entries[CAPABILITIES] = ('100644', METADATA_AFTER_BLOB)
    return entries


def preserved_index_entries(root):
    raw = subprocess.check_output(['git', 'ls-files', '--stage', '-z', '--', *sorted(preserved_paths())], cwd=root)
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
    assert registry['metadata_exception'] == metadata_identity(), 'metadata exception identity differs'
    assert set(registry['preserved_files']) == preserved_paths(), 'preserved inventory differs'
    entries = preserved_entries()
    if tracked is None:
        tracked = preserved_index_entries(root)
    assert tracked == entries, 'preserved index inventory, mode or blob differs'
    for path, row in registry['preserved_files'].items():
        assert row['commit'] == preserved_ref(path), 'preserved pin differs: ' + path
        raw = metadata_exception() if path == CAPABILITIES else v1.blob(row['commit'], path)
        assert row['sha256'] == v1.sha(raw), 'preserved SHA differs: ' + path
        assert row['mode'] == entries[path][0], 'preserved mode differs: ' + path
        if path != CAPABILITIES:
            assert v1.blob(SOURCE, path) == raw, 'public preserved file differs: ' + path
        target = v4.require_regular_path(root, path)
        assert bool(target.stat().st_mode & 0o111) == (row['mode'] == '100755'), 'preserved file mode differs: ' + path
        assert target.read_bytes() == raw, 'preserved file changed: ' + path


@contextmanager
def historical_snapshot():
    """Isolate exact schema26 disk/index evidence without retargeting old guards."""
    common = v1.git('rev-parse', '--git-common-dir').decode().strip()
    objects = (ROOT / common / 'objects').resolve()
    with tempfile.TemporaryDirectory(prefix='codecortex-v5-source-') as temp:
        root = Path(temp)
        subprocess.check_call(['git', 'init', '-q', str(root)])
        (root / '.git/objects/info/alternates').write_text(str(objects) + '\n')
        subprocess.check_call(['git', 'read-tree', V5_SOURCE], cwd=root)
        paths = historical_paths()
        entries = tree_entries(V5_SOURCE, *sorted(paths))
        assert set(entries) == paths, 'historical snapshot inventory differs'
        for path, (mode, _) in entries.items():
            target = root / path
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(v1.blob(V5_SOURCE, path))
            target.chmod(0o755 if mode == '100755' else 0o644)
        yield root


def apply_public_deltas(registry, previous):
    verify_public_deltas(registry)
    assert set(previous) == set(source_entries(V5_DELIVERY)), 'fixed v5 base inventory differs'
    assert all(raw == v1.blob(V5_DELIVERY, p) for p, raw in previous.items()), 'fixed v5 base bytes differ'
    assert len(previous) == 767, 'fixed v5 input count differs'
    expected = dict(previous)
    for key, before_ref, after_ref, paths, count in [
        ('b1_public_delta', V5_DELIVERY, B1_SOURCE, B1_DELTA_PATHS, 778),
        ('cvref_fts_public_delta', B1_SOURCE, SOURCE, CVREF_FTS_DELTA_PATHS, 786),
    ]:
        actual = set(v1.git('diff', '--no-renames', '--name-only', before_ref, after_ref, '--', 'crates', 'Cargo.toml', 'Cargo.lock').decode().splitlines())
        assert actual == paths, 'public source delta inventory differs'
        for path in sorted(paths):
            row = registry[key][path]
            before = expected.get(path)
            assert (v1.sha(before) if before is not None else None) == row['before_sha256'], 'ordered delta before SHA differs: ' + path
            expected[path] = v1.blob(after_ref, path)
        entries = source_entries(after_ref)
        assert len(expected) == count and set(expected) == set(entries), 'accepted union inventory differs'
        assert all(raw == v1.blob(after_ref, p) for p, raw in expected.items()), 'accepted union bytes differ'
        manifest_key = 'b1_complete_inputs' if after_ref == B1_SOURCE else 'complete_inputs'
        modes_key = 'b1_complete_modes' if after_ref == B1_SOURCE else 'complete_modes'
        assert {p: v1.sha(b) for p, b in expected.items()} == registry[manifest_key], 'complete manifest differs'
        assert {p: row[0] for p, row in entries.items()} == registry[modes_key], 'complete mode manifest differs'
    return expected


def approved_union(registry, root=ROOT):
    verify_identities(registry)
    v1.ensure_refs([V5_SOURCE, V5_DELIVERY, B1_SOURCE, SOURCE])
    verify_preserved_files(registry, root=root)
    with historical_snapshot() as snapshot:
        previous = v5.approved_union(v5.load_registry(), root=snapshot)
    return apply_public_deltas(registry, previous)


def verify_current_tree(root, expected, tracked=None):
    entries = source_entries()
    assert set(entries) == set(expected), 'pinned entry inventory differs'
    if tracked is None:
        tracked = v4.index_entries(root)
    assert set(tracked) == set(entries), 'current tracked input inventory differs'
    assert tracked == entries, 'current index mode or blob differs'
    disk = {p.relative_to(root).as_posix() for p in (root / 'crates').rglob('*') if p.is_symlink() or not p.is_dir()}
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
                          historical_inputs=767, b1_inputs=778,
                          b1_delivery_paths=len(B1_DELIVERY_PATHS), cvref_fts_delivery_paths=len(CVREF_FTS_DELIVERY_PATHS),
                          preserved_files=len(preserved_paths()), registry_sha256=REGISTRY_SHA256)))


if __name__ == '__main__':
    main()
