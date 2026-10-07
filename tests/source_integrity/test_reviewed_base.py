"""Independent small-model counterexamples for immutable reviewed delta bases.

Only the Git/history oracle is substituted. The production admission function,
pin validation, before/after SHA checks, on-disk review bytes, and final product
inventory/byte checks execute unchanged. This does not replace historical replay.
"""

import contextlib
import hashlib
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))
import verify_reviewed_source as guard


class ExplicitReviewedBaseTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="reviewed-base-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.name = next(iter(guard.APPROVED))
        self.new_base, self.source, self.review = "a" * 40, "b" * 40, "c" * 40
        self.changed = "crates/fixture.rs"
        self.review_path = "artifacts/checkpoints/fixture-base-review/review.json"
        self.before = {"Cargo.toml": b"workspace", "Cargo.lock": b"fixed lock", self.changed: b"before"}
        self.after = {**self.before, self.changed: b"after"}
        review = {"base": self.new_base, "source": self.source, "verdict": "accepted_scoped"}
        review_bytes = json.dumps(review).encode()
        target = self.root / self.review_path
        target.parent.mkdir(parents=True)
        target.write_bytes(review_bytes)
        self.blobs = {guard.BASE: dict(self.before), guard.PRODUCT: dict(self.after),
                      self.new_base: dict(self.before), self.source: dict(self.after),
                      self.review: {self.review_path: review_bytes}}
        self.pins = {self.name: {"base": self.new_base, "source": self.source,
                                "review": self.review, "review_path": self.review_path,
                                "paths": [self.changed]}}
        self.registry = {**guard.identities(), "deltas": {self.name: {
            "base": self.new_base, "source": self.source, "review": self.review,
            "review_path": self.review_path, "review_sha256": self.sha(review_bytes),
            "paths": {self.changed: {"before_sha256": self.sha(self.before[self.changed]),
                                     "sha256": self.sha(self.after[self.changed])}}}},
            "complete_inputs": {key: self.sha(value) for key, value in self.after.items()}}

    @staticmethod
    def sha(value):
        return hashlib.sha256(value).hexdigest()

    def verify(self):
        git = guard.previous.v2.v1
        def changed_paths(before, after):
            keys = self.blobs[before].keys() | self.blobs[after].keys()
            return {key for key in keys if self.blobs[before].get(key) != self.blobs[after].get(key)}
        with contextlib.ExitStack() as stack:
            for owner, name, value in [
                (guard, "APPROVED", self.pins),
                (guard, "verify_historical_helpers", lambda root: None),
                (guard.previous, "load_registry", lambda: {}),
                (guard.previous, "approved_union", lambda registry, root: dict(self.before)),
                (git, "ensure_refs", lambda refs: None),
                (git, "inputs", lambda ref: set(self.blobs[ref])),
                (git, "blob", lambda ref, path: self.blobs[ref][path]),
                (guard.previous.v2, "changed_paths", changed_paths),
            ]:
                stack.enter_context(patch.object(owner, name, value))
            return guard.approved_union(self.registry, root=self.root)

    def test_explicit_immutable_base_accepts_exact_reviewed_delta(self):
        self.assertEqual(self.verify(), self.after)

    def test_base_bytes_must_equal_already_accepted_input(self):
        # Registry before-hash and final product stay valid. Only the newly pinned
        # base is wrong, so removing the new base-byte comparison makes this pass.
        self.blobs[self.new_base][self.changed] = b"unaccepted base bytes"
        with self.assertRaisesRegex(AssertionError, "pinned base differs from accepted bytes"):
            self.verify()

    def test_unreviewed_other_base_bytes_are_not_imported(self):
        # The independent source branch can carry other history, but only the
        # explicitly reviewed delta is reconstructed into the final product.
        self.blobs[self.new_base]["Cargo.lock"] = b"not imported"
        self.blobs[self.source]["Cargo.lock"] = b"not imported"
        result = self.verify()
        self.assertEqual(result["Cargo.lock"], self.before["Cargo.lock"])

    def test_final_product_cannot_smuggle_unreviewed_base_bytes(self):
        self.blobs[self.new_base]["Cargo.lock"] = b"unreviewed"
        self.blobs[self.source]["Cargo.lock"] = b"unreviewed"
        self.blobs[guard.PRODUCT]["Cargo.lock"] = b"unreviewed"
        with self.assertRaisesRegex(AssertionError, "product source bytes differ"):
            self.verify()

    def test_source_diff_cannot_add_an_undeclared_path(self):
        self.blobs[self.source]["crates/hidden.rs"] = b"not reviewed"
        with self.assertRaisesRegex(AssertionError, "source delta inventory differs"):
            self.verify()


if __name__ == "__main__":
    unittest.main()
