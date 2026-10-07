"""Independent rejection checks for explicitly pinned development deltas."""
import copy
from functools import lru_cache
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / 'scripts'))
import verify_reviewed_source as guard


class ReviewedSourceTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        # Only immutable Git blobs are cached; current disk reads stay live.
        # The original v1/v2/v3 controls still run independently in this suite.
        cache = patch.object(guard.previous.v2.v1, 'blob',
                             lru_cache(None)(guard.previous.v2.v1.blob))
        cache.start()
        cls.addClassCleanup(cache.stop)
        cls.registry = guard.load_registry()
        cls.expected = guard.approved_union(cls.registry)

    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.root = Path(tmp.name)
        for path, raw in self.expected.items():
            dest = self.root / path
            dest.parent.mkdir(parents=True, exist_ok=True)
            dest.write_bytes(raw)
        self.tracked = set(self.expected)

    def verify(self):
        guard.previous.v2.v1.verify_tree(self.root, self.expected, self.tracked)

    def test_complete_fixed_product_matches(self):
        self.verify()
        self.assertEqual(set(self.expected), set(self.registry['complete_inputs']))
        self.assertTrue(guard.APPROVED)

    def test_current_changes_cannot_grant_themselves_approval(self):
        for path in sorted({'Cargo.lock', 'Cargo.toml'} | {
                p for pin in guard.APPROVED.values() for p in pin['paths']}):
            with self.subTest(path=path):
                target = self.root / path
                target.write_bytes(self.expected[path] + b'\n// changed\n')
                with self.assertRaisesRegex(AssertionError, 'input bytes'):
                    self.verify()
                target.write_bytes(self.expected[path])

    def test_unknown_and_missing_membership_rejected(self):
        unknown = self.root / 'crates/unknown.rs'
        unknown.write_text('// unknown\n')
        with self.assertRaisesRegex(AssertionError, 'disk input inventory'):
            self.verify()
        unknown.unlink()
        self.tracked.add('crates/unknown.rs')
        with self.assertRaisesRegex(AssertionError, 'tracked input inventory'):
            self.verify()
        self.tracked.remove('crates/unknown.rs')
        target = self.root / 'crates/cc-search/src/evidence_hydrator.rs'
        target.unlink()
        with self.assertRaises((AssertionError, FileNotFoundError)):
            self.verify()

    def test_symlink_input_rejected(self):
        path = next(iter(guard.APPROVED.values()))['paths'][0]
        target = self.root / path
        outside = self.root / 'original'
        outside.write_bytes(target.read_bytes())
        target.unlink()
        target.symlink_to(outside)
        with self.assertRaisesRegex(AssertionError, 'symlink input'):
            self.verify()

    def test_registry_digest_and_immutable_identities_rejected(self):
        for key, value in [('product_source', 'HEAD'), ('base_source', 'latest'),
                           ('source_version', 'unknown'), ('schema_version', 3)]:
            changed = copy.deepcopy(self.registry)
            changed[key] = value
            path = self.root / 'registry.json'
            path.write_text(json.dumps(changed))
            with self.assertRaisesRegex(AssertionError, 'stale or altered'):
                guard.load_registry(path)
            with self.assertRaisesRegex(AssertionError, 'wrong source identity'):
                guard.approved_union(changed)

    def test_delta_pins_paths_and_before_after_digests_rejected(self):
        for name in guard.APPROVED:
            for key in ['source', 'review']:
                changed = copy.deepcopy(self.registry)
                changed['deltas'][name][key] = 'HEAD'
                with self.subTest(delta=name, key=key), self.assertRaisesRegex(
                        AssertionError, 'wrong delta pin'):
                    guard.approved_union(changed)
            changed = copy.deepcopy(self.registry)
            changed['deltas'][name]['paths']['crates/unknown.rs'] = {}
            with self.subTest(delta=name), self.assertRaisesRegex(
                    AssertionError, 'registry delta inventory'):
                guard.approved_union(changed)
            path = guard.APPROVED[name]['paths'][0]
            for key in ['before_sha256', 'sha256']:
                changed = copy.deepcopy(self.registry)
                changed['deltas'][name]['paths'][path][key] = '0' * 64
                with self.subTest(delta=name, key=key), self.assertRaisesRegex(
                        AssertionError, 'delta (base|source) SHA'):
                    guard.approved_union(changed)

    def test_missing_delta_and_complete_manifest_rejected(self):
        for name in guard.APPROVED:
            changed = copy.deepcopy(self.registry)
            changed['deltas'].pop(name)
            with self.subTest(delta=name), self.assertRaisesRegex(
                    AssertionError, 'reviewed delta inventory'):
                guard.approved_union(changed)
        for remove in [True, False]:
            changed = copy.deepcopy(self.registry)
            if remove:
                del changed['complete_inputs']['Cargo.lock']
            else:
                changed['complete_inputs']['crates/unknown.rs'] = '0' * 64
            with self.assertRaisesRegex(AssertionError, 'complete source manifest'):
                guard.approved_union(changed)

    def test_review_digest_rejected(self):
        for name in guard.APPROVED:
            changed = copy.deepcopy(self.registry)
            changed['deltas'][name]['review_sha256'] = '0' * 64
            with self.subTest(delta=name), self.assertRaisesRegex(AssertionError, 'review digest'):
                guard.approved_union(changed)

    def test_overlapping_reviewed_deltas_rejected(self):
        names = list(guard.APPROVED)
        self.assertGreaterEqual(len(names), 2)
        changed = copy.deepcopy(guard.APPROVED)
        changed[names[1]]['paths'].append(changed[names[0]]['paths'][0])
        with patch.object(guard, 'APPROVED', changed):
            with self.assertRaisesRegex(AssertionError, 'overlapping reviewed deltas'):
                guard.approved_union(self.registry)

    def test_historical_helpers_cannot_be_rewritten_or_symlinked(self):
        for path in guard.HISTORICAL_HELPERS:
            dest = self.root / path
            dest.parent.mkdir(parents=True, exist_ok=True)
            dest.write_bytes(guard.historical_blob(path))
        guard.verify_historical_helpers(self.root)
        for path in guard.HISTORICAL_HELPERS:
            dest = self.root / path
            original = dest.read_bytes()
            dest.write_bytes(original + b'\n# changed\n')
            with self.assertRaisesRegex(AssertionError, 'historical verifier or registry'):
                guard.verify_historical_helpers(self.root)
            dest.write_bytes(original)
        dest = self.root / guard.HISTORICAL_HELPERS[0]
        original = self.root / 'original-helper'
        original.write_bytes(dest.read_bytes())
        dest.unlink()
        dest.symlink_to(original)
        with self.assertRaisesRegex(AssertionError, 'historical verifier or registry'):
            guard.verify_historical_helpers(self.root)

    def test_floating_refs_in_future_constants_fail_closed(self):
        for name, value in [('BASE', 'HEAD'), ('PRODUCT', 'latest')]:
            with patch.object(guard, name, value):
                with self.assertRaisesRegex(AssertionError, 'immutable full commit SHAs'):
                    guard.check_identity(self.registry)
        changed = copy.deepcopy(guard.APPROVED)
        changed[next(iter(changed))]['source'] = 'main'
        with patch.object(guard, 'APPROVED', changed):
            with self.assertRaisesRegex(AssertionError, 'immutable full commit SHAs'):
                guard.check_identity(self.registry)

    def test_ci_only_allows_the_explicit_selector_migration(self):
        guard.verify_ci()
        target = self.root / '.github/workflows/ci.yml'
        target.parent.mkdir(parents=True)
        target.write_text(guard.expected_ci() + '\n# unreviewed workflow edit\n')
        with self.assertRaisesRegex(AssertionError, 'CI differs'):
            guard.verify_ci(self.root)

    def test_optimized_python_cannot_disable_inherited_guards(self):
        result = subprocess.run([sys.executable, '-O', str(guard.ROOT / 'scripts/verify_reviewed_source.py'),
                                 '--source-version', guard.VERSION], capture_output=True, text=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('without -O', result.stderr)

    def test_cli_never_accepts_head_or_latest_as_version(self):
        for version in ['HEAD', 'latest']:
            result = subprocess.run([sys.executable, str(guard.ROOT / 'scripts/verify_reviewed_source.py'),
                                     '--source-version', version], capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('invalid choice', result.stderr)


if __name__ == '__main__':
    unittest.main()
