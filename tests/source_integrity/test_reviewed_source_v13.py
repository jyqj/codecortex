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
import verify_reviewed_source_v13 as guard
import v13_historical_ci_adapter as historical

# unittest discovery loads all modules before running any methods. Only the
# four exact historical methods are adapted; their original files stay frozen.
historical.install(guard)


class CompleteSourceReviewTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cache = patch.object(guard.git, "blob", lru_cache(None)(guard.git.blob))
        cache.start()
        cls.addClassCleanup(cache.stop)
        cls.registry = guard.load_registry()
        # This is an actual invocation of v13 and both complete old chains.
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
        self.assertGreater(len(self.expected), len(self.inherited))
        self.assertEqual(self.registry["validation_inputs"], guard.validation_inventory(guard.PRODUCT))
        self.assertIn("tests/source_integrity/test_reviewed_source_v13.py", self.validation)
        self.assertIn("tests/source_integrity/v13_historical_ci_adapter.py", self.validation)
        self.assertIn(".github/workflows/p7-closeout.yml", self.validation)
        self.assertEqual(guard.VALIDATION_EXCLUSIONS, frozenset({
            "scripts/verify_reviewed_source_v13.py", "scripts/reviewed-source-registry-v13.json",
            ".github/workflows/ci.yml"}))

    def test_registry_bytes_identity_and_floating_pins_fail_closed(self):
        path = self.write("registry-copy.json", guard.REGISTRY.read_bytes() + b"\n")
        with self.assertRaisesRegex(AssertionError, "altered v13 registry"):
            guard.load_registry(path)
        for key in guard.identities():
            with self.subTest(key=key), self.assertRaisesRegex(AssertionError, "wrong v13 identity"):
                self.verify(dict(self.registry, **{key: "altered"}))
        for key in ("BASE", "PRODUCT", "REVIEW"):
            for value in ("HEAD", "refs/heads/main", "a" * 39):
                with self.subTest(pin=key, value=value), patch.object(guard, key, value):
                    with self.assertRaisesRegex(AssertionError, "immutable full commit"):
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
        # Exercise an added input and an existing edited input, not just one
        # arbitrary row. The expected before hash for new source is null.
        added = next(path for path, row in self.registry["delta"].items() if row["before_sha256"] is None)
        edited = next(path for path, row in self.registry["delta"].items() if row["before_sha256"] is not None)
        for path in (added, edited):
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
        mutations = [(key, "altered") for key in ("source", "base", "verdict")]
        mutations += [("independent_reviewers", []), ("unresolved_blockers", ["pending"]), ("paths", {})]
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
        paths = ("scripts/p7_mechanism_build.py", ".github/workflows/p7-closeout.yml",
                 "tests/source_integrity/test_reviewed_source_v13.py")
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

    def test_v12_source_tests_and_both_ci_snapshot_files_are_byte_frozen(self):
        guard.verify_snapshots(self.root)
        paths = [*guard.FROZEN, *(guard.CI_SNAPSHOT + "/" + path for path in
                  (".github/workflows/ci.yml", ".github/workflows/p7-engineering.yml"))]
        for path in paths:
            target = self.root / path
            original = target.read_bytes()
            target.write_bytes(original + b"\n# changed history\n")
            with self.subTest(path=path), self.assertRaisesRegex(AssertionError, "snapshot changed"):
                guard.verify_snapshots(self.root)
            target.write_bytes(original)
        with patch.object(guard.previous, "verify_ci", side_effect=AssertionError("historical CI actually rejected")):
            with self.assertRaisesRegex(AssertionError, "historical CI actually rejected"):
                guard.verify_snapshots(self.root)

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
        proofs = {name: module for name, module in modules.items() if hasattr(module, "approved_union")}
        with ExitStack() as contexts:
            spies = {name: contexts.enter_context(patch.object(module, "approved_union", wraps=module.approved_union))
                     for name, module in proofs.items()}
            self.assertEqual(guard.approved_union(self.registry), self.expected)
            self.assertTrue(all(spy.call_count > 0 for spy in spies.values()))
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
            ("verify_reviewed_source_v13.py --source-version " + guard.VERSION,
             "verify_reviewed_source_v12.py --source-version " + guard.previous.VERSION),
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

    def test_all_four_unchanged_historical_methods_execute_in_exact_context(self):
        actual_module = sys.modules[guard.previous.__name__]
        for module_name, class_name, method_name in historical.METHODS:
            with self.subTest(method=method_name), tempfile.TemporaryDirectory() as directory:
                case = unittest.TestCase()
                case.root = Path(directory)
                method = getattr(getattr(importlib.import_module(module_name), class_name), method_name)
                self.assertTrue(hasattr(method, "__v13_original__"))
                method(case)  # executes the exact old assertion body and guard
                self.assertIs(sys.modules[guard.previous.__name__], actual_module)
        selected = historical.facade(guard)
        selected.verify_ci()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for path in (".github/workflows/ci.yml", ".github/workflows/p7-engineering.yml"):
                target = root / path
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(guard.git.blob(guard.BASE, path))
            selected.verify_ci(root=root)
            (root / ".github/workflows/ci.yml").write_text("name: unreviewed\n")
            with self.assertRaisesRegex(AssertionError, "CI differs"):
                selected.verify_ci(root=root)

    def test_optimized_python_and_implicit_or_floating_selectors_fail(self):
        script = str(guard.ROOT / "scripts/verify_reviewed_source_v13.py")
        for arguments, message in ((["-O", script, "--source-version", guard.VERSION], "without -O"),
                                   ([script, "--source-version", "latest"], "invalid choice"),
                                   ([script], "required")):
            result = subprocess.run([sys.executable, "-B", *arguments], capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn(message, result.stderr)


if __name__ == "__main__":
    unittest.main()
