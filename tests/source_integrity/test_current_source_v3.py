"""Bounded negative controls for the exact accepted owner-context registry."""
import copy
import json
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / 'scripts'))
import verify_current_source_v3 as guard


class OwnerSourceTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.registry = guard.load_registry()
        cls.expected = guard.approved_union(cls.registry)

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
        guard.v2.v1.verify_tree(self.root, self.expected, self.tracked)

    def test_complete_union_passes(self):
        self.verify()
        self.assertEqual(len(self.expected), 764)

    def test_unknown_disk_and_tracked_additions(self):
        dest = self.root / 'crates/unapproved.rs'
        dest.write_text('// unapproved\n')
        with self.assertRaisesRegex(AssertionError, 'disk input inventory'):
            self.verify()
        dest.unlink()
        self.tracked.add('crates/unapproved.rs')
        with self.assertRaisesRegex(AssertionError, 'tracked input inventory'):
            self.verify()

    def test_owner_cargo_and_previous_capture_mutations(self):
        paths = guard.DELTA_PATHS | guard.v2.CAPTURE_PATHS | {'Cargo.lock', 'Cargo.toml'}
        for path in sorted(paths):
            with self.subTest(path=path):
                dest = self.root / path
                dest.write_bytes(self.expected[path] + b'\n// unapproved\n')
                with self.assertRaisesRegex(AssertionError, 'input bytes'):
                    self.verify()
                dest.write_bytes(self.expected[path])

    def test_owner_cargo_removals(self):
        for path in sorted(guard.DELTA_PATHS | {'Cargo.lock', 'Cargo.toml'}):
            with self.subTest(path=path):
                dest = self.root / path
                dest.unlink()
                with self.assertRaises((AssertionError, FileNotFoundError)):
                    self.verify()
                dest.write_bytes(self.expected[path])
                self.tracked.remove(path)
                with self.assertRaisesRegex(AssertionError, 'tracked input inventory'):
                    self.verify()
                self.tracked.add(path)

    def test_owner_symlink_rejected(self):
        path = sorted(guard.DELTA_PATHS)[0]
        dest = self.root / path
        original = self.root / 'original'
        original.write_bytes(dest.read_bytes())
        dest.unlink()
        dest.symlink_to(original)
        with self.assertRaisesRegex(AssertionError, 'symlink input'):
            self.verify()

    def test_registry_cannot_follow_head_latest_or_old_version(self):
        for mutation in (
            {'accepted_source': 'HEAD'}, {'accepted_delivery': 'latest'},
            {'independent_acceptance': 'HEAD'}, {'approved_base': 'HEAD'},
            {'source_version': guard.v2.VERSION}, {'schema_version': 2},
        ):
            with self.subTest(mutation=mutation):
                path = self.root / 'registry.json'
                path.write_text(json.dumps(dict(self.registry, **mutation), indent=2) + '\n')
                with self.assertRaisesRegex(AssertionError, 'stale or altered'):
                    guard.load_registry(path)
                with self.assertRaisesRegex(AssertionError, 'wrong v3 identity'):
                    guard.approved_union(dict(self.registry, **mutation))

    def test_delta_hash_and_inventory_cannot_authorize_new_code(self):
        for field in ['before_sha256', 'sha256']:
            r = copy.deepcopy(self.registry)
            r['owner_context_delta'][sorted(guard.DELTA_PATHS)[0]][field] = '0' * 64
            with self.assertRaisesRegex(AssertionError, 'owner (base|source) SHA'):
                guard.approved_union(r)
        r = copy.deepcopy(self.registry)
        r['owner_context_delta']['crates/unapproved.rs'] = {}
        with self.assertRaisesRegex(AssertionError, 'owner delta inventory'):
            guard.approved_union(r)

    def test_complete_manifest_addition_and_omission_rejected(self):
        for add in [True, False]:
            r = copy.deepcopy(self.registry)
            if add:
                r['complete_inputs']['crates/unapproved.rs'] = '0' * 64
            else:
                del r['complete_inputs']['Cargo.lock']
            with self.assertRaisesRegex(AssertionError, 'complete manifest differs'):
                guard.approved_union(r)

    def test_history_hash_pin_and_omission_rejected(self):
        path = 'artifacts/reviews/query-owner-closing-qualifiers-independent-20261004/binding.json'
        for field, value in [('sha256', '0' * 64), ('commit', 'HEAD')]:
            r = copy.deepcopy(self.registry)
            r['records'][path][field] = value
            with self.assertRaisesRegex(AssertionError, 'owner history (SHA|pin)'):
                guard.approved_union(r)
        r = copy.deepcopy(self.registry)
        del r['records'][path]
        with self.assertRaisesRegex(AssertionError, 'owner history inventory'):
            guard.approved_union(r)

    def test_ci_selector_only_change_and_historical_p0_preserved(self):
        import verify_reviewed_source as current
        import verify_reviewed_source_v11 as selected
        original = guard.v2.v1.blob(guard.BASE, '.github/workflows/ci.yml').decode()
        expected = original.replace(
            'verify_current_source_v2.py --source-version python-inventory-20261004-v2',
            'verify_current_source_v3.py --source-version ' + guard.VERSION).replace(
                'explicitly selected v2 capture union',
                'explicitly selected v3 accepted owner-context union')
        target = self.root / '.github/workflows/ci.yml'
        target.parent.mkdir(parents=True)
        guard.v2.v1.ensure_refs(['ae906513886bef4501d0a1d2ecffd68ef76c3f5d'])
        target.write_bytes(guard.v2.v1.blob(
            'ae906513886bef4501d0a1d2ecffd68ef76c3f5d', '.github/workflows/ci.yml'))
        current.verify_ci(self.root)
        selected.verify_ci()
        self.assertEqual(guard.v2.v1.blob(current.BASE, '.github/workflows/ci.yml').decode(), expected)
        for path in ['scripts/current-source-registry-v1.json',
                     'scripts/current-source-registry-v2.json',
                     'scripts/verify_current_source.py', 'scripts/verify_current_source_v2.py',
                     'scripts/p0_historical_corpus.py']:
            self.assertEqual((guard.ROOT / path).read_bytes(), guard.v2.v1.blob(guard.BASE, path))


if __name__ == '__main__':
    unittest.main()
