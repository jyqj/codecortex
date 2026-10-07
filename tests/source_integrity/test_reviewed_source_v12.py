"""Both historical proofs and both sides of the reviewed merge are mandatory."""
import ast
import copy
from functools import lru_cache
import json
from pathlib import Path
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))
import verify_reviewed_source_v12 as guard


class DualSourceReviewTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        # Cache immutable Git blobs only. All working-tree snapshots and records
        # are still read from disk on every guard call.
        cache = patch.object(guard.git, "blob", lru_cache(None)(guard.git.blob))
        cache.start()
        cls.addClassCleanup(cache.stop)
        cls.registry = guard.load_registry()
        cls.expected = guard.approved_union(cls.registry)
        cls.inherited = {name: guard.git.blob(guard.BASE, name)
                         for name in guard.git.inputs(guard.BASE)}
        cls.joint_inherited = {name: guard.git.blob(guard.JOINT_BASE, name)
                               for name in guard.git.inputs(guard.JOINT_BASE)}
        cls.joint_registry = guard.joint.load_registry(guard.ROOT / guard.JOINT_REGISTRY_PATH)

    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        target = self.root / guard.REVIEW_PATH
        target.parent.mkdir(parents=True)
        target.write_bytes(guard.git.blob(guard.REVIEW, guard.REVIEW_PATH))

    def verify(self, registry=None, inherited=None, joint_inherited=None):
        return guard.reconstruct(self.registry if registry is None else registry,
                                 self.inherited if inherited is None else inherited,
                                 self.joint_inherited if joint_inherited is None else joint_inherited,
                                 root=self.root)

    def test_both_proofs_produce_the_reviewed_785_input_merge(self):
        result = self.verify()
        self.assertEqual(result, self.expected)
        self.assertEqual(len(result), 785)
        self.assertEqual({name for name in result if result[name] != self.inherited[name]},
                         {guard.LOAD_PATH})
        self.assertEqual({name for name in result if result[name] != self.joint_inherited[name]},
                         guard.JOINT_OVERRIDES)
        self.assertEqual(len(self.joint_registry["deltas"]["p7_strategy_p8_engineering"]["paths"]), 22)

    def test_stale_registry_and_every_identity_are_rejected(self):
        path = self.root / "registry.json"
        path.write_text(json.dumps(self.registry) + " ")
        with self.assertRaisesRegex(AssertionError, "altered v12 registry"):
            guard.load_registry(path)
        for key in guard.identities():
            with self.subTest(key=key), self.assertRaisesRegex(AssertionError, "wrong v12 identity"):
                self.verify(dict(self.registry, **{key: "altered"}))

    def test_floating_pins_and_wrong_parent_products_fail_closed(self):
        for name in ("BASE", "JOINT_BASE", "PRODUCT", "REVIEW", "PREVIOUS_SNAPSHOT", "JOINT_SNAPSHOT"):
            with self.subTest(name=name), patch.object(guard, name, "HEAD"):
                with self.assertRaisesRegex(AssertionError, "immutable full commit"):
                    guard.verify_pins()
        for name, message in (("BASE", "proven v11 product"),
                              ("JOINT_BASE", "proven joint22 product")):
            with self.subTest(name=name), patch.object(guard, name, "0" * 40):
                with self.assertRaisesRegex(AssertionError, message):
                    guard.verify_pins()

    def test_each_inherited_inventory_and_bytes_are_bound(self):
        for parent, original in (("inherited", self.inherited), ("joint_inherited", self.joint_inherited)):
            for mutation in ("bytes", "missing", "extra"):
                altered = dict(original)
                if mutation == "bytes":
                    altered["Cargo.lock"] += b"\n"
                elif mutation == "missing":
                    del altered["Cargo.lock"]
                else:
                    altered["crates/unknown.rs"] = b"// unknown\n"
                with self.subTest(parent=parent, mutation=mutation):
                    with self.assertRaisesRegex(AssertionError, "inherited bytes differ"):
                        self.verify(**{parent: altered})

    def test_delta_and_joint_overrides_cannot_expand_or_disappear(self):
        for field, message in (("delta", "exactly the reviewed load test"),
                               ("joint_overrides", "joint override inventory")):
            for kind in ("remove", "add"):
                data = copy.deepcopy(self.registry)
                if kind == "remove":
                    del data[field][next(iter(data[field]))]
                else:
                    data[field]["Cargo.lock"] = {"before_sha256": "0" * 64, "sha256": "1" * 64}
                with self.subTest(field=field, kind=kind), self.assertRaisesRegex(AssertionError, message):
                    self.verify(data)

    def test_before_and_after_digests_are_checked_for_both_parents(self):
        for field in ("delta", "joint_overrides"):
            for name in self.registry[field]:
                for key, message in (("before_sha256", "before digest"), ("sha256", "after digest")):
                    data = copy.deepcopy(self.registry)
                    data[field][name][key] = "0" * 64
                    with self.subTest(field=field, path=name, key=key):
                        with self.assertRaisesRegex(AssertionError, message):
                            self.verify(data)

    def test_fixed_review_digest_bytes_and_symlink_are_checked(self):
        with self.assertRaisesRegex(AssertionError, "review digest"):
            self.verify(dict(self.registry, review_sha256="0" * 64))
        target = self.root / guard.REVIEW_PATH
        original = target.read_bytes()
        target.write_bytes(original + b"\n")
        with self.assertRaisesRegex(AssertionError, "fixed review record changed"):
            self.verify()
        other = self.root / "review-copy.json"
        other.write_bytes(original)
        target.unlink()
        target.symlink_to(other)
        with self.assertRaisesRegex(AssertionError, "fixed review record changed"):
            self.verify()

    def test_independent_review_cannot_omit_a_parent_or_a_delta(self):
        # Only the fixed-review blob is substituted to exercise its semantic
        # binding; no approval chain, source inventory or source blob is mocked.
        original_blob = guard.git.blob
        original = json.loads(original_blob(guard.REVIEW, guard.REVIEW_PATH))
        mutations = [(key, "altered") for key in ("source", "base", "joint_base", "verdict")]
        mutations += [("paths", {}), ("joint_paths", {})]
        for key, value in mutations:
            changed = dict(original, **{key: value})
            raw = json.dumps(changed).encode()
            (self.root / guard.REVIEW_PATH).write_bytes(raw)
            data = dict(self.registry, review_sha256=guard.git.sha(raw))

            def blob(ref, name):
                return raw if (ref, name) == (guard.REVIEW, guard.REVIEW_PATH) else original_blob(ref, name)

            with self.subTest(key=key), patch.object(guard.git, "blob", blob):
                with self.assertRaisesRegex(AssertionError, "both fixed source deltas"):
                    self.verify(data)

    def test_independent_review_rows_cannot_disagree_with_either_parent(self):
        original_blob = guard.git.blob
        original = json.loads(original_blob(guard.REVIEW, guard.REVIEW_PATH))
        for field in ("paths", "joint_paths"):
            for key in ("before_sha256", "sha256"):
                changed = copy.deepcopy(original)
                changed[field][guard.LOAD_PATH][key] = "0" * 64
                raw = json.dumps(changed).encode()
                (self.root / guard.REVIEW_PATH).write_bytes(raw)
                data = dict(self.registry, review_sha256=guard.git.sha(raw))

                def blob(ref, name):
                    return raw if (ref, name) == (guard.REVIEW, guard.REVIEW_PATH) else original_blob(ref, name)

                with self.subTest(field=field, key=key), patch.object(guard.git, "blob", blob):
                    with self.assertRaisesRegex(AssertionError, "independent review delta differs"):
                        self.verify(data)

    def test_complete_manifest_cannot_omit_add_or_mutate(self):
        for mutation in ("missing", "extra", "bytes"):
            data = copy.deepcopy(self.registry)
            if mutation == "missing":
                del data["complete_inputs"]["Cargo.lock"]
            else:
                data["complete_inputs"]["crates/unknown.rs" if mutation == "extra" else "Cargo.lock"] = "0" * 64
            with self.subTest(mutation=mutation), self.assertRaisesRegex(AssertionError, "complete manifest"):
                self.verify(data)

    def test_all_aliases_and_previous_snapshots_remain_byte_frozen(self):
        for name, (ref, original) in guard.SNAPSHOTS.items():
            target = self.root / name
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(guard.git.blob(ref, original))
        guard.verify_snapshots(self.root)
        for name in guard.SNAPSHOTS:
            target = self.root / name
            raw = target.read_bytes()
            target.write_bytes(raw + b"\n")
            with self.subTest(path=name), self.assertRaisesRegex(AssertionError, "approval snapshot changed"):
                guard.verify_snapshots(self.root)
            target.write_bytes(raw)
        target = self.root / guard.JOINT_REGISTRY_PATH
        original = self.root / "alias-copy.json"
        original.write_bytes(target.read_bytes())
        target.unlink()
        target.symlink_to(original)
        with self.assertRaisesRegex(AssertionError, "approval snapshot changed"):
            guard.verify_snapshots(self.root)

    def test_each_full_approval_chain_is_required(self):
        for chain, label in ((guard.previous, "v11 chain rejected"), (guard.joint, "joint22 chain rejected")):
            with self.subTest(chain=label), patch.object(chain, "approved_union", side_effect=AssertionError(label)):
                with self.assertRaisesRegex(AssertionError, label):
                    guard.approved_union(self.registry)

    def test_incoming_registry_uses_its_explicit_original_bytes(self):
        self.assertEqual(guard.joint.load_registry(guard.ROOT / guard.JOINT_REGISTRY_PATH), self.joint_registry)
        # The alias deliberately retains its canonical default. Accepting that
        # default would silently read the older P8 registry, not joint22's.
        with self.assertRaisesRegex(AssertionError, "stale or altered reviewed registry"):
            guard.joint.load_registry()
        data = copy.deepcopy(self.joint_registry)
        del data["deltas"]["p7_strategy_p8_engineering"]
        with self.assertRaisesRegex(AssertionError, "reviewed delta inventory"):
            guard.joint.approved_union(data)
        changed = copy.deepcopy(guard.joint.APPROVED)
        changed["p7_strategy_p8_engineering"]["paths"].append(changed["gc_unlink_accounting"]["paths"][0])
        with patch.object(guard.joint, "APPROVED", changed):
            with self.assertRaisesRegex(AssertionError, "overlapping reviewed deltas"):
                guard.joint.approved_union(self.joint_registry)

    def test_original_incoming_ci_method_runs_all_its_unchanged_controls(self):
        path = guard.ROOT / guard.JOINT_TEST_PATH
        self.assertEqual(path.read_bytes(), guard.git.blob(guard.JOINT_SNAPSHOT,
                                                        "tests/source_integrity/test_reviewed_source.py"))
        module = ast.parse(path.read_bytes(), filename=str(path))
        methods = [method for cls in module.body if isinstance(cls, ast.ClassDef)
                   for method in cls.body if isinstance(method, ast.FunctionDef)
                   and method.name == "test_ci_only_allows_reviewed_migrations_and_local_p8_checks"]
        self.assertEqual(len(methods), 1)
        # Compile the exact unchanged method. The only adapter supplies the
        # original CI root to its zero-argument positive check; every mutated
        # root is passed to the real unchanged joint guard. No guard is patched.
        original_root = guard.ROOT / guard.JOINT_FIXTURE_ROOT
        facade = SimpleNamespace(expected_ci=guard.joint.expected_ci,
                                 verify_ci=lambda root=original_root: guard.joint.verify_ci(root=root))
        namespace = {"guard": facade}
        exec(compile(ast.Module(body=methods, type_ignores=[]), str(path), "exec"), namespace)
        namespace[methods[0].name](self)

    def test_joint_ci_is_executed_in_addition_to_frozen_snapshot_checks(self):
        guard.verify_joint_ci_snapshot()
        with patch.object(guard.joint, "verify_ci", side_effect=AssertionError("joint CI rejected")):
            with self.assertRaisesRegex(AssertionError, "joint CI rejected"):
                guard.approved_union(self.registry)

    def test_current_ci_preserves_all_old_steps_and_private_binary_binding(self):
        target = self.root / ".github/workflows/ci.yml"
        target.parent.mkdir(parents=True)
        p7 = self.root / ".github/workflows/p7-engineering.yml"
        p7.write_bytes(guard.git.blob(guard.previous.previous.P7_SNAPSHOT, ".github/workflows/p7-engineering.yml"))
        expected = guard.expected_ci()
        target.write_text(expected)
        guard.verify_ci(self.root)
        self.assertEqual(expected.count("python3 scripts/verify_historical_integrations_v2.py"), 1)
        private_default = 'CODECORTEX_BENCH_BINARY="$RUNNER_TEMP/p7-017-default/codecortex"'
        self.assertEqual(expected.count(private_default), 20)
        for before, after in (("cargo test --workspace", "true # cargo test --workspace"),
                              ("python3 scripts/verify_historical_integrations_v2.py", "true # skip historical proof"),
                              ("-p 'test_p8_*.py'", "-p test_p8_release_evidence.py"),
                              (private_default, 'CODECORTEX_BENCH_BINARY="$PWD/target/debug/codecortex"')):
            self.assertIn(before, expected)
            target.write_text(expected.replace(before, after, 1))
            with self.subTest(step=before), self.assertRaisesRegex(AssertionError, "CI differs"):
                guard.verify_ci(self.root)
        target.write_text(expected)
        p7.write_bytes(p7.read_bytes() + b"\n# drift\n")
        with self.assertRaisesRegex(AssertionError, "P7 engineering workflow changed"):
            guard.verify_ci(self.root)

    def test_current_source_cannot_self_authorize_or_hide_a_new_input(self):
        for name, raw in self.expected.items():
            target = self.root / name
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(raw)
        guard.git.verify_tree(self.root, self.expected, set(self.expected))
        target = self.root / guard.LOAD_PATH
        target.write_bytes(target.read_bytes() + b"\n// unreviewed\n")
        with self.assertRaisesRegex(AssertionError, "input bytes"):
            guard.git.verify_tree(self.root, self.expected, set(self.expected))
        target.write_bytes(self.expected[guard.LOAD_PATH])
        unknown = self.root / "crates/unknown.rs"
        unknown.write_text("// unknown\n")
        with self.assertRaisesRegex(AssertionError, "disk input inventory"):
            guard.git.verify_tree(self.root, self.expected, set(self.expected))

    def test_optimized_python_and_unknown_selection_fail_closed(self):
        script = str(guard.ROOT / "scripts/verify_reviewed_source_v12.py")
        for options, message in ((["-O", script, "--source-version", guard.VERSION], "without -O"),
                                 ([script, "--source-version", "latest"], "invalid choice"),
                                 ([script], "required")):
            result = subprocess.run([sys.executable, *options], capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn(message, result.stderr)


if __name__ == "__main__":
    unittest.main()
