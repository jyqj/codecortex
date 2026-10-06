"""Ordered public source reconstruction and adversarial v6 inventory controls."""
import copy
import functools
import json
import os
from pathlib import Path
import stat
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / 'scripts'))
import verify_current_source_v6 as guard


class OrderedPublicSourceTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        # Only immutable Git blob reads are cached; disk/index reads stay live.
        cls.cache = mock.patch.object(guard.v1, 'blob', functools.lru_cache(None)(guard.v1.blob))
        cls.cache.start()
        cls.addClassCleanup(cls.cache.stop)
        cls.registry = guard.load_registry()
        cls.expected = guard.approved_union(cls.registry)
        cls.previous = {p: guard.v1.blob(guard.V5_DELIVERY, p) for p in guard.source_entries(guard.V5_DELIVERY)}
        cls.entries = guard.source_entries()
        cls.preserved_entries = guard.preserved_entries()
        cls.delta_paths = guard.B1_DELTA_PATHS | guard.CVREF_FTS_DELTA_PATHS

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        for path, raw in self.expected.items():
            target = self.root / path
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(raw)
            target.chmod(0o755 if self.entries[path][0] == '100755' else 0o644)
        self.tracked = dict(self.entries)
        self.preserved_tracked = dict(self.preserved_entries)

    def verify(self):
        guard.verify_current_tree(self.root, self.expected, self.tracked)

    def preserve(self):
        for path, row in self.registry['preserved_files'].items():
            target = self.root / path
            target.parent.mkdir(parents=True, exist_ok=True)
            raw = guard.metadata_exception() if path == guard.CAPABILITIES else guard.v1.blob(row['commit'], path)
            target.write_bytes(raw)
            target.chmod(0o755 if row['mode'] == '100755' else 0o644)

    def verify_preserved(self, registry=None):
        guard.verify_preserved_files(registry or self.registry, root=self.root, tracked=self.preserved_tracked)

    def test_complete_ordered_union_and_exact_delivery_counts(self):
        self.verify()
        self.assertEqual(len(self.previous), 767)
        self.assertEqual(len(self.registry['b1_complete_inputs']), 778)
        self.assertEqual(len(self.expected), 786)
        self.assertEqual(len(guard.V5_DELIVERY_PATHS), 4)
        self.assertEqual(len(guard.B1_DELTA_PATHS), 76)
        self.assertEqual(len(guard.B1_DELIVERY_PATHS), 80)
        self.assertEqual(len(guard.CVREF_FTS_DELTA_PATHS), 11)
        self.assertEqual(len(guard.CVREF_FTS_DELIVERY_PATHS), 14)
        self.assertEqual(len(set(self.expected) - set(self.previous)), 19)
        self.assertEqual({p for p in self.expected if self.expected[p] != self.previous.get(p)}, self.delta_paths)

    def test_reconstruction_is_deterministic_and_does_not_change_v5_union(self):
        before = dict(self.previous)
        first = guard.apply_public_deltas(self.registry, self.previous)
        second = guard.apply_public_deltas(self.registry, self.previous)
        self.assertEqual(first, self.expected)
        self.assertEqual(first, second)
        self.assertEqual(before, self.previous)

    def test_historical_snapshot_keeps_schema26_and_exact_source_index(self):
        with guard.historical_snapshot() as snapshot:
            self.assertEqual(json.loads((snapshot / guard.CAPABILITIES).read_bytes())['database_schema'], 26)
            guard.v5.verify_preserved_files(guard.v5.load_registry(), root=snapshot)
            self.assertEqual(guard.v4.index_entries(snapshot), guard.source_entries(guard.V5_SOURCE))
            previous = guard.v5.approved_union(guard.v5.load_registry(), root=snapshot)
            self.assertEqual(previous, self.previous)

    def test_old_v5_legitimately_rejects_current_source(self):
        with self.assertRaisesRegex(AssertionError, 'preserved index inventory, mode or blob differs'):
            guard.v5.approved_union(guard.v5.load_registry(), root=guard.ROOT)

    def test_unknown_disk_and_tracked_additions(self):
        for path in ['crates/unapproved.rs', 'crates/cc-index/target/ignored.rs']:
            target = self.root / path
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text('// unapproved\n')
            with self.assertRaisesRegex(AssertionError, 'disk input inventory'):
                self.verify()
            target.unlink()
        self.tracked['crates/unapproved.rs'] = ('100644', '0' * 40)
        with self.assertRaisesRegex(AssertionError, 'tracked input inventory'):
            self.verify()

    def test_source_cargo_and_unchanged_input_mutations(self):
        for path in sorted(self.delta_paths | {'Cargo.toml', 'Cargo.lock', 'crates/cc-search/src/plan.rs'}):
            with self.subTest(path=path):
                target = self.root / path
                target.write_bytes(self.expected[path] + b'\n// unauthorized\n')
                with self.assertRaisesRegex(AssertionError, 'input bytes'):
                    self.verify()
                target.write_bytes(self.expected[path])

    def test_source_cargo_and_unchanged_input_omissions(self):
        for path in sorted(self.delta_paths | {'Cargo.toml', 'Cargo.lock', 'crates/cc-search/src/plan.rs'}):
            with self.subTest(path=path):
                target = self.root / path
                target.unlink()
                with self.assertRaises((AssertionError, FileNotFoundError)):
                    self.verify()
                target.write_bytes(self.expected[path])
                del self.tracked[path]
                with self.assertRaisesRegex(AssertionError, 'tracked input inventory'):
                    self.verify()
                self.tracked[path] = self.entries[path]

    def test_unknown_fifo_and_socket_metadata_rejected_without_opening(self):
        for name in ['crates/unapproved-special', 'crates/cc-index/target/unapproved-special']:
            target = self.root / name
            target.parent.mkdir(parents=True, exist_ok=True)
            os.mkfifo(target)
            with self.assertRaisesRegex(AssertionError, 'disk input inventory'):
                self.verify()
            target.unlink()
            target.write_bytes(b'socket metadata placeholder')
            original_stat = Path.stat
            def socket_type(path, *args, **kwargs):
                observed = original_stat(path, *args, **kwargs)
                return os.stat_result((stat.S_IFSOCK | 0o600, *observed[1:])) if path == target else observed
            with mock.patch.object(Path, 'stat', socket_type):
                with self.assertRaisesRegex(AssertionError, 'disk input inventory'):
                    self.verify()
            target.unlink()

    def test_expected_fifo_rejected_without_opening(self):
        target = self.root / 'Cargo.lock'
        target.unlink()
        os.mkfifo(target)
        with self.assertRaisesRegex(AssertionError, 'nonregular current input'):
            self.verify()

    def test_unknown_symlink_directory_rejected(self):
        (self.root / 'crates/unknown').symlink_to(self.root / 'crates/cc-search', target_is_directory=True)
        with self.assertRaisesRegex(AssertionError, 'disk input inventory'):
            self.verify()

    def test_leaf_symlinks_rejected(self):
        for path in ['Cargo.lock', sorted(self.delta_paths)[0]]:
            target = self.root / path
            original = self.root / 'original'
            original.write_bytes(target.read_bytes())
            target.unlink()
            target.symlink_to(original)
            with self.assertRaisesRegex(AssertionError, 'symlink input'):
                self.verify()
            target.unlink()
            target.write_bytes(self.expected[path])

    def test_source_directory_symlinks_rejected(self):
        for path in ['crates', 'crates/cc-index/tests/fixtures']:
            target = self.root / path
            relocated = self.root / 'relocated'
            target.rename(relocated)
            target.symlink_to(relocated, target_is_directory=True)
            with self.assertRaisesRegex(AssertionError, 'symlink input or directory|disk input inventory'):
                self.verify()
            target.unlink()
            relocated.rename(target)

    def test_disk_and_index_modes_for_changed_added_and_unchanged_inputs(self):
        for path in [*sorted(self.delta_paths), 'Cargo.toml', 'crates/cc-search/src/plan.rs']:
            target = self.root / path
            target.chmod(0o755)
            with self.assertRaisesRegex(AssertionError, 'current file mode differs'):
                self.verify()
            target.chmod(0o644)
            self.tracked[path] = ('100755', self.entries[path][1])
            with self.assertRaisesRegex(AssertionError, 'current index mode or blob differs'):
                self.verify()
            self.tracked[path] = self.entries[path]

    def test_staged_blob_drift_rejected(self):
        self.tracked['Cargo.lock'] = ('100644', '0' * 40)
        with self.assertRaisesRegex(AssertionError, 'current index mode or blob differs'):
            self.verify()

    def test_real_index_modes_blobs_and_unmerged_stages_rejected(self):
        def git(*args, data=None):
            return subprocess.check_output(['git', *args], cwd=self.root, input=data, stderr=subprocess.DEVNULL)
        git('init', '-q')
        git('add', '--', 'crates', 'Cargo.toml', 'Cargo.lock')
        guard.verify_current_tree(self.root, self.expected)
        git('update-index', '--chmod=+x', 'Cargo.toml')
        with self.assertRaisesRegex(AssertionError, 'current index mode or blob differs'):
            guard.verify_current_tree(self.root, self.expected)
        git('update-index', '--chmod=-x', 'Cargo.toml')
        oid = git('hash-object', '-w', '--stdin', data=b'altered staged bytes').decode().strip()
        git('update-index', '--cacheinfo', '100644,' + oid + ',Cargo.lock')
        with self.assertRaisesRegex(AssertionError, 'current index mode or blob differs'):
            guard.verify_current_tree(self.root, self.expected)
        oid = self.entries['Cargo.lock'][1]
        git('update-index', '--index-info', data=('0 ' + '0' * 40 + '\tCargo.lock\n100644 ' + oid + ' 2\tCargo.lock\n').encode())
        with self.assertRaisesRegex(AssertionError, 'unmerged or duplicate input'):
            guard.verify_current_tree(self.root, self.expected)

    def test_duplicate_and_unmerged_index_parsers(self):
        for raw in [b'100644 ' + b'0' * 40 + b' 2\tCargo.lock\0', (b'100644 ' + b'0' * 40 + b' 0\tCargo.lock\0') * 2]:
            with mock.patch.object(guard.subprocess, 'check_output', return_value=raw):
                with self.assertRaisesRegex(AssertionError, 'unmerged or duplicate input'):
                    guard.v4.index_entries(self.root)
                with self.assertRaisesRegex(AssertionError, 'unmerged or duplicate preserved input'):
                    guard.preserved_index_entries(self.root)

    def test_registry_identity_and_serialization_are_fixed(self):
        for key, expected in guard.identities().items():
            r = copy.deepcopy(self.registry)
            r[key] = 0 if isinstance(expected, int) else 'HEAD'
            with self.assertRaisesRegex(AssertionError, 'wrong v6 identity'):
                guard.apply_public_deltas(r, self.previous)
        path = self.root / 'registry.json'
        path.write_text(json.dumps(self.registry, separators=(',', ':')))
        with self.assertRaisesRegex(AssertionError, 'stale or altered v6 registry'):
            guard.load_registry(path)
        path.unlink()
        path.symlink_to(guard.REGISTRY)
        with self.assertRaisesRegex(AssertionError, 'symlink input'):
            guard.load_registry(path)

    def test_every_public_delivery_row_and_inventory_are_fixed(self):
        for key, before, after, paths in [('v5_delivery_delta', guard.V5_SOURCE, guard.V5_DELIVERY, guard.V5_DELIVERY_PATHS), ('b1_public_delta', guard.V5_DELIVERY, guard.B1_SOURCE, guard.B1_DELIVERY_PATHS), ('cvref_fts_public_delta', guard.B1_SOURCE, guard.SOURCE, guard.CVREF_FTS_DELIVERY_PATHS)]:
            for path in sorted(paths):
                for field in ['before_sha256', 'sha256', 'before_mode', 'mode']:
                    with self.subTest(key=key, path=path, field=field):
                        r = copy.deepcopy(self.registry)
                        r[key][path][field] = 'wrong'
                        with self.assertRaisesRegex(AssertionError, 'public (before|source) (SHA|mode) differs'):
                            guard.verify_delivery(r, key, before, after, paths)
                r = copy.deepcopy(self.registry)
                del r[key][path]
                with self.assertRaisesRegex(AssertionError, 'registered delivery inventory differs'):
                    guard.verify_delivery(r, key, before, after, paths)
            r = copy.deepcopy(self.registry)
            r[key]['crates/unapproved.rs'] = {}
            with self.assertRaisesRegex(AssertionError, 'registered delivery inventory differs'):
                guard.verify_delivery(r, key, before, after, paths)

    def test_complete_manifest_hash_modes_and_inventory_are_fixed(self):
        for field in ['b1_complete_inputs', 'b1_complete_modes', 'complete_inputs', 'complete_modes']:
            for action in ['change', 'add', 'remove']:
                r = copy.deepcopy(self.registry)
                if action == 'change':
                    r[field]['Cargo.lock'] = 'wrong'
                elif action == 'add':
                    r[field]['crates/unapproved.rs'] = 'wrong'
                else:
                    del r[field]['Cargo.lock']
                with self.assertRaisesRegex(AssertionError, 'complete (mode )?manifest differs'):
                    guard.apply_public_deltas(r, self.previous)

    def test_arbitrary_previous_union_rejected(self):
        for action in ['change', 'add', 'remove']:
            previous = dict(self.previous)
            if action == 'change':
                previous['Cargo.lock'] += b'\n'
            elif action == 'add':
                previous['crates/unapproved.rs'] = b'\n'
            else:
                del previous['Cargo.lock']
            with self.assertRaisesRegex(AssertionError, 'fixed v5 base (bytes differ|inventory differs)'):
                guard.apply_public_deltas(self.registry, previous)

    def test_wrong_raw_tree_parent_and_delivery_inventory_rejected(self):
        original = guard.v1.git
        for ref, tree, parent in [(guard.V5_SOURCE, guard.V5_SOURCE_TREE, guard.v5.BASE), (guard.V5_DELIVERY, guard.V5_DELIVERY_TREE, guard.V5_SOURCE), (guard.B1_SOURCE, guard.B1_TREE, guard.V5_DELIVERY), (guard.SOURCE, guard.SOURCE_TREE, guard.B1_SOURCE)]:
            for raw, message in [(f'tree {"0" * 40}\nparent {parent}\n\nx'.encode(), 'fixed tree differs'), (f'tree {tree}\n\nx'.encode(), 'public parent differs'), (f'tree {tree}\nparent {"0" * 40}\n\nx'.encode(), 'public parent differs')]:
                def altered(*args):
                    return raw if args == ('cat-file', 'commit', ref) else original(*args)
                with mock.patch.object(guard.v1, 'git', side_effect=altered):
                    with self.assertRaisesRegex(AssertionError, message):
                        guard.verify_commit(ref, tree, parent)
            def changed_tree(*args):
                return b'0' * 40 if args == ('rev-parse', ref + '^{tree}') else original(*args)
            with mock.patch.object(guard.v1, 'git', side_effect=changed_tree):
                with self.assertRaisesRegex(AssertionError, 'fixed tree differs'):
                    guard.verify_commit(ref, tree, parent)
        selector = ('diff', '--no-renames', '--name-only', guard.B1_SOURCE, guard.SOURCE)
        def changed_delivery(*args):
            return b'crates/unapproved.rs\n' if args == selector else original(*args)
        with mock.patch.object(guard.v1, 'git', side_effect=changed_delivery):
            with self.assertRaisesRegex(AssertionError, 'public delivery inventory differs'):
                guard.verify_public_deltas(self.registry)

    def test_shallow_porcelain_parent_is_never_used_for_v6_ancestry(self):
        original = guard.v1.git
        def shallow_porcelain(*args):
            if args[0] in {'show', 'rev-list'} and '--format=%P' in args:
                self.fail('v6 must inspect raw parents')
            return original(*args)
        with mock.patch.object(guard.v1, 'git', side_effect=shallow_porcelain):
            guard.verify_public_deltas(self.registry)

    def test_missing_fixed_objects_have_no_fallback(self):
        with mock.patch.object(guard.v1, 'ensure_refs', side_effect=subprocess.CalledProcessError(1, 'git fetch')) as fetch:
            with self.assertRaises(subprocess.CalledProcessError):
                guard.approved_union(self.registry)
            fetch.assert_called_once_with([guard.V5_SOURCE, guard.V5_DELIVERY, guard.B1_SOURCE, guard.SOURCE])

    def test_metadata_exception_changes_only_one_field_and_exact_bytes(self):
        before = guard.v1.blob(guard.SOURCE, guard.CAPABILITIES)
        after = guard.metadata_exception()
        self.assertEqual(before.replace(b'"database_schema": 27', b'"database_schema": 28'), after)
        self.assertEqual(len(before), 2160)
        self.assertEqual(guard.v1.sha(after), guard.METADATA_AFTER_SHA256)
        a, b = json.loads(before), json.loads(after)
        self.assertEqual(a.pop('database_schema'), 27)
        self.assertEqual(b.pop('database_schema'), 28)
        self.assertEqual(a, b)
        self.preserve()
        self.verify_preserved()
        target = self.root / guard.CAPABILITIES
        for raw in [before, after.replace(b'"project_model_version": 3', b'"project_model_version": 4'), after.replace(b'"rebuild_on_mismatch"', b'"continue_on_mismatch"'), json.dumps(json.loads(after)).encode(), after + b'\n']:
            target.write_bytes(raw)
            with self.assertRaisesRegex(AssertionError, 'preserved file changed'):
                self.verify_preserved()
        target.write_bytes(after)

    def test_every_metadata_exception_identity_is_fixed(self):
        self.preserve()
        for key in guard.metadata_identity():
            r = copy.deepcopy(self.registry)
            r['metadata_exception'][key] = 'wrong'
            with self.assertRaisesRegex(AssertionError, 'metadata exception identity differs'):
                self.verify_preserved(r)
        original = guard.v1.blob
        def changed_before(ref, path):
            raw = original(ref, path)
            return raw + b'\n' if ref == guard.SOURCE and path == guard.CAPABILITIES else raw
        with mock.patch.object(guard.v1, 'blob', side_effect=changed_before):
            with self.assertRaisesRegex(AssertionError, 'metadata before SHA differs'):
                guard.metadata_exception()

    def test_preserved_files_bytes_omissions_and_leaf_symlinks(self):
        self.preserve()
        self.verify_preserved()
        for path in sorted(guard.preserved_paths()):
            target = self.root / path
            original = target.read_bytes()
            target.write_bytes(original + b'\nchanged\n')
            with self.assertRaisesRegex(AssertionError, 'preserved file changed'):
                self.verify_preserved()
            target.unlink()
            with self.assertRaises(FileNotFoundError):
                self.verify_preserved()
            (self.root / 'original').write_bytes(original)
            target.symlink_to(self.root / 'original')
            with self.assertRaisesRegex(AssertionError, 'symlink input'):
                self.verify_preserved()
            target.unlink()
            target.write_bytes(original)

    def test_preserved_disk_and_index_modes_or_staged_blobs(self):
        self.preserve()
        for path in [guard.CAPABILITIES, 'scripts/verify_current_source_v5.py', 'docs/reviews/cpp-cvref-identity-public-20261006/README.md']:
            target = self.root / path
            target.chmod(0o755)
            with self.assertRaisesRegex(AssertionError, 'preserved file mode differs'):
                self.verify_preserved()
            target.chmod(0o644)
            for entry in [('100755', self.preserved_entries[path][1]), ('100644', '0' * 40)]:
                self.preserved_tracked[path] = entry
                with self.assertRaisesRegex(AssertionError, 'preserved index inventory, mode or blob differs'):
                    self.verify_preserved()
            del self.preserved_tracked[path]
            with self.assertRaisesRegex(AssertionError, 'preserved index inventory, mode or blob differs'):
                self.verify_preserved()
            self.preserved_tracked[path] = self.preserved_entries[path]

    def test_preserved_pins_hashes_modes_and_inventory(self):
        self.preserve()
        for path in sorted(guard.preserved_paths()):
            for field, value in [('commit', 'HEAD'), ('sha256', '0' * 64), ('mode', '100755')]:
                r = copy.deepcopy(self.registry)
                r['preserved_files'][path][field] = value
                with self.assertRaisesRegex(AssertionError, 'preserved (pin|SHA|mode) differs'):
                    self.verify_preserved(r)
        for added in [True, False]:
            r = copy.deepcopy(self.registry)
            if added:
                r['preserved_files']['docs/unapproved.md'] = {}
            else:
                del r['preserved_files'][guard.CAPABILITIES]
            with self.assertRaisesRegex(AssertionError, 'preserved inventory differs'):
                self.verify_preserved(r)

    def test_preserved_directory_symlink_and_fifo_rejected(self):
        self.preserve()
        target = self.root / 'docs'
        relocated = self.root / 'relocated-docs'
        target.rename(relocated)
        target.symlink_to(relocated, target_is_directory=True)
        with self.assertRaisesRegex(AssertionError, 'symlink input or directory'):
            self.verify_preserved()
        target.unlink()
        relocated.rename(target)
        target = self.root / guard.CAPABILITIES
        target.unlink()
        os.mkfifo(target)
        with self.assertRaisesRegex(AssertionError, 'nonregular current input'):
            self.verify_preserved()

    def test_explicit_cli_selector_and_optimization_rejection(self):
        script = guard.ROOT / 'scripts/verify_current_source_v6.py'
        versions = ['HEAD', 'latest', 'unknown-v6', guard.v5.VERSION, guard.v4.VERSION, guard.v4.v3.VERSION, guard.v4.v3.v2.VERSION, 'python-resource-20261004-v1']
        for args in [[], *[['--source-version', version] for version in versions]]:
            result = subprocess.run([sys.executable, '-B', str(script), *args], capture_output=True)
            self.assertEqual(result.returncode, 2)
        for optimization in ['-O', '-OO']:
            result = subprocess.run([sys.executable, '-B', optimization, str(script), '--source-version', guard.VERSION], capture_output=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn(b'requires Python assertions', result.stderr)


if __name__ == '__main__':
    unittest.main()
