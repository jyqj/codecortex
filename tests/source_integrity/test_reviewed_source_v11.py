"""A reviewed fixture correction cannot admit an unrelated product or CI change."""
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
import verify_reviewed_source_v11 as guard


class WorkerFixtureReviewTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cache = patch.object(guard.git, "blob", lru_cache(None)(guard.git.blob))
        cache.start()
        cls.addClassCleanup(cache.stop)
        cls.registry = guard.load_registry()
        cls.inherited = guard.previous.approved_union(guard.previous.load_registry())
        cls.expected = guard.approved_union(cls.registry)

    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        target = self.root / guard.REVIEW_PATH
        target.parent.mkdir(parents=True)
        target.write_bytes(guard.git.blob(guard.REVIEW, guard.REVIEW_PATH))

    def verify(self, registry=None, inherited=None):
        return guard.reconstruct(self.registry if registry is None else registry,
                                 self.inherited if inherited is None else inherited, root=self.root)

    def test_single_test_delta_preserves_other_784_inputs(self):
        result = self.verify()
        self.assertEqual(result, self.expected)
        self.assertEqual(len(result), 785)
        self.assertEqual({name for name in result if result[name] != self.inherited[name]}, {guard.TEST_PATH})

    def test_stale_registry_and_floating_pins_are_rejected(self):
        target = self.root / "registry.json"
        target.write_text(json.dumps(self.registry) + " ")
        with self.assertRaisesRegex(AssertionError, "altered v11 registry"):
            guard.load_registry(target)
        for key in guard.identities():
            changed = dict(self.registry, **{key: "altered"})
            with self.subTest(key=key), self.assertRaisesRegex(AssertionError, "wrong v11 identity"):
                self.verify(changed)
        for name in ("BASE", "PRODUCT", "REVIEW", "PREVIOUS_SNAPSHOT"):
            with self.subTest(pin=name), patch.object(guard, name, "HEAD"):
                with self.assertRaisesRegex(AssertionError, "immutable full commit"):
                    guard.verify_pins()

    def test_production_and_extra_test_delta_are_rejected(self):
        for name in ("Cargo.lock", "crates/cc-eval/src/benchmark/statistics.rs", "crates/unknown.rs"):
            data = copy.deepcopy(self.registry)
            data["delta"][name] = {"before_sha256": "0" * 64, "sha256": "1" * 64}
            with self.subTest(path=name), self.assertRaisesRegex(AssertionError, "exactly the reviewed worker test"):
                self.verify(data)
        data = dict(self.registry, delta={})
        with self.assertRaisesRegex(AssertionError, "exactly the reviewed worker test"):
            self.verify(data)

    def test_before_after_and_inherited_bytes_remain_bound(self):
        for key, message in (("before_sha256", "before digest"), ("sha256", "after digest")):
            data = copy.deepcopy(self.registry)
            data["delta"][guard.TEST_PATH][key] = "0" * 64
            with self.subTest(key=key), self.assertRaisesRegex(AssertionError, message):
                self.verify(data)
        inherited = dict(self.inherited)
        inherited["Cargo.lock"] += b"\n"
        with self.assertRaisesRegex(AssertionError, "inherited bytes"):
            self.verify(inherited=inherited)

    def test_review_digest_mutation_and_symlink_are_rejected(self):
        with self.assertRaisesRegex(AssertionError, "review digest"):
            self.verify(dict(self.registry, review_sha256="0" * 64))
        target = self.root / guard.REVIEW_PATH
        original = target.read_bytes()
        target.write_bytes(original + b"\n")
        with self.assertRaisesRegex(AssertionError, "review record changed"):
            self.verify()
        copy_path = self.root / "review-copy.json"
        copy_path.write_bytes(original)
        target.unlink()
        target.symlink_to(copy_path)
        with self.assertRaisesRegex(AssertionError, "review record changed"):
            self.verify()

    def test_complete_manifest_cannot_omit_add_or_mutate(self):
        for kind in ("omit", "add", "mutate"):
            data = copy.deepcopy(self.registry)
            if kind == "omit":
                del data["complete_inputs"]["Cargo.lock"]
            else:
                data["complete_inputs"]["crates/unknown.rs" if kind == "add" else "Cargo.lock"] = "0" * 64
            with self.subTest(kind=kind), self.assertRaisesRegex(AssertionError, "complete manifest"):
                self.verify(data)

    def test_v10_snapshots_and_proof_cannot_be_bypassed(self):
        for name in guard.SNAPSHOTS:
            target = self.root / name
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(guard.git.blob(guard.PREVIOUS_SNAPSHOT, name))
        guard.verify_snapshots(self.root)
        target.write_bytes(target.read_bytes() + b"\n")
        with self.assertRaisesRegex(AssertionError, "v10 approval snapshot changed"):
            guard.verify_snapshots(self.root)
        with patch.object(guard.previous, "approved_union", side_effect=AssertionError("inherited proof rejected")):
            with self.assertRaisesRegex(AssertionError, "inherited proof rejected"):
                guard.approved_union(self.registry)

    def test_current_ci_and_every_old_step_are_preserved(self):
        guard.verify_ci()
        target = self.root / ".github/workflows/ci.yml"
        target.parent.mkdir(parents=True)
        target.write_text(guard.expected_ci())
        p7 = self.root / ".github/workflows/p7-engineering.yml"
        p7.write_bytes(guard.git.blob(guard.previous.P7_SNAPSHOT, ".github/workflows/p7-engineering.yml"))
        guard.verify_ci(self.root)
        target.write_text(guard.expected_ci().replace("cargo test --workspace", "true # cargo test --workspace", 1))
        with self.assertRaisesRegex(AssertionError, "CI differs"):
            guard.verify_ci(self.root)
        target.write_text(guard.expected_ci())
        p7.write_bytes(p7.read_bytes() + b"\n# altered\n")
        with self.assertRaisesRegex(AssertionError, "P7 engineering workflow changed"):
            guard.verify_ci(self.root)

    def test_unknown_current_source_cannot_hide_behind_fixture_review(self):
        for name, raw in self.expected.items():
            target = self.root / name
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(raw)
        guard.git.verify_tree(self.root, self.expected, set(self.expected))
        (self.root / "crates/cc-eval/src/benchmark/statistics.rs").write_text("// unrelated mutation\n")
        with self.assertRaisesRegex(AssertionError, "input bytes"):
            guard.git.verify_tree(self.root, self.expected, set(self.expected))

    def test_optimized_python_and_unknown_selection_fail_closed(self):
        script = str(guard.ROOT / "scripts/verify_reviewed_source_v11.py")
        for options, message in ((["-O", script, "--source-version", guard.VERSION], "without -O"),
                                 ([script, "--source-version", "latest"], "invalid choice")):
            result = subprocess.run([sys.executable, *options], capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn(message, result.stderr)


if __name__ == "__main__":
    unittest.main()
