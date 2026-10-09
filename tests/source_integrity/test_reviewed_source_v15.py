"""Exact current manifests and actual inherited proof/tamper controls for P8."""
from functools import lru_cache
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))
import verify_reviewed_source_v15 as guard
import v15_historical_context as history
import v15_historical_test_adapter as adapter

adapter.install()


class P8CompletionSourceTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cache = patch.object(guard.git, "blob", lru_cache(None)(guard.git.blob))
        cache.start()
        cls.addClassCleanup(cache.stop)
        cls.registry = guard.load_registry()
        cls.expected = guard.approved_union(cls.registry)
        cls.inherited = {p: guard.git.blob(guard.BASE, p) for p in guard.git.inputs(guard.BASE)}

    def test_only_exact_historical_review_metadata_exception_is_allowed(self):
        path = "artifacts/checkpoints/p8-corpus-external-holdout-20261008/round5/independent-source-review.json"
        self.assertEqual(history.relative(path), path)
        for invalid in (path + ".body", str(Path(path).parent / "holdout.jsonl"),
                        "/" + path, "../" + path, path.replace("/round5/", "/round5/../")):
            with self.subTest(path=invalid), self.assertRaises(AssertionError):
                history.relative(invalid)

    def test_complete_current_bytes_and_validation_are_bound(self):
        self.assertEqual(set(self.expected), guard.git.inputs(guard.PRODUCT))
        self.assertEqual({p: guard.git.sha(v) for p, v in self.expected.items()}, self.registry["complete_inputs"])
        self.assertEqual(guard.validation_inventory(guard.PRODUCT), self.registry["validation_inputs"])
        for path, data in self.expected.items():
            guard.unchanged_file(guard.ROOT, path, data, "current source")
        guard.verify_ci()

    def test_old_approval_rejection_propagates(self):
        with patch.object(guard.previous, "approved_union", side_effect=AssertionError("old approval rejected")):
            with self.assertRaisesRegex(AssertionError, "old approval rejected"):
                guard.approved_union(self.registry)

    def test_old_ci_rejection_propagates(self):
        with patch.object(guard.previous, "verify_ci", side_effect=AssertionError("old CI rejected")):
            with self.assertRaisesRegex(AssertionError, "old CI rejected"):
                guard.approved_union(self.registry)

    def test_registry_and_floating_source_pin_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "altered.json"
            path.write_bytes(guard.REGISTRY.read_bytes() + b"\n")
            with self.assertRaises(AssertionError): guard.load_registry(path)
        for value in ("HEAD", "main", "a" * 39):
            with self.subTest(value=value), patch.object(guard, "PRODUCT", value):
                with self.assertRaises(AssertionError): guard.verify_pins()

    def test_historical_context_is_real_and_current_protection_cannot_change(self):
        with history.HistoricalContext() as context:
            selected = adapter.HistoricalGuard(guard.previous, context)
            expected = selected.approved_union(selected.load_registry())
            self.assertEqual(expected, self.inherited)
            target = context.root / "scripts/verify_reviewed_source_v14.py"
            original = target.read_bytes()
            target.chmod(0o644)
            target.write_bytes(original + b"\n# changed\n")
            with self.assertRaisesRegex(AssertionError, "historical"):
                history.verify_current_history(context.root)

    def test_current_inventory_tamper_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            for path in self.registry["validation_inputs"]:
                target = root / path
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(guard.git.blob(guard.PRODUCT, path))
            review = root / guard.REVIEW_PATH
            review.parent.mkdir(parents=True, exist_ok=True)
            review.write_bytes(guard.git.blob(guard.REVIEW, guard.REVIEW_PATH))
            self.assertEqual(guard.reconstruct(self.registry, self.inherited, root), self.expected)
            added = root / "scripts/unreviewed.py"
            added.write_text("raise RuntimeError('not reviewed')\n")
            with self.assertRaisesRegex(AssertionError, "inventory"):
                guard.reconstruct(self.registry, self.inherited, root)
            added.unlink()
            target = root / "scripts/p8_runtime.py"
            target.write_bytes(target.read_bytes() + b"\n# changed\n")
            with self.assertRaisesRegex(AssertionError, "changed"):
                guard.reconstruct(self.registry, self.inherited, root)


if __name__ == "__main__":
    unittest.main()
