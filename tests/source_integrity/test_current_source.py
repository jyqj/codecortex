"""Harmless copies exercise the actual guard; no product mutation or execution."""
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / 'scripts'))
import verify_current_source as guard


class CurrentSourceTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.expected = guard.approved_union(guard.load_records())

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        for path, raw in self.expected.items():
            dest = self.root / path
            dest.parent.mkdir(parents=True, exist_ok=True)
            dest.write_bytes(raw)
        self.tracked = set(self.expected)

    def verify(self):
        guard.verify_tree(self.root, self.expected, self.tracked)

    def test_valid_exact_current(self):
        self.verify()

    def test_unknown_untracked_file(self):
        (self.root / 'crates/unknown.rs').write_text('// harmless\n')
        with self.assertRaisesRegex(AssertionError, 'disk input inventory'):
            self.verify()

    def test_unknown_tracked_file(self):
        self.tracked.add('crates/unknown.rs')
        with self.assertRaisesRegex(AssertionError, 'tracked input inventory'):
            self.verify()

    def test_changed_file(self):
        (self.root / 'crates/cc-model/src/declaration_identity.rs').write_text('// mutation\n')
        with self.assertRaisesRegex(AssertionError, 'input bytes'):
            self.verify()

    def test_changed_lock(self):
        (self.root / 'Cargo.lock').write_text('# mutation\n')
        with self.assertRaisesRegex(AssertionError, 'input bytes'):
            self.verify()

    def test_missing_file_on_disk(self):
        (self.root / 'crates/cc-parsers/src/python_identity.rs').unlink()
        with self.assertRaisesRegex(AssertionError, 'disk input inventory'):
            self.verify()

    def test_removed_tracked_file(self):
        self.tracked.remove('Cargo.toml')
        with self.assertRaisesRegex(AssertionError, 'tracked input inventory'):
            self.verify()

    def test_symlink_rejected(self):
        path = self.root / 'crates/cc-parsers/src/python_identity.rs'
        original = self.root / 'original'
        original.write_bytes(path.read_bytes())
        path.unlink()
        path.symlink_to(original)
        with self.assertRaisesRegex(AssertionError, 'symlink input'):
            self.verify()

    def reject_registry(self, mutate):
        registry = json.loads(guard.REGISTRY.read_bytes())
        mutate(registry)
        path = self.root / 'registry.json'
        path.write_text(json.dumps(registry, indent=2) + '\n')
        with self.assertRaisesRegex(AssertionError, 'stale or altered'):
            guard.load_records(path)

    def test_stale_registry(self):
        self.reject_registry(lambda r: r.update(approved_product=guard.BASE))

    def test_wrong_registry_version(self):
        self.reject_registry(lambda r: r.update(schema_version=2))

    def test_altered_manifest_pin(self):
        def mutate(r):
            next(iter(r['records'].values()))['sha256'] = '0' * 64
        self.reject_registry(mutate)

    def test_altered_source_pin(self):
        def mutate(r):
            next(iter(r['records'].values()))['commit'] = guard.PRODUCT
        self.reject_registry(mutate)

    def test_independent_declared_sha_verification(self):
        records = guard.load_records()
        manifest = json.loads(records[guard.B + 'source-guard.json'])
        next(iter(manifest['source_files'].values()))['sha256'] = '0' * 64
        records[guard.B + 'source-guard.json'] = json.dumps(manifest).encode()
        with self.assertRaisesRegex(AssertionError, 'bounded declared SHA'):
            guard.approved_union(records)

    def test_altered_lint_transformation(self):
        records = guard.load_records()
        transform = json.loads(records[guard.B + 'test-only-lint-fix/transformation.json'])
        transform['new_expression'] += ' // mutation'
        records[guard.B + 'test-only-lint-fix/transformation.json'] = json.dumps(transform).encode()
        with self.assertRaises(AssertionError):
            guard.approved_union(records)

    def test_diff_serialization_independent_of_git_abbreviation(self):
        records = guard.load_records()
        original_git = guard.git
        def long_abbreviation(*args):
            return original_git('-c', 'core.abbrev=12', *args)
        with mock.patch.object(guard, 'git', side_effect=long_abbreviation):
            self.assertEqual(guard.approved_union(records), self.expected)

    def test_altered_r1_patch(self):
        records = guard.load_records()
        records[guard.P + 'r1-migration.patch'] += b'\n# mutation\n'
        with self.assertRaisesRegex(AssertionError, 'R1 test transformation'):
            guard.approved_union(records)


if __name__ == '__main__':
    unittest.main()
