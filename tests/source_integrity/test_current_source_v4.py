"""Immutable public member-source registry and bounded adversarial controls."""
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
import verify_current_source_v4 as guard


class MemberSourceTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        # Only immutable Git blobs are cached; current files/index are always read.
        cls.cache = mock.patch.object(guard.v1, 'blob', functools.lru_cache(None)(guard.v1.blob))
        cls.cache.start()
        cls.addClassCleanup(cls.cache.stop)
        cls.registry = guard.load_registry()
        cls.expected = guard.approved_union(cls.registry)
        cls.previous = {p: guard.v1.blob(guard.BASE, p) for p in cls.expected}
        cls.entries = guard.source_entries()

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

    def verify(self):
        guard.verify_current_tree(self.root, self.expected, self.tracked)

    def preserve(self):
        for path, row in self.registry['preserved_files'].items():
            target = self.root / path
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(guard.v1.blob(row['commit'], path))

    def test_complete_fixed_union_and_exact_delta(self):
        self.verify()
        self.assertEqual(len(self.expected), 764)
        self.assertEqual({p for p in self.expected if self.expected[p] != self.previous[p]}, guard.DELTA_PATHS)
        self.assertEqual(guard.apply_member_delta(self.registry, self.previous), self.expected)

    def test_unknown_disk_and_tracked_additions(self):
        target = self.root / 'crates/unapproved.rs'
        target.write_text('// unapproved\n')
        with self.assertRaisesRegex(AssertionError, 'disk input inventory'):
            self.verify()
        target.unlink()
        self.tracked['crates/unapproved.rs'] = ('100644', '0' * 40)
        with self.assertRaisesRegex(AssertionError, 'tracked input inventory'):
            self.verify()

    def test_source_cargo_and_earlier_capture_mutations(self):
        for path in sorted(guard.DELTA_PATHS | guard.v3.v2.CAPTURE_PATHS | {'Cargo.toml', 'Cargo.lock'}):
            with self.subTest(path=path):
                target = self.root / path
                target.write_bytes(self.expected[path] + b'\n// unauthorized\n')
                with self.assertRaisesRegex(AssertionError, 'input bytes'):
                    self.verify()
                target.write_bytes(self.expected[path])

    def test_unknown_special_files_rejected(self):
        fifo = self.root / 'crates/unapproved-pipe'
        os.mkfifo(fifo)
        with self.assertRaisesRegex(AssertionError, 'disk input inventory'):
            self.verify()
        fifo.unlink()
        sockpath = self.root / 'crates/unapproved-socket'
        sockpath.write_bytes(b'harmless socket-type placeholder')
        original_stat = Path.stat

        def socket_type(path, *args, **kwargs):
            observed = original_stat(path, *args, **kwargs)
            if path == sockpath:
                return os.stat_result((stat.S_IFSOCK | 0o600, *observed[1:]))
            return observed

        # A metadata-only socket control needs no socket-creation permissions.
        # The real FIFO above exercises the same nondirectory inventory rule.
        with mock.patch.object(Path, 'stat', socket_type):
            with self.assertRaisesRegex(AssertionError, 'disk input inventory'):
                self.verify()

    def test_unknown_symlink_directory_rejected(self):
        target = self.root / 'crates/unapproved-directory'
        target.symlink_to(self.root / 'crates/cc-search', target_is_directory=True)
        with self.assertRaisesRegex(AssertionError, 'disk input inventory'):
            self.verify()

    def test_source_and_cargo_omissions(self):
        for path in sorted(guard.DELTA_PATHS | {'Cargo.toml', 'Cargo.lock'}):
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

    def test_leaf_symlinks_rejected(self):
        for path in ['Cargo.lock', sorted(guard.DELTA_PATHS)[0]]:
            with self.subTest(path=path):
                target = self.root / path
                original = self.root / 'original'
                original.write_bytes(target.read_bytes())
                target.unlink()
                target.symlink_to(original)
                with self.assertRaisesRegex(AssertionError, 'symlink input'):
                    self.verify()
                target.unlink()
                target.write_bytes(self.expected[path])

    def test_directory_symlinks_rejected(self):
        for path in ['crates', 'crates/cc-search/src']:
            with self.subTest(path=path):
                target = self.root / path
                relocated = self.root / 'relocated'
                target.rename(relocated)
                target.symlink_to(relocated, target_is_directory=True)
                with self.assertRaisesRegex(AssertionError, 'symlink input or directory|disk input inventory'):
                    self.verify()
                target.unlink()
                relocated.rename(target)

    def test_disk_and_index_mode_drift_rejected(self):
        path = 'Cargo.toml'
        target = self.root / path
        target.chmod(0o755)
        with self.assertRaisesRegex(AssertionError, 'current file mode differs'):
            self.verify()
        target.chmod(0o644)
        self.tracked[path] = ('100755', self.entries[path][1])
        with self.assertRaisesRegex(AssertionError, 'current index mode or blob differs'):
            self.verify()

    def test_staged_blob_drift_rejected(self):
        self.tracked['Cargo.lock'] = ('100644', '0' * 40)
        with self.assertRaisesRegex(AssertionError, 'current index mode or blob differs'):
            self.verify()

    def test_unmerged_and_duplicate_index_stages_rejected(self):
        for raw in [b'100644 ' + b'0' * 40 + b' 2\tCargo.lock\0',
                    (b'100644 ' + b'0' * 40 + b' 0\tCargo.lock\0') * 2]:
            with self.subTest(raw=raw), mock.patch.object(guard.subprocess, 'check_output', return_value=raw):
                with self.assertRaisesRegex(AssertionError, 'unmerged or duplicate input'):
                    guard.index_entries(self.root)

    def test_registry_cannot_admit_head_latest_old_versions_or_quality(self):
        for mutation in [
            {'accepted_public_source': 'HEAD'}, {'accepted_public_source': 'latest'},
            {'accepted_public_tree': 'HEAD^{tree}'}, {'approved_base': 'HEAD'},
            {'previous_registry_sha256': '0' * 64}, {'previous_source_version': 'latest'},
            {'source_version': guard.v3.VERSION}, {'schema_version': 3},
            {'scope': 'quality'}, {'quality_and_100k': 'passed'},
        ]:
            with self.subTest(mutation=mutation):
                registry = dict(self.registry, **mutation)
                path = self.root / 'registry.json'
                path.write_text(json.dumps(registry, indent=2) + '\n')
                with self.assertRaisesRegex(AssertionError, 'stale or altered'):
                    guard.load_registry(path)
                with self.assertRaisesRegex(AssertionError, 'wrong v4 identity'):
                    guard.apply_member_delta(registry, self.previous)

    def test_delta_hashes_inventory_and_complete_manifest_are_fixed(self):
        for field in ['before_sha256', 'sha256']:
            registry = copy.deepcopy(self.registry)
            registry['member_ranking_delta'][sorted(guard.DELTA_PATHS)[0]][field] = '0' * 64
            with self.assertRaisesRegex(AssertionError, 'member (base|source) SHA'):
                guard.apply_member_delta(registry, self.previous)
        for field, path in [('member_ranking_delta', 'crates/unapproved.rs'),
                            ('complete_inputs', 'crates/unapproved.rs')]:
            registry = copy.deepcopy(self.registry)
            registry[field][path] = '0' * 64
            with self.assertRaisesRegex(AssertionError, '(delta inventory|complete manifest) differs'):
                guard.apply_member_delta(registry, self.previous)
        registry = copy.deepcopy(self.registry)
        del registry['complete_inputs']['Cargo.lock']
        with self.assertRaisesRegex(AssertionError, 'complete manifest differs'):
            guard.apply_member_delta(registry, self.previous)

    def test_arbitrary_previous_union_is_rejected(self):
        previous = dict(self.previous)
        previous['Cargo.lock'] += b'\n'
        with self.assertRaisesRegex(AssertionError, 'fixed base bytes differ'):
            guard.apply_member_delta(self.registry, previous)
        previous = dict(self.previous)
        del previous['Cargo.lock']
        with self.assertRaisesRegex(AssertionError, 'fixed base inventory differs'):
            guard.apply_member_delta(self.registry, previous)

    def test_preserved_guards_ci_and_public_rejection_conclusion(self):
        self.preserve()
        guard.verify_preserved_files(self.registry, root=self.root)
        for path in sorted(guard.PRESERVED_PATHS):
            with self.subTest(path=path):
                target = self.root / path
                original = target.read_bytes()
                target.write_bytes(original + b'\nchanged\n')
                with self.assertRaisesRegex(AssertionError, 'preserved file changed'):
                    guard.verify_preserved_files(self.registry, root=self.root)
                target.write_bytes(original)

    def test_forged_or_omitted_public_conclusion_is_rejected(self):
        self.preserve()
        for field, value in [('commit', 'HEAD'), ('sha256', '0' * 64)]:
            registry = copy.deepcopy(self.registry)
            registry['preserved_files'][guard.PUBLIC_CONCLUSION][field] = value
            with self.assertRaisesRegex(AssertionError, 'preserved (pin|SHA) differs'):
                guard.verify_preserved_files(registry, root=self.root)
        registry = copy.deepcopy(self.registry)
        del registry['preserved_files'][guard.PUBLIC_CONCLUSION]
        with self.assertRaisesRegex(AssertionError, 'preserved inventory differs'):
            guard.verify_preserved_files(registry, root=self.root)

    def test_preserved_directory_symlink_is_rejected(self):
        self.preserve()
        target = self.root / 'artifacts'
        relocated = self.root / 'moved-artifacts'
        target.rename(relocated)
        target.symlink_to(relocated, target_is_directory=True)
        with self.assertRaisesRegex(AssertionError, 'symlink input or directory'):
            guard.verify_preserved_files(self.registry, root=self.root)

    def test_explicit_cli_version_required_and_optimization_rejected(self):
        script = guard.ROOT / 'scripts/verify_current_source_v4.py'
        for args in [[], ['--source-version', 'latest'], ['--source-version', guard.v3.VERSION]]:
            result = subprocess.run([sys.executable, '-B', str(script), *args], capture_output=True)
            self.assertEqual(result.returncode, 2)
        for optimization in ['-O', '-OO']:
            result = subprocess.run([sys.executable, '-B', optimization, str(script),
                                     '--source-version', guard.VERSION], capture_output=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn(b'requires Python assertions', result.stderr)


if __name__ == '__main__':
    unittest.main()
