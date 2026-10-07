"""Harmless fresh copies test the selected complete capture union and its pins."""
import copy
import json
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / 'scripts'))
import verify_current_source_v2 as guard


class CaptureSourceTests(unittest.TestCase):
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
        guard.v1.verify_tree(self.root, self.expected, self.tracked)

    def test_complete_union_passes(self):
        self.verify()
        self.assertEqual(set(self.expected), set(self.registry['complete_inputs']))

    def test_unknown_disk_addition(self):
        (self.root / 'crates/unknown.rs').write_text('// unknown\n')
        with self.assertRaisesRegex(AssertionError, 'disk input inventory'):
            self.verify()

    def test_unknown_tracked_addition(self):
        self.tracked.add('crates/unknown.rs')
        with self.assertRaisesRegex(AssertionError, 'tracked input inventory'):
            self.verify()

    def test_each_capture_and_cargo_mutation(self):
        for path in sorted(guard.CAPTURE_PATHS | {guard.FIXTURE, 'Cargo.toml', 'Cargo.lock'}):
            with self.subTest(path=path):
                dest = self.root / path
                dest.write_bytes(self.expected[path] + b'\n# mutation\n')
                with self.assertRaisesRegex(AssertionError, 'input bytes'):
                    self.verify()
                dest.write_bytes(self.expected[path])

    def test_each_capture_and_cargo_disk_removal(self):
        for path in sorted(guard.CAPTURE_PATHS | {guard.FIXTURE, 'Cargo.toml', 'Cargo.lock'}):
            with self.subTest(path=path):
                dest = self.root / path
                dest.unlink()
                with self.assertRaises((AssertionError, FileNotFoundError)):
                    self.verify()
                dest.write_bytes(self.expected[path])

    def test_each_capture_and_cargo_tracked_removal(self):
        for path in sorted(guard.CAPTURE_PATHS | {guard.FIXTURE, 'Cargo.toml', 'Cargo.lock'}):
            with self.subTest(path=path):
                self.tracked.remove(path)
                with self.assertRaisesRegex(AssertionError, 'tracked input inventory'):
                    self.verify()
                self.tracked.add(path)

    def test_old_source_mutation(self):
        (self.root / 'crates/cc-model/src/declaration_identity.rs').write_text('// mutation\n')
        with self.assertRaisesRegex(AssertionError, 'input bytes'):
            self.verify()

    def test_capture_symlink_rejected(self):
        dest = self.root / guard.FIXTURE
        original = self.root / 'original'
        original.write_bytes(dest.read_bytes())
        dest.unlink()
        dest.symlink_to(original)
        with self.assertRaisesRegex(AssertionError, 'symlink input'):
            self.verify()

    def test_registry_cannot_self_authorize_head_or_unknown_version(self):
        for mutation in (
            {'fixed_product': 'HEAD'}, {'source_version': 'latest'},
            {'approved_capture_source': guard.ORIGINAL}, {'schema_version': 1},
        ):
            with self.subTest(mutation=mutation):
                r = dict(self.registry, **mutation)
                path = self.root / 'registry.json'
                path.write_text(json.dumps(r, indent=2) + '\n')
                with self.assertRaisesRegex(AssertionError, 'stale or altered'):
                    guard.load_registry(path)

    def test_independent_capture_hash_check(self):
        r = copy.deepcopy(self.registry)
        r['capture_delta'][guard.MODULE]['sha256'] = '0' * 64
        with self.assertRaisesRegex(AssertionError, 'capture SHA'):
            guard.approved_union(r)

    def test_complete_manifest_cannot_omit_unknown_or_known_input(self):
        for path, add in [('crates/unknown.rs', True), ('Cargo.lock', False)]:
            r = copy.deepcopy(self.registry)
            if add:
                r['complete_inputs'][path] = '0' * 64
            else:
                del r['complete_inputs'][path]
            with self.assertRaisesRegex(AssertionError, 'complete manifest differs'):
                guard.approved_union(r)

    def test_fixture_pin_cannot_follow_head(self):
        r = copy.deepcopy(self.registry)
        r['integration_fixtures'][guard.FIXTURE]['commit'] = 'HEAD'
        with self.assertRaises(AssertionError):
            guard.approved_union(r)

    def test_review_contract_digest_checked(self):
        r = copy.deepcopy(self.registry)
        path = 'docs/internals/python-inventory-capture-v1.md'
        r['records'][path]['sha256'] = '0' * 64
        with self.assertRaisesRegex(AssertionError, 'review/contract SHA'):
            guard.approved_union(r)

    def test_ci_selects_explicit_reviewed_version_and_keeps_historical_p0(self):
        ci = (guard.ROOT / '.github/workflows/ci.yml').read_text()
        self.assertIn('verify_current_source_v3.py --source-version query-owner-context-20261004-v3', ci)
        self.assertIn('scripts/p0_historical_corpus.py --validator target/debug/cc-eval', ci)
        self.assertNotIn('python3 scripts/verify_current_source.py\n', ci)


if __name__ == '__main__':
    unittest.main()
