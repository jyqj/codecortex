"""Actual historical proofs, complete source and validation-domain tamper tests.

This file loads the final pinned registry; placeholders never become a passing
fixture. Immutable Git blobs alone are cached. Every working-tree observation,
old proof, current CI check and rejection predicate still executes.
"""
import copy
from contextlib import ExitStack
from functools import lru_cache
import importlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))
import v14_historical_context as history
import v14_historical_test_adapter as historical

# Discovery imports every test module before running methods. The original v13
# file and method bodies stay frozen; only omitted default roots are adapted.
# A missing or unfinalized v14 guard is an ordinary setUpClass failure, never a
# skipped test or a substituted passing registry.
historical.install()
guard = None


class OracleCompatSourceReviewTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        global guard
        guard = importlib.import_module("verify_reviewed_source_v14")
        cache = patch.object(guard.git, "blob", lru_cache(None)(guard.git.blob))
        cache.start()
        cls.addClassCleanup(cache.stop)
        cls.registry = guard.load_registry()
        # This is an actual invocation of v14 and both complete old chains.
        cls.expected = guard.approved_union(cls.registry)
        cls.inherited = {path: guard.git.blob(guard.BASE, path)
                         for path in guard.git.inputs(guard.BASE)}
        cls.review = json.loads(guard.git.blob(guard.REVIEW, guard.REVIEW_PATH))
        cls.validation = {path: guard.git.blob(guard.PRODUCT, path)
                          for path in cls.registry["validation_inputs"]}

    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.write(guard.REVIEW_PATH, guard.git.blob(guard.REVIEW, guard.REVIEW_PATH))
        for path, raw in self.validation.items():
            self.write(path, raw)

    def write(self, path, raw):
        target = self.root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(raw)
        return target

    def verify(self, registry=None, inherited=None):
        return guard.reconstruct(self.registry if registry is None else registry,
                                 self.inherited if inherited is None else inherited,
                                 root=self.root)

    def test_fixed_source_and_complete_validation_are_reconstructed(self):
        self.assertEqual(self.verify(), self.expected)
        self.assertEqual(set(self.expected), guard.git.inputs(guard.PRODUCT))
        self.assertTrue(set(self.inherited) <= set(self.expected))
        self.assertEqual(guard.BASE, history.HISTORICAL_HEAD)
        self.assertEqual(guard.previous.VERSION, "p7-closeout-source-20261008-v13")
        self.assertEqual(self.review["complete_inputs"], self.registry["complete_inputs"])
        self.assertEqual(self.registry["validation_inputs"], guard.validation_inventory(guard.PRODUCT))
        self.assertIn("tests/source_integrity/test_reviewed_source_v14.py", self.validation)
        self.assertIn("scripts/v14_historical_context.py", self.validation)
        self.assertIn("tests/source_integrity/v14_historical_test_adapter.py", self.validation)
        self.assertIn("tests/source_integrity/test_v14_historical_context.py", self.validation)
        self.assertIn("scripts/verify_reviewed_source_v13.py", self.validation)
        self.assertIn("scripts/reviewed-source-registry-v13.json", self.validation)
        self.assertIn("tests/source_integrity/test_reviewed_source_v13.py", self.validation)
        self.assertIn(".github/workflows/p7-closeout.yml", self.validation)
        self.assertEqual(guard.VALIDATION_EXCLUSIONS, frozenset({
            "scripts/verify_reviewed_source_v14.py", "scripts/reviewed-source-registry-v14.json",
            ".github/workflows/ci.yml"}))

    def test_registry_bytes_identity_and_floating_pins_fail_closed(self):
        path = self.write("registry-copy.json", guard.REGISTRY.read_bytes() + b"\n")
        with self.assertRaisesRegex(AssertionError, "altered v14 registry"):
            guard.load_registry(path)
        for key in guard.identities():
            with self.subTest(key=key), self.assertRaisesRegex(AssertionError, "wrong v14 identity"):
                self.verify(dict(self.registry, **{key: "altered"}))
        for key in ("BASE", "PRODUCT", "REVIEW"):
            for value in ("HEAD", "refs/heads/main", "a" * 39):
                with self.subTest(pin=key, value=value), patch.object(guard, key, value):
                    with self.assertRaisesRegex(AssertionError, "immutable full commit"):
                        guard.verify_pins()
        for value in ("PENDING_FIXED_REGISTRY_SHA256", "a" * 63):
            with self.subTest(registry_sha=value), patch.object(guard, "REGISTRY_SHA256", value):
                with self.assertRaises(AssertionError):
                    guard.verify_pins()

    def test_unproven_inherited_bytes_inventory_and_removed_inputs_rejected(self):
        for mutation in ("bytes", "missing", "extra"):
            altered = dict(self.inherited)
            if mutation == "bytes":
                altered["Cargo.lock"] += b"\n"
            elif mutation == "missing":
                del altered["Cargo.lock"]
            else:
                altered["crates/unreviewed.rs"] = b"// not reviewed\n"
            with self.subTest(mutation=mutation), self.assertRaisesRegex(AssertionError, "proven inherited source"):
                self.verify(inherited=altered)
        original_inputs = guard.git.inputs
        with patch.object(guard.git, "inputs", lambda ref: original_inputs(ref) - {"Cargo.lock"}
                          if ref == guard.PRODUCT else original_inputs(ref)):
            with self.assertRaisesRegex(AssertionError, "removes an inherited input"):
                self.verify()

    def test_delta_cannot_expand_disappear_or_change_before_after_digests(self):
        for mutation in ("add", "remove"):
            changed = copy.deepcopy(self.registry)
            if mutation == "add":
                changed["delta"]["crates/not_reviewed.rs"] = {"before_sha256": None, "sha256": "0" * 64}
            else:
                del changed["delta"][next(iter(changed["delta"]))]
            with self.subTest(mutation=mutation), self.assertRaisesRegex(AssertionError, "source delta inventory"):
                self.verify(changed)
        # The expected before hash for a new source input is null.
        # Check every actual added and edited path in the frozen review. A
        # future source delta without an added path must not fabricate one.
        self.assertTrue(self.registry["delta"])
        for path in self.registry["delta"]:
            for key in ("before_sha256", "sha256"):
                changed = copy.deepcopy(self.registry)
                changed["delta"][path][key] = "0" * 64
                with self.subTest(path=path, key=key), self.assertRaisesRegex(AssertionError, "before/after digest"):
                    self.verify(changed)

    def test_review_digest_bytes_and_symlink_are_mandatory(self):
        with self.assertRaisesRegex(AssertionError, "review digest"):
            self.verify(dict(self.registry, review_sha256="0" * 64))
        target = self.root / guard.REVIEW_PATH
        original = target.read_bytes()
        target.write_bytes(original + b"\n")
        with self.assertRaisesRegex(AssertionError, "fixed independent review changed"):
            self.verify()
        other = self.write("review-copy.json", original)
        target.unlink()
        target.symlink_to(other)
        with self.assertRaisesRegex(AssertionError, "fixed independent review changed"):
            self.verify()

    def test_review_cannot_omit_independence_scope_or_delta_rows(self):
        original_blob = guard.git.blob
        mutations = [(key, "altered") for key in ("source", "base", "verdict", "scope")]
        mutations += [("independent_reviewers", []), ("independent_reviewers", "not a reviewer list"),
                      ("unresolved_blockers", ["pending"]), ("paths", {})]
        for key, value in mutations:
            altered = dict(self.review, **{key: value})
            raw = json.dumps(altered).encode()
            self.write(guard.REVIEW_PATH, raw)
            registry = dict(self.registry, review_sha256=guard.git.sha(raw))

            def blob(ref, path):
                return raw if (ref, path) == (guard.REVIEW, guard.REVIEW_PATH) else original_blob(ref, path)

            with self.subTest(key=key), patch.object(guard.git, "blob", blob):
                with self.assertRaisesRegex(AssertionError, "does not accept the exact source delta"):
                    self.verify(registry)

    def test_review_before_after_and_validation_rows_cannot_be_rewritten(self):
        original_blob = guard.git.blob
        path = next(iter(self.registry["delta"]))
        for key in ("before_sha256", "sha256", "validation_inputs"):
            altered = copy.deepcopy(self.review)
            if key == "validation_inputs":
                altered[key] = {}
            else:
                altered["paths"][path][key] = "0" * 64
            raw = json.dumps(altered).encode()
            self.write(guard.REVIEW_PATH, raw)
            registry = dict(self.registry, review_sha256=guard.git.sha(raw))

            def blob(ref, name):
                return raw if (ref, name) == (guard.REVIEW, guard.REVIEW_PATH) else original_blob(ref, name)

            message = "validation review inventory" if key == "validation_inputs" else "independent source review differs"
            with self.subTest(key=key), patch.object(guard.git, "blob", blob):
                with self.assertRaisesRegex(AssertionError, message):
                    self.verify(registry)

    def test_complete_source_manifest_is_not_regenerated_from_the_checkout(self):
        for mutation in ("missing", "extra", "bytes"):
            changed = copy.deepcopy(self.registry)
            if mutation == "missing":
                del changed["complete_inputs"]["Cargo.lock"]
            else:
                changed["complete_inputs"]["crates/unknown.rs" if mutation == "extra" else "Cargo.lock"] = "0" * 64
            with self.subTest(mutation=mutation), self.assertRaisesRegex(AssertionError, "complete manifest"):
                self.verify(changed)

    def test_validation_manifest_is_complete_even_if_review_omits_the_same_path(self):
        original_blob = guard.git.blob
        path = ".github/workflows/p7-closeout.yml"
        registry = copy.deepcopy(self.registry)
        review = copy.deepcopy(self.review)
        del registry["validation_inputs"][path]
        del review["validation_inputs"][path]
        raw = json.dumps(review).encode()
        self.write(guard.REVIEW_PATH, raw)
        registry["review_sha256"] = guard.git.sha(raw)

        def blob(ref, name):
            return raw if (ref, name) == (guard.REVIEW, guard.REVIEW_PATH) else original_blob(ref, name)

        with patch.object(guard.git, "blob", blob):
            with self.assertRaisesRegex(AssertionError, "complete validation manifest"):
                self.verify(registry)

    def test_added_script_workflow_or_test_is_detected_on_disk(self):
        for path in ("scripts/unreviewed.py", ".github/workflows/unreviewed.yml",
                     "tests/source_integrity/test_unreviewed.py", "scripts/__pycache__/hidden_source.py"):
            target = self.write(path, b"# actual unreviewed source\n")
            with self.subTest(path=path), self.assertRaisesRegex(AssertionError, "validation disk inventory"):
                self.verify()
            target.unlink()
        self.write("scripts/__pycache__/generated.cpython-312.pyc", b"generated bytecode only")
        self.assertEqual(self.verify(), self.expected)

    def test_validation_deletion_edit_and_same_byte_symlink_fail(self):
        paths = ("scripts/p8_compat.py", "scripts/v14_historical_context.py",
                 ".github/workflows/p7-closeout.yml",
                 "tests/source_integrity/v14_historical_test_adapter.py",
                 "tests/source_integrity/test_v14_historical_context.py",
                 "tests/source_integrity/test_reviewed_source_v14.py")
        for path in paths:
            target = self.root / path
            original = target.read_bytes()
            target.write_bytes(original + b"\n# unreviewed\n")
            with self.subTest(path=path, mutation="edit"), self.assertRaisesRegex(AssertionError, "reviewed validation input changed"):
                self.verify()
            target.unlink()
            with self.subTest(path=path, mutation="remove"), self.assertRaisesRegex(AssertionError, "validation disk inventory"):
                self.verify()
            other = self.write("same-byte-copy", original)
            target.symlink_to(other)
            with self.subTest(path=path, mutation="symlink"), self.assertRaisesRegex(AssertionError, "reviewed validation input changed"):
                self.verify()
            target.unlink()
            target.write_bytes(original)

    def test_source_tree_tracks_new_missing_changed_and_symlink_inputs(self):
        for path, raw in self.expected.items():
            self.write(path, raw)
        guard.git.verify_tree(self.root, self.expected, set(self.expected))
        path = next(path for path in self.registry["delta"] if path.startswith("crates/"))
        target = self.root / path
        target.write_bytes(self.expected[path] + b"\n// unreviewed\n")
        with self.assertRaisesRegex(AssertionError, "input bytes"):
            guard.git.verify_tree(self.root, self.expected, set(self.expected))
        target.write_bytes(self.expected[path])
        unknown = self.write("crates/unreviewed.rs", b"// unreviewed\n")
        with self.assertRaisesRegex(AssertionError, "disk input inventory"):
            guard.git.verify_tree(self.root, self.expected, set(self.expected))
        with self.assertRaisesRegex(AssertionError, "tracked input inventory"):
            guard.git.verify_tree(self.root, self.expected, set(self.expected) | {"crates/unreviewed.rs"})
        unknown.unlink()
        target.unlink()
        with self.assertRaisesRegex(AssertionError, "disk input inventory"):
            guard.git.verify_tree(self.root, self.expected, set(self.expected))
        other = self.write("source-copy", self.expected[path])
        target.symlink_to(other)
        with self.assertRaisesRegex(AssertionError, "symlink input"):
            guard.git.verify_tree(self.root, self.expected, set(self.expected))

    def test_old_protection_bytes_cannot_hide_behind_the_historical_context(self):
        blobs, protected = history.catalogue()
        for path in protected:
            if not (self.root / path).exists():
                self.write(path, blobs[path])
        guard.verify_snapshots(self.root)
        paths = (*guard.FROZEN, guard.previous.REVIEW_PATH,
                 "tests/source_integrity/v13_historical_ci_adapter.py",
                 "scripts/source_snapshots/v12_ci/.github/workflows/ci.yml")
        for path in paths:
            target = self.root / path
            original = target.read_bytes()
            for mutation in ("edit", "delete", "symlink"):
                target.unlink()
                if mutation == "edit":
                    target.write_bytes(original + b"\n# changed history\n")
                elif mutation == "symlink":
                    target.symlink_to(self.write("protected-copy", original))
                try:
                    with self.subTest(path=path, mutation=mutation), self.assertRaises(AssertionError):
                        guard.verify_snapshots(self.root)
                    with self.subTest(path=path, mutation=mutation, full_guard=True), self.assertRaises(AssertionError):
                        guard.approved_union(self.registry, root=self.root)
                finally:
                    if target.exists() or target.is_symlink():
                        target.unlink()
                    target.write_bytes(original)
        # A pristine materialized history remains usable after every rejected
        # current-tree mutation; it does not authorize those mutations.
        with history.HistoricalContext(self.root) as context:
            context.verify()

    def test_every_historical_approval_function_executes_and_can_reject(self):
        modules = {}

        def visit(module):
            if module.__name__ in modules:
                return
            modules[module.__name__] = module
            for name in ("previous", "joint", "p7", "p8", "v1", "v2"):
                child = getattr(module, name, None)
                if getattr(child, "__name__", "").startswith("verify_"):
                    visit(child)

        visit(guard.previous)
        self.assertIn("verify_reviewed_source_v13", modules)
        self.assertIn("verify_reviewed_source_joint22_v10", modules)
        proofs = {name: module for name, module in modules.items() if hasattr(module, "approved_union")}
        with ExitStack() as contexts:
            helper = contexts.enter_context(patch.object(guard.history, "approved_union", wraps=guard.history.approved_union))
            old_ci = contexts.enter_context(patch.object(guard.previous, "verify_ci", wraps=guard.previous.verify_ci))
            spies = {name: contexts.enter_context(patch.object(module, "approved_union", wraps=module.approved_union))
                     for name, module in proofs.items()}
            self.assertEqual(guard.approved_union(self.registry), self.expected)
            self.assertTrue(all(spy.call_count > 0 for spy in spies.values()))
            self.assertGreater(helper.call_count, 0)
            self.assertGreater(old_ci.call_count, 0)
        for name, module in proofs.items():
            with self.subTest(chain=name), patch.object(module, "approved_union", side_effect=AssertionError("real prior proof rejected")):
                with self.assertRaisesRegex(AssertionError, "real prior proof rejected"):
                    guard.approved_union(self.registry)

    def test_current_ci_and_historical_proof_steps_cannot_drift(self):
        target = self.write(".github/workflows/ci.yml", guard.expected_ci().encode())
        guard.verify_ci(self.root)
        expected = target.read_text()
        changes = (
            ("cargo test --workspace", "true # cargo test --workspace"),
            ("python3 scripts/verify_historical_integrations_v2.py", "true # skip old proof"),
            ("verify_reviewed_source_v14.py --source-version " + guard.VERSION,
             "verify_reviewed_source_v13.py --source-version " + guard.previous.VERSION),
        )
        for before, after in changes:
            self.assertIn(before, expected)
            target.write_text(expected.replace(before, after, 1))
            with self.subTest(step=before), self.assertRaisesRegex(AssertionError, "CI differs"):
                guard.verify_ci(self.root)
        target.write_text(expected)
        same_bytes = self.write("ci-copy.yml", target.read_bytes())
        target.unlink()
        target.symlink_to(same_bytes)
        with self.assertRaisesRegex(AssertionError, "CI differs"):
            guard.verify_ci(self.root)
        target.unlink()
        target.write_text(expected)
        p7 = self.root / ".github/workflows/p7-engineering.yml"
        p7.write_bytes(p7.read_bytes() + b"\n# altered\n")
        with self.assertRaisesRegex(AssertionError, "P7 engineering workflow changed"):
            guard.verify_ci(self.root)

    def test_actual_v13_ci_and_helper_rejections_reach_the_new_guard(self):
        for module, method in ((guard.previous, "verify_ci"), (guard.history, "approved_union")):
            with self.subTest(method=method), patch.object(module, method, side_effect=AssertionError("real history rejected")):
                with self.assertRaisesRegex(AssertionError, "real history rejected"):
                    guard.approved_union(self.registry)

    def test_new_review_must_bind_the_complete_source_manifest(self):
        original_blob = guard.git.blob
        for mutation in ("missing_manifest", "missing_path", "extra_path", "bytes"):
            altered = copy.deepcopy(self.review)
            if mutation == "missing_manifest":
                del altered["complete_inputs"]
            elif mutation == "missing_path":
                del altered["complete_inputs"]["Cargo.lock"]
            else:
                path = "crates/unreviewed.rs" if mutation == "extra_path" else "Cargo.lock"
                altered["complete_inputs"][path] = "0" * 64
            raw = json.dumps(altered).encode()
            self.write(guard.REVIEW_PATH, raw)
            registry = dict(self.registry, review_sha256=guard.git.sha(raw))

            def blob(ref, path):
                return raw if (ref, path) == (guard.REVIEW, guard.REVIEW_PATH) else original_blob(ref, path)

            with self.subTest(mutation=mutation), patch.object(guard.git, "blob", blob):
                with self.assertRaises(AssertionError):
                    self.verify(registry)

    def test_parent_directory_symlinks_cannot_supply_unchanged_validation_or_review(self):
        for path in ("scripts", "tests/source_integrity", str(Path(guard.REVIEW_PATH).parent)):
            parent = self.root / path
            temporary = tempfile.TemporaryDirectory()
            self.addCleanup(temporary.cleanup)
            moved = Path(temporary.name) / "same-byte-parent"
            parent.rename(moved)
            try:
                parent.symlink_to(moved, target_is_directory=True)
                with self.subTest(parent=path), self.assertRaises(AssertionError):
                    self.verify()
            finally:
                parent.unlink()
                moved.rename(parent)

    def test_validation_is_checked_before_any_historical_approval(self):
        # A changed helper or adapter is a current validation change. It must
        # reject before that helper is allowed to execute a historical proof.
        guard.verify_validation_inputs(self.root, guard.validation_inventory(guard.PRODUCT))
        for path in ("scripts/v14_historical_context.py", "tests/source_integrity/v14_historical_test_adapter.py"):
            target = self.root / path
            original = target.read_bytes()
            target.write_bytes(original + b"\n# unreviewed validation\n")
            try:
                with self.subTest(path=path), patch.object(guard.history, "approved_union", wraps=guard.history.approved_union) as called:
                    with self.assertRaisesRegex(AssertionError, "reviewed validation input changed") as rejected:
                        guard.approved_union(self.registry, root=self.root)
                    self.assertIn(path, str(rejected.exception))
                    called.assert_not_called()
            finally:
                target.write_bytes(original)

    def test_installed_old_guard_keeps_explicit_ci_fixture_rejections(self):
        module = importlib.import_module("test_reviewed_source_v13")
        self.assertIsInstance(module.guard, historical.HistoricalGuard)
        self.assertIs(module.guard._real, guard.previous)
        self.assertIsNot(module.guard, guard.previous)
        with patch.object(guard.previous, "verify_ci", wraps=guard.previous.verify_ci) as called:
            module.guard.verify_ci()
            called.assert_called_once_with(root=module.guard.ROOT)
        for path in (".github/workflows/ci.yml", ".github/workflows/p7-engineering.yml"):
            self.write(path, history.fixed_blob(path))
        module.guard.verify_ci(self.root)
        self.write(".github/workflows/ci.yml", b"name: unreviewed\n")
        for args, kwargs in (((self.root,), {}), ((), {"root": self.root})):
            with self.assertRaisesRegex(AssertionError, "CI differs"):
                module.guard.verify_ci(*args, **kwargs)

    def test_optimized_python_and_implicit_or_floating_selectors_fail(self):
        script = str(guard.ROOT / "scripts/verify_reviewed_source_v14.py")
        for arguments, message in ((["-O", script, "--source-version", guard.VERSION], "without -O"),
                                   ([script, "--source-version", "latest"], "invalid choice"),
                                   ([script], "required")):
            result = subprocess.run([sys.executable, "-B", *arguments], capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn(message, result.stderr)


if __name__ == "__main__":
    unittest.main()
