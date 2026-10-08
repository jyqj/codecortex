"""Real v13 dual-chain execution, original test bodies and context tamper controls."""

from contextlib import ExitStack
from functools import lru_cache
import importlib
from pathlib import Path
import shutil
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))
import v14_historical_context as history
import verify_reviewed_source_v13 as previous
import v14_historical_test_adapter as adapter


def prior_proofs():
    modules = {}

    def visit(module):
        if module.__name__ in modules:
            return
        modules[module.__name__] = module
        for name in ("previous", "joint", "p7", "p8", "v1", "v2"):
            child = getattr(module, name, None)
            if getattr(child, "__name__", "").startswith("verify_"):
                visit(child)

    visit(previous)
    return {name: module for name, module in modules.items() if hasattr(module, "approved_union")}


class HistoricalContextTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cache = patch.object(previous.git, "blob", lru_cache(None)(previous.git.blob))
        cache.start()
        cls.addClassCleanup(cache.stop)
        cls.context = history.HistoricalContext()
        cls.addClassCleanup(cls.context.close)
        cls.selected = adapter.HistoricalGuard(previous, cls.context)
        cls.registry = cls.selected.load_registry()
        cls.expected = cls.context.approved_union(previous, cls.registry)
        cls.blobs, cls.protected = history.catalogue()

    def temporary(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        return Path(temporary.name)

    def test_actual_v13_result_matches_the_fixed_historical_source(self):
        names = set(history.git("ls-tree", "-r", "--name-only", history.HISTORICAL_HEAD,
                                "--", "crates", "Cargo.toml", "Cargo.lock").decode().splitlines())
        self.assertEqual(set(self.expected), names)
        self.assertEqual(len(self.expected), 795)
        for path, raw in self.expected.items():
            self.assertEqual(raw, history.fixed_blob(path), path)
        self.assertIn("scripts/verify_reviewed_source_v13.py", self.protected)
        self.assertIn("tests/source_integrity/test_reviewed_source_v13.py", self.protected)
        self.assertNotIn("scripts/p8_compat.py", self.protected)
        self.assertIn("scripts/p8_compat.py", self.blobs)

    def test_every_actual_historical_approval_is_called_and_can_reject(self):
        proofs = prior_proofs()
        self.assertIn("verify_reviewed_source_v13", proofs)
        self.assertIn("verify_reviewed_source_joint22_v10", proofs)
        self.assertIn("verify_reviewed_source", proofs)
        self.assertIn("verify_reviewed_source_p7_v9", proofs)
        with ExitStack() as contexts:
            spies = {name: contexts.enter_context(patch.object(module, "approved_union", wraps=module.approved_union))
                     for name, module in proofs.items()}
            self.assertEqual(self.context.approved_union(previous, self.registry), self.expected)
            self.assertTrue(all(spy.call_count for spy in spies.values()))
        for name, module in proofs.items():
            with self.subTest(proof=name), patch.object(module, "approved_union", side_effect=AssertionError("actual historical proof rejected")):
                with self.assertRaisesRegex(AssertionError, "actual historical proof rejected"):
                    self.context.approved_union(previous, self.registry)

    def test_actual_v13_ci_rejection_also_propagates(self):
        with patch.object(previous, "verify_ci", side_effect=AssertionError("actual v13 CI rejected")):
            with self.assertRaisesRegex(AssertionError, "actual v13 CI rejected"):
                self.context.approved_union(previous, self.registry)

    def test_current_frozen_files_cannot_hide_behind_the_pristine_snapshot(self):
        for path in ("scripts/verify_reviewed_source_v13.py", "scripts/reviewed-source-registry-v13.json",
                     previous.REVIEW_PATH, "tests/source_integrity/test_reviewed_source_v13.py",
                     "scripts/source_snapshots/v12_ci/.github/workflows/ci.yml"):
            target = self.context.root / path
            original = target.read_bytes()
            for mutation in ("edit", "delete", "symlink"):
                try:
                    target.unlink()
                    if mutation == "edit":
                        target.write_bytes(original + b"\n# unreviewed\n")
                    elif mutation == "symlink":
                        copy = self.temporary() / "same-bytes"
                        copy.write_bytes(original)
                        target.symlink_to(copy)
                    with self.subTest(path=path, mutation=mutation), self.assertRaisesRegex(AssertionError, "historical protection"):
                        history.verify_current_history(self.context.root)
                finally:
                    if target.exists() or target.is_symlink():
                        target.unlink()
                    target.write_bytes(original)
                    target.chmod(0o444)

    def test_same_byte_parent_directory_symlink_is_rejected(self):
        parent = self.context.root / "scripts/source_snapshots/v12_ci/.github"
        moved = self.temporary() / "same-parent"
        shutil.move(parent, moved)
        try:
            parent.symlink_to(moved, target_is_directory=True)
            with self.assertRaisesRegex(AssertionError, "historical protection symlink"):
                history.verify_current_history(self.context.root)
        finally:
            parent.unlink()
            shutil.move(moved, parent)

    def test_added_historical_snapshot_input_is_rejected_on_every_use(self):
        extra = self.context.root / "scripts/__pycache__/not_bytecode.py"
        extra.parent.mkdir(exist_ok=True)
        extra.write_text("# unreviewed historical input\n")
        try:
            with self.assertRaisesRegex(AssertionError, "historical context inventory"):
                self.context.approved_union(previous, self.registry)
        finally:
            extra.unlink()

    def test_context_tamper_after_the_real_proof_cannot_be_reported_as_passed(self):
        target = self.context.root / "scripts/verify_reviewed_source_v13.py"
        original = target.read_bytes()
        real = previous.approved_union

        def tamper(*args, **kwargs):
            result = real(*args, **kwargs)
            target.unlink()
            target.write_bytes(original + b"\n# changed after proof\n")
            return result

        try:
            with patch.object(previous, "approved_union", side_effect=tamper):
                with self.assertRaisesRegex(AssertionError, "historical context bytes changed"):
                    self.context.approved_union(previous, self.registry)
        finally:
            target.unlink()
            target.write_bytes(original)
            target.chmod(0o444)

    def test_guard_constant_patch_reads_and_writes_real_function_globals(self):
        self.assertIs(self.selected.__dict__, previous.__dict__)
        for name in ("BASE", "PRODUCT", "REVIEW"):
            original = getattr(previous, name)
            with self.subTest(pin=name), patch.object(self.selected, name, "HEAD"):
                self.assertEqual(getattr(previous, name), "HEAD")
                with self.assertRaisesRegex(AssertionError, "immutable full commit"):
                    self.selected.verify_pins()
            self.assertEqual(getattr(previous, name), original)

    def test_dynamic_function_patch_and_restore_affect_the_real_module(self):
        real = previous.verify_snapshots
        with patch.object(self.selected, "verify_snapshots", side_effect=AssertionError("proxy write reached real module")) as rejected:
            self.assertIs(previous.verify_snapshots, rejected)
            with self.assertRaisesRegex(AssertionError, "proxy write reached real module"):
                self.selected.verify_snapshots()
        self.assertIs(previous.verify_snapshots, real)
        with patch.object(previous, "verify_snapshots", wraps=real) as spy:
            self.selected.verify_snapshots()
            spy.assert_called_once_with(root=self.context.root)

    def test_explicit_positional_and_keyword_fixture_roots_are_never_redirected(self):
        fixture = self.temporary()
        for positional in (True, False):
            with self.subTest(positional=positional), patch.object(previous, "approved_union", wraps=previous.approved_union) as spy:
                with self.assertRaises((AssertionError, FileNotFoundError)):
                    if positional:
                        self.selected.approved_union(self.registry, fixture)
                    else:
                        self.selected.approved_union(self.registry, root=fixture)
                passed_root = spy.call_args.args[1] if positional else spy.call_args.kwargs["root"]
                self.assertEqual(passed_root, fixture)
                self.assertNotEqual(passed_root, self.context.root)
        for path in (".github/workflows/ci.yml", ".github/workflows/p7-engineering.yml"):
            target = fixture / path
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(self.blobs[path])
        self.selected.verify_ci(fixture)
        self.selected.verify_ci(root=fixture)
        (fixture / ".github/workflows/ci.yml").write_text("name: unreviewed\n")
        for args, kwargs in (((fixture,), {}), ((), {"root": fixture})):
            with self.assertRaisesRegex(AssertionError, "CI differs"):
                self.selected.verify_ci(*args, **kwargs)

    def test_proxy_and_real_module_wraps_spies_call_real_bodies_without_recursion(self):
        inherited = {path: previous.git.blob(previous.BASE, path) for path in previous.git.inputs(previous.BASE)}
        calls = {"approved_union": (self.registry,), "reconstruct": (self.registry, inherited),
                 "verify_snapshots": (), "verify_ci": (), "load_registry": ()}
        for method, arguments in calls.items():
            original = getattr(previous, method)
            for target in (self.selected, previous):
                with self.subTest(method=method, proxy=target is self.selected):
                    with patch.object(target, method, wraps=getattr(target, method)) as spy:
                        result = getattr(self.selected, method)(*arguments)
                        self.assertEqual(spy.call_count, 1)
                        if method in ("approved_union", "reconstruct"):
                            self.assertEqual(result, self.expected)
                    self.assertIs(getattr(previous, method), original)
        for target in (self.selected, previous):
            fixture = self.temporary()
            with patch.object(target, "verify_ci", wraps=target.verify_ci) as spy:
                with self.assertRaises((AssertionError, FileNotFoundError)):
                    self.selected.verify_ci(fixture)
                spy.assert_called_once_with(fixture)

    def test_nested_proxy_spies_and_inner_real_rejection_preserve_instrumentation(self):
        original = previous.verify_ci
        with patch.object(self.selected, "verify_ci", wraps=self.selected.verify_ci) as outer:
            with patch.object(self.selected, "verify_ci", wraps=self.selected.verify_ci) as inner:
                self.selected.verify_ci()
                self.assertEqual(outer.call_count, 1)
                self.assertEqual(inner.call_count, 1)
            with patch.object(previous, "verify_ci", side_effect=AssertionError("inner real rejection")):
                with self.assertRaisesRegex(AssertionError, "inner real rejection"):
                    self.selected.verify_ci()
            self.selected.verify_ci()
            self.assertEqual(outer.call_count, 2)
        self.assertIs(previous.verify_ci, original)

    def test_original_v13_constant_and_dual_chain_test_bodies_execute(self):
        module = importlib.import_module("test_reviewed_source_v13")
        original_class = module.CompleteSourceReviewTests
        subclass = type("UnchangedV13Controls", (original_class,), {})
        names = ("test_registry_bytes_identity_and_floating_pins_fail_closed",
                 "test_every_historical_approval_function_executes_and_can_reject")
        for name in names:
            self.assertIs(getattr(subclass, name).__code__, getattr(original_class, name).__code__)
        result = unittest.TestResult()
        with patch.object(module, "guard", self.selected):
            unittest.TestSuite(subclass(name) for name in names).run(result)
        self.assertEqual(result.testsRun, len(names))
        self.assertFalse(result.skipped)
        self.assertFalse(result.failures or result.errors, str(result.failures + result.errors))

    def test_floating_pin_and_protected_paths_are_rejected_before_git(self):
        for pin in ("HEAD", "refs/heads/main", "a" * 39):
            with self.subTest(pin=pin), patch.object(history, "HISTORICAL_HEAD", pin), patch.object(history, "git") as read:
                with self.assertRaisesRegex(AssertionError, "immutable full commit"):
                    history.catalogue()
                read.assert_not_called()
        for path in ("../escape", "questions.holdout.jsonl", "foo//bar"):
            with self.subTest(path=path), patch.object(history, "git") as read:
                with self.assertRaises(AssertionError):
                    history.fixed_blob(path)
                read.assert_not_called()


if __name__ == "__main__":
    unittest.main()
