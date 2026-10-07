"""Reject unreviewed integration, drift, and loss of either historical proof."""
import copy
from functools import lru_cache
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))
import verify_reviewed_source_v10 as guard


class SequentialReviewTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cache = patch.object(guard.git, "blob", lru_cache(None)(guard.git.blob))
        cache.start()
        cls.addClassCleanup(cache.stop)
        cls.registry = guard.load_registry()
        cls.p8 = guard.p8.approved_union(guard.p8.load_registry())
        cls.p7 = guard.p7.approved_union(guard.p7.load_registry(guard.P7_REGISTRY))
        cls.expected = guard.approved_union(cls.registry)

    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        path = self.root / guard.REVIEW_PATH
        path.parent.mkdir(parents=True)
        path.write_bytes(guard.git.blob(guard.REVIEW, guard.REVIEW_PATH))

    def verify(self, registry=None, p8=None, p7=None):
        return guard.reconstruct(registry if registry is not None else self.registry,
                                 p8 if p8 is not None else self.p8,
                                 p7 if p7 is not None else self.p7, root=self.root)

    def test_combined_product_preserves_all_785_inputs(self):
        self.assertEqual(self.verify(), self.expected)
        self.assertEqual(len(self.expected), 785)
        self.assertEqual(set(self.registry["delta"]), set(json.loads(
            (self.root / guard.REVIEW_PATH).read_bytes())["paths"]))

    def test_registry_tampering_and_floating_identity_are_rejected(self):
        target = self.root / "registry.json"
        target.write_text(json.dumps(self.registry) + " ")
        with self.assertRaisesRegex(AssertionError, "altered v10 registry"):
            guard.load_registry(target)
        for key in guard.identities():
            data = copy.deepcopy(self.registry)
            data[key] = "not-the-fixed-identity"
            with self.subTest(key=key), self.assertRaisesRegex(AssertionError, "wrong v10 identity"):
                self.verify(data)
        for name in ["BASE", "PRODUCT", "REVIEW", "P7_SNAPSHOT", "P8_SNAPSHOT"]:
            with self.subTest(pin=name), patch.object(guard, name, "HEAD"):
                with self.assertRaisesRegex(AssertionError, "immutable full commit SHAs"):
                    guard.verify_pins()

    def test_every_delta_before_and_after_is_checked(self):
        for path in self.registry["delta"]:
            for key in ["before_sha256", "sha256"]:
                data = copy.deepcopy(self.registry)
                data["delta"][path][key] = "0" * 64
                with self.subTest(path=path, key=key), self.assertRaisesRegex(
                        AssertionError, "sequential delta (before|source) digest"):
                    self.verify(data)

    def test_omitted_or_self_added_delta_cannot_change_the_inventory(self):
        for add in [True, False]:
            data = copy.deepcopy(self.registry)
            if add:
                data["delta"]["crates/unreviewed.rs"] = {"sha256": "0" * 64}
            else:
                data["delta"].pop(next(iter(data["delta"])))
            with self.assertRaisesRegex(AssertionError, "delta inventory"):
                self.verify(data)

    def test_p8_approved_base_cannot_be_replaced(self):
        changed = dict(self.p8)
        changed["Cargo.lock"] += b"\n"
        with self.assertRaisesRegex(AssertionError, "approved P8 bytes"):
            self.verify(p8=changed)
        changed.pop("Cargo.lock")
        with self.assertRaisesRegex(AssertionError, "approved P8 bytes"):
            self.verify(p8=changed)

    def test_independent_review_is_pinned_and_cannot_be_symlinked(self):
        data = copy.deepcopy(self.registry)
        data["review_sha256"] = "0" * 64
        with self.assertRaisesRegex(AssertionError, "review digest"):
            self.verify(data)
        path = self.root / guard.REVIEW_PATH
        raw = path.read_bytes()
        path.write_bytes(raw + b"\n")
        with self.assertRaisesRegex(AssertionError, "review record changed"):
            self.verify()
        outside = self.root / "review-copy.json"
        outside.write_bytes(raw)
        path.unlink()
        path.symlink_to(outside)
        with self.assertRaisesRegex(AssertionError, "review record changed"):
            self.verify()

    def test_p7_adoption_and_the_exact_override_are_required(self):
        changed = dict(self.p7)
        changed["crates/cc-semantic/src/gc.rs"] += b"\n"
        with self.assertRaisesRegex(AssertionError, "P7 adopted bytes differ"):
            self.verify(p7=changed)
        for overrides in [[], sorted(guard.P7_OVERRIDES) + ["crates/cc-semantic/src/gc.rs"]]:
            data = copy.deepcopy(self.registry)
            data["p7_reviewed_overrides"] = overrides
            with self.assertRaisesRegex(AssertionError, "P7 override inventory"):
                self.verify(data)

    def test_complete_manifest_cannot_hide_or_add_inputs(self):
        for change in ["remove", "add", "alter"]:
            data = copy.deepcopy(self.registry)
            if change == "remove":
                del data["complete_inputs"]["Cargo.lock"]
            elif change == "add":
                data["complete_inputs"]["crates/unreviewed.rs"] = "0" * 64
            else:
                data["complete_inputs"]["Cargo.lock"] = "0" * 64
            with self.assertRaisesRegex(AssertionError, "complete manifest"):
                self.verify(data)

    def test_both_historical_approval_snapshots_remain_original(self):
        for path, (ref, original) in guard.SNAPSHOTS.items():
            target = self.root / path
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(guard.git.blob(ref, original))
        guard.verify_snapshots(self.root)
        for path in guard.SNAPSHOTS:
            target = self.root / path
            raw = target.read_bytes()
            target.write_bytes(raw + b"\n")
            with self.subTest(path=path), self.assertRaisesRegex(AssertionError, "snapshot changed"):
                guard.verify_snapshots(self.root)
            target.write_bytes(raw)

    def test_current_workflows_have_only_the_explicit_migration(self):
        import verify_reviewed_source_v12 as selected
        selected.verify_ci()
        path = self.root / ".github/workflows/ci.yml"
        path.parent.mkdir(parents=True)
        guard.git.ensure_refs(["48efa5a6058005fe141d7a835ea9f127db9bd28b"])
        path.write_bytes(guard.git.blob("48efa5a6058005fe141d7a835ea9f127db9bd28b", ".github/workflows/ci.yml"))
        self.assertEqual(path.read_text(), guard.expected_ci())
        p7 = self.root / ".github/workflows/p7-engineering.yml"
        p7.write_bytes(guard.git.blob(guard.P7_SNAPSHOT, ".github/workflows/p7-engineering.yml"))
        guard.verify_ci(self.root)
        path.write_text(guard.expected_ci().replace("cargo test --workspace", "true # cargo test --workspace", 1))
        with self.assertRaisesRegex(AssertionError, "CI differs"):
            guard.verify_ci(self.root)
        path.write_text(guard.expected_ci())
        p7.write_bytes(p7.read_bytes() + b"\n# changed\n")
        with self.assertRaisesRegex(AssertionError, "P7 engineering workflow changed"):
            guard.verify_ci(self.root)

    def test_current_tree_must_match_the_independent_fixed_source(self):
        for path, raw in self.expected.items():
            target = self.root / path
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(raw)
        guard.git.verify_tree(self.root, self.expected, set(self.expected))
        (self.root / "crates/cc-eval/src/bin/cc-eval.rs").write_text("// changed merged CLI\n")
        with self.assertRaisesRegex(AssertionError, "input bytes"):
            guard.git.verify_tree(self.root, self.expected, set(self.expected))

    def test_optimized_python_and_floating_version_are_rejected(self):
        script = str(guard.ROOT / "scripts/verify_reviewed_source_v10.py")
        result = subprocess.run([sys.executable, "-O", script, "--source-version", guard.VERSION],
                                capture_output=True, text=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("without -O", result.stderr)
        result = subprocess.run([sys.executable, script, "--source-version", "latest"],
                                capture_output=True, text=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("invalid choice", result.stderr)


if __name__ == "__main__":
    unittest.main()
