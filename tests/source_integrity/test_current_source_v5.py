"""Fixed public C++ source reconstruction with bounded adversarial controls."""
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
import verify_current_source_v5 as guard


class CppSourceTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        # Cache only immutable Git blobs. Current disk/index reads stay live.
        cls.cache = mock.patch.object(guard.v1, 'blob', functools.lru_cache(None)(guard.v1.blob))
        cls.cache.start()
        cls.addClassCleanup(cls.cache.stop)
        cls.registry = guard.load_registry()
        cls.expected = guard.approved_union(cls.registry)
        cls.previous = {p: guard.v1.blob(guard.BASE, p) for p in guard.v1.inputs(guard.BASE)}
        cls.entries = guard.source_entries()
        cls.preserved_entries = guard.preserved_entries()

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
            target.write_bytes(guard.v1.blob(row['commit'], path))
            target.chmod(0o755 if row['mode'] == '100755' else 0o644)

    def verify_preserved(self, registry=None):
        guard.verify_preserved_files(registry or self.registry, root=self.root,
                                     tracked=self.preserved_tracked)

    def test_complete_fixed_union_and_exact_delta(self):
        self.verify()
        self.assertEqual(len(self.previous), 764)
        self.assertEqual(len(self.expected), 767)
        self.assertEqual({p for p in self.expected if self.expected[p] != self.previous.get(p)}, guard.DELTA_PATHS)
        self.assertEqual(len(guard.DELTA_PATHS), 11)
        self.assertEqual(len(guard.DELIVERY_PATHS), 15)
        self.assertEqual(set(self.expected) - set(self.previous), {
            'crates/cc-index/tests/cpp_namespace_identity_lifecycle.rs',
            'crates/cc-parsers/tests/cpp_namespace_functions.rs',
            'crates/cc-parsers/tests/fixtures/cpp_namespace_baseline.json',
        })

    def test_repeated_reconstruction_is_deterministic(self):
        first = guard.apply_cpp_delta(self.registry, self.previous)
        second = guard.apply_cpp_delta(self.registry, self.previous)
        self.assertEqual(first, self.expected)
        self.assertEqual(first, second)
        self.assertEqual(self.previous, {p: guard.v1.blob(guard.BASE, p) for p in self.previous})

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
        paths = guard.DELTA_PATHS | {'Cargo.toml', 'Cargo.lock', 'crates/cc-search/src/plan.rs'}
        for path in sorted(paths):
            with self.subTest(path=path):
                target = self.root / path
                target.write_bytes(self.expected[path] + b'\n// unauthorized\n')
                with self.assertRaisesRegex(AssertionError, 'input bytes'):
                    self.verify()
                target.write_bytes(self.expected[path])

    def test_source_cargo_and_unchanged_input_omissions(self):
        paths = guard.DELTA_PATHS | {'Cargo.toml', 'Cargo.lock', 'crates/cc-search/src/plan.rs'}
        for path in sorted(paths):
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

    def test_unknown_real_fifo_and_socket_metadata_rejected(self):
        for name in ['crates/unapproved-special', 'crates/cc-index/target/unapproved-special']:
            target = self.root / name
            target.parent.mkdir(parents=True, exist_ok=True)
            os.mkfifo(target)
            with self.assertRaisesRegex(AssertionError, 'disk input inventory'):
                self.verify()
            target.unlink()
            target.write_bytes(b'harmless socket-type placeholder')
            original_stat = Path.stat

            def socket_type(path, *args, **kwargs):
                observed = original_stat(path, *args, **kwargs)
                if path == target:
                    return os.stat_result((stat.S_IFSOCK | 0o600, *observed[1:]))
                return observed

            # Metadata-only: do not create or execute a real socket.
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
        for path in ['Cargo.lock', sorted(guard.DELTA_PATHS)[0]]:
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
        for path in ['crates', 'crates/cc-parsers/tests/fixtures']:
            target = self.root / path
            relocated = self.root / 'relocated'
            target.rename(relocated)
            target.symlink_to(relocated, target_is_directory=True)
            with self.assertRaisesRegex(AssertionError, 'symlink input or directory|disk input inventory'):
                self.verify()
            target.unlink()
            relocated.rename(target)

    def test_disk_and_index_modes_for_changed_added_and_unchanged_inputs(self):
        for path in [*sorted(guard.DELTA_PATHS), 'Cargo.toml', 'crates/cc-search/src/plan.rs']:
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
        for raw in [b'100644 ' + b'0' * 40 + b' 2\tCargo.lock\0',
                    (b'100644 ' + b'0' * 40 + b' 0\tCargo.lock\0') * 2]:
            with mock.patch.object(guard.subprocess, 'check_output', return_value=raw):
                with self.assertRaisesRegex(AssertionError, 'unmerged or duplicate input'):
                    guard.v4.index_entries(self.root)
                with self.assertRaisesRegex(AssertionError, 'unmerged or duplicate preserved input'):
                    guard.preserved_index_entries(self.root)

    def test_registry_identity_and_serialization_are_fixed(self):
        for key, expected in guard.identities().items():
            r = copy.deepcopy(self.registry)
            r[key] = 0 if isinstance(expected, int) else 'HEAD'
            with self.assertRaisesRegex(AssertionError, 'wrong v5 identity'):
                guard.apply_cpp_delta(r, self.previous)
        path = self.root / 'registry.json'
        path.write_text(json.dumps(self.registry, separators=(',', ':')))
        with self.assertRaisesRegex(AssertionError, 'stale or altered v5 registry'):
            guard.load_registry(path)
        path.unlink()
        path.symlink_to(guard.REGISTRY)
        with self.assertRaisesRegex(AssertionError, 'symlink input'):
            guard.load_registry(path)

    def test_public_delivery_inventory_and_every_row_are_fixed(self):
        for path in sorted(guard.DELIVERY_PATHS):
            for field in ['before_sha256', 'sha256', 'before_mode', 'mode']:
                with self.subTest(path=path, field=field):
                    r = copy.deepcopy(self.registry)
                    r['public_delta'][path][field] = 'wrong'
                    with self.assertRaisesRegex(AssertionError, 'public (before|source) (SHA|mode) differs'):
                        guard.verify_public_delta(r)
            r = copy.deepcopy(self.registry)
            del r['public_delta'][path]
            with self.assertRaisesRegex(AssertionError, 'registered delivery inventory differs'):
                guard.verify_public_delta(r)
        r = copy.deepcopy(self.registry)
        r['public_delta']['crates/unapproved.rs'] = {}
        with self.assertRaisesRegex(AssertionError, 'registered delivery inventory differs'):
            guard.verify_public_delta(r)

    def test_complete_manifest_hash_modes_and_inventory_are_fixed(self):
        for field in ['complete_inputs', 'complete_modes']:
            for action in ['change', 'add', 'remove']:
                r = copy.deepcopy(self.registry)
                if action == 'change':
                    r[field]['Cargo.lock'] = 'wrong'
                elif action == 'add':
                    r[field]['crates/unapproved.rs'] = 'wrong'
                else:
                    del r[field]['Cargo.lock']
                with self.assertRaisesRegex(AssertionError, 'complete (mode )?manifest differs'):
                    guard.apply_cpp_delta(r, self.previous)

    def test_arbitrary_previous_union_rejected(self):
        for action in ['change', 'add', 'remove']:
            previous = dict(self.previous)
            if action == 'change':
                previous['Cargo.lock'] += b'\n'
            elif action == 'add':
                previous['crates/unapproved.rs'] = b'\n'
            else:
                del previous['Cargo.lock']
            with self.assertRaisesRegex(AssertionError, 'fixed base (bytes differ|inventory differs)'):
                guard.apply_cpp_delta(self.registry, previous)

    def test_wrong_fixed_tree_parent_or_delivery_rejected(self):
        original = guard.v1.git
        for selector, replacement, message in [
            (('rev-parse', guard.SOURCE + '^{tree}'), b'0' * 40, 'fixed tree differs'),
            (('rev-parse', guard.BASE + '^{tree}'), b'0' * 40, 'fixed tree differs'),
            (('show', '-s', '--format=%P', guard.SOURCE), b'0' * 40, 'public parent differs'),
            (('diff', '--no-renames', '--name-only', guard.BASE, guard.SOURCE), b'crates/unapproved.rs\n', 'public delivery inventory differs'),
        ]:
            def altered(*args):
                return replacement if args == selector else original(*args)
            with mock.patch.object(guard.v1, 'git', side_effect=altered):
                with self.assertRaisesRegex(AssertionError, message):
                    guard.apply_cpp_delta(self.registry, self.previous)

    def test_missing_fixed_objects_have_no_fallback(self):
        with mock.patch.object(guard.v1, 'ensure_refs', side_effect=subprocess.CalledProcessError(1, 'git fetch')) as fetch:
            with self.assertRaises(subprocess.CalledProcessError):
                guard.approved_union(self.registry)
            fetch.assert_called_once_with([guard.BASE, guard.SOURCE])

    def test_public_schema_is_exact_and_current_metadata_preserved(self):
        self.preserve()
        capabilities = json.loads((self.root / guard.CAPABILITIES).read_bytes())
        self.assertEqual(capabilities['database_schema'], 26)
        self.assertEqual(capabilities['project_model_version'], 3)
        capabilities['database_schema'] = 25
        (self.root / guard.CAPABILITIES).write_text(json.dumps(capabilities))
        with self.assertRaisesRegex(AssertionError, 'preserved file changed'):
            self.verify_preserved()

    def test_preserved_files_bytes_omissions_and_leaf_symlinks(self):
        self.preserve()
        self.verify_preserved()
        for path in sorted(guard.PRESERVED_PATHS):
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
        for path in [guard.CAPABILITIES, guard.PUBLIC_CONCLUSION, 'scripts/verify_current_source_v4.py']:
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
        for path in sorted(guard.PRESERVED_PATHS):
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
                del r['preserved_files'][guard.PUBLIC_CONCLUSION]
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
        target = self.root / guard.PUBLIC_CONCLUSION
        target.unlink()
        os.mkfifo(target)
        with self.assertRaisesRegex(AssertionError, 'nonregular current input'):
            self.verify_preserved()

    def test_explicit_cli_selector_and_optimization_rejection(self):
        script = guard.ROOT / 'scripts/verify_current_source_v5.py'
        versions = ['HEAD', 'latest', 'unknown-v5', guard.v4.VERSION, guard.v4.v3.VERSION,
                    guard.v4.v3.v2.VERSION, 'python-resource-20261004-v1']
        for args in [[], *[['--source-version', v] for v in versions]]:
            result = subprocess.run([sys.executable, '-B', str(script), *args], capture_output=True)
            self.assertEqual(result.returncode, 2)
        for optimization in ['-O', '-OO']:
            result = subprocess.run([sys.executable, '-B', optimization, str(script),
                                     '--source-version', guard.VERSION], capture_output=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn(b'requires Python assertions', result.stderr)


if __name__ == '__main__':
    unittest.main()
