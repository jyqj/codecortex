"""Fixed-lock negatives and a deliberately zero-exit, no-artifact fake evaluator."""

import copy
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import p8_compat as compat


class CompatLockTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="p8-compat-test-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.input = self.root / "inputs"
        self.input.mkdir()
        self.scorer = self.root / "scorer"
        self.scorer.mkdir()
        self.tool = self.root / "tool"
        self.tool.mkdir()
        self.eval = self.tool / "cc-eval"
        self.eval.write_text("#!/bin/sh\nexit 0\n")
        self.eval.chmod(0o755)
        self.rg = self.tool / "rg"
        self.rg.write_text("#!/bin/sh\nexit 1\n")
        self.rg.chmod(0o755)
        (self.input / "source").mkdir()
        (self.input / "source/example.rs").write_text("pub fn example() {}\n")
        self.query = {"id": "synthetic-1", "split": "dev", "query": "example", "query_family": "synthetic-family"}
        self.write(self.input / "queries.dev.jsonl", self.query)
        self.suite = {"schema_version": 1, "name": "synthetic controls only", "scoring": "oce-compat-v1",
            "queries": "queries.dev.jsonl", "queries_digest": "a" * 64,
            "source": {"root": "source", "commit": None, "digest": "b" * 64, "files": ["example.rs"]},
            "engine_config": {"auto_index": {"enabled": False}}, "top_k": 10, "timeout_ms": 1000,
            "repetitions": 1, "warmup": 0, "seed": 2}
        self.write(self.input / "suite.compat.json", self.suite)
        self.write(self.tool / "build.json", {"scope": "synthetic fake binary, not an actual cc-eval build"})
        self.write(self.tool / "policy.json", {"schema_version": 1, "max_ndcg_regression": 0.01,
            "max_top1_regression": 0.01, "max_latency_ratio": 1.2, "minimum_latency_samples": 200})
        for name in compat.SCORER_FILES:
            f = self.scorer / name
            f.parent.mkdir(parents=True, exist_ok=True)
            f.write_text("// synthetic scorer pin fixture\n")
        self.lock = {"schema_version": 1, "dataset": "public-dev-control", "repository": "fixture/example",
            "commit": "a" * 40, "source_mode": "snapshot_control", "input_root": str(self.input),
            "evaluator": {"path": str(self.eval), "sha256": self.sha(self.eval), "source_sha": "b" * 40,
                          "build_receipt": str(self.tool / "build.json"), "build_receipt_sha256": self.sha(self.tool / "build.json")},
            "backend": {"kind": "rg", "path": str(self.rg), "sha256": self.sha(self.rg)},
            "scorer": {"root": str(self.scorer), "files": {p: self.sha(self.scorer / p) for p in compat.SCORER_FILES}},
            "platform": compat.current_platform(),
            "suites": [{"profile": "compat", "path": "suite.compat.json", "sha256": self.sha(self.input / "suite.compat.json"),
                        "query_sha256": self.sha(self.input / "queries.dev.jsonl"),
                        "source_files": {"example.rs": self.sha(self.input / "source/example.rs")}}],
            "comparison_policy": {"path": str(self.tool / "policy.json"), "sha256": self.sha(self.tool / "policy.json")},
            "timeout_seconds": 5}
        self.lockfile = self.root / "lock.json"
        self.save_lock()

    @staticmethod
    def write(filename, value):
        filename.write_text(json.dumps(value) + "\n")

    @staticmethod
    def sha(filename):
        return compat.sha(filename.read_bytes())

    def save_lock(self):
        self.write(self.lockfile, self.lock)

    def inspect(self):
        self.save_lock()
        return compat.inspect_lock(self.lockfile)

    def test_bound_synthetic_control_is_explicitly_a_control(self):
        checked = self.inspect()
        self.assertEqual(checked["identity"]["dataset"], "public-dev-control")
        self.assertEqual(checked["suites"][0]["query_rows"], 1)

    def test_actual_zero_exit_fake_evaluator_cannot_fake_a_completed_run(self):
        output = self.root / "run"
        result, code = compat.run_locked(self.lockfile, output)
        self.assertEqual(code, 2)
        self.assertEqual(result["status"], "invalid_measurement")
        self.assertFalse(result["release_certified"])
        self.assertTrue((output / "compat-run.log").is_file())
        self.assertEqual([c["process_exit_code"] for c in result["commands"]], [0, 0])

    def test_query_or_binary_or_scorer_mutation_rejects_before_spawn(self):
        targets = [self.input / "queries.dev.jsonl", self.eval, self.scorer / sorted(compat.SCORER_FILES)[0]]
        for target in targets:
            with self.subTest(target=str(target)):
                before = target.read_bytes()
                target.write_bytes(before + b"changed")
                with mock.patch.object(compat, "command") as spawn, self.assertRaises(compat.Invalid):
                    compat.run_locked(self.lockfile, self.root / "never-created")
                spawn.assert_not_called()
                self.assertFalse((self.root / "never-created").exists())
                target.write_bytes(before)

    def test_external_name_cannot_relabel_control_source_as_flask(self):
        self.lock.update(dataset="flask", repository="pallets/flask", commit=compat.TARGETS["flask"][1])
        with self.assertRaisesRegex(compat.Invalid, "Git source lock"):
            self.inspect()

    def test_wrong_external_commit_is_not_accepted(self):
        self.lock.update(dataset="cc-switch", repository="farion1231/cc-switch", source_mode="git_checkout")
        with self.assertRaisesRegex(compat.Invalid, "target commit"):
            self.inspect()

    def test_platform_and_glob_policy_mismatch_are_rejected(self):
        for key in ("os", "architecture", "glob_policy"):
            with self.subTest(key=key):
                original = self.lock["platform"][key]
                self.lock["platform"][key] = "different"
                with self.assertRaisesRegex(compat.Invalid, "glob-policy"):
                    self.inspect()
                self.lock["platform"][key] = original

    def test_source_manifest_extra_or_omitted_file_is_rejected(self):
        self.lock["suites"][0]["source_files"]["unlisted.rs"] = "0" * 64
        with self.assertRaisesRegex(compat.Invalid, "inventory"):
            self.inspect()

    def test_native_profile_cannot_use_compat_scorer(self):
        self.lock["suites"].append({**copy.deepcopy(self.lock["suites"][0]), "profile": "native"})
        with self.assertRaisesRegex(compat.Invalid, "mixed scoring"):
            self.inspect()

    def test_holdout_file_pointer_and_symlink_source_are_rejected(self):
        source = self.input / "source/example.rs"
        source.unlink()
        source.symlink_to(self.rg)
        with self.assertRaisesRegex(compat.Invalid, "symlink"):
            self.inspect()
        with self.assertRaises(compat.Invalid):
            compat.joined_input(self.input, ".", "holdout.json")

    def test_output_cannot_overlap_or_replace_inputs(self):
        with self.assertRaisesRegex(compat.Invalid, "overlap"):
            compat.run_locked(self.lockfile, self.input / "run")
        self.assertFalse((self.input / "run").exists())

    def test_existing_output_is_never_overwritten(self):
        out = self.root / "existing"
        out.mkdir()
        (out / "keep.txt").write_text("retained")
        with self.assertRaises(FileExistsError):
            compat.run_locked(self.lockfile, out)
        self.assertEqual((out / "keep.txt").read_text(), "retained")

    def test_comparison_identity_difference_is_nonzero_before_comparator(self):
        left, right = self.root / "left", self.root / "right"
        left.mkdir()
        right.mkdir()
        a = {"identity": {"platform": compat.current_platform(), "backend": {"sha256": "a" * 64}}}
        b = copy.deepcopy(a)
        b["identity"]["backend"]["sha256"] = "b" * 64
        with mock.patch.object(compat, "load_run", side_effect=[a, b]), mock.patch.object(compat, "command") as execute:
            result, code = compat.compare_runs(left, right, self.eval, self.root / "comparison")
        self.assertEqual(code, 2)
        self.assertEqual(result["status"], "non_comparable_or_invalid")
        self.assertIn("backend", result["reason"])
        execute.assert_not_called()

    def test_raw_copy_preserves_original_and_rejects_later_drift(self):
        raw = self.root / "raw"
        raw.mkdir()
        (raw / "response.json").write_text('{"result":1}\n')
        before = compat.copy_raw(raw, self.root / "copy")
        self.assertEqual(compat.raw_inventory(raw), before)
        (raw / "response.json").write_text('{"result":2}\n')
        self.assertNotEqual(compat.raw_inventory(raw), before)

    def test_external_git_head_and_dirty_state_are_actually_checked(self):
        root = self.input / "source"
        def git(*args):
            return subprocess.check_output(["git", "-C", str(root), *args], stderr=subprocess.DEVNULL)
        git("init", "-q")
        git("add", ".")
        git("-c", "user.name=P8 fixture", "-c", "user.email=p8@example.invalid", "commit", "-qm", "source")
        head = git("rev-parse", "HEAD").decode().strip()
        compat.git_checkout_lock(root, head)
        with self.assertRaisesRegex(compat.Invalid, "HEAD drift"):
            compat.git_checkout_lock(root, "0" * 40)
        (root / "untracked.rs").write_text("untracked source")
        with self.assertRaisesRegex(compat.Invalid, "dirty external"):
            compat.git_checkout_lock(root, head)

    def test_zero_exit_comparator_must_emit_a_comparison_artifact(self):
        left, right = self.root / "left", self.root / "right"
        (left / "compat").mkdir(parents=True)
        (right / "compat").mkdir(parents=True)
        receipt = {"identity": {"platform": compat.current_platform(), "evaluator_sha256": self.sha(self.eval)},
                   "profiles": {"compat": {}}}
        with mock.patch.object(compat, "load_run", return_value=receipt):
            result, code = compat.compare_runs(left, right, self.eval, self.root / "no-comparison-artifact")
        self.assertEqual(code, 2)
        self.assertIn("omitted comparison artifact", result["reason"])

    def test_negative_signal_exit_cannot_be_maxed_into_a_green_comparison(self):
        left, right = self.root / "left", self.root / "right"
        (left / "compat").mkdir(parents=True)
        (right / "compat").mkdir(parents=True)
        receipt = {"identity": {"platform": compat.current_platform(), "evaluator_sha256": self.sha(self.eval)},
                   "profiles": {"compat": {}}}
        def forged(argv, *_args, **_kwargs):
            self.write(Path(argv[-1]), {"status": "failed", "exit_code": -9})
            return {"exit_code": -9}
        with mock.patch.object(compat, "load_run", return_value=receipt), mock.patch.object(compat, "command", side_effect=forged):
            result, code = compat.compare_runs(left, right, self.eval, self.root / "signal-exit")
        self.assertEqual(code, 2)
        self.assertIn("exit-code mismatch", result["reason"])


if __name__ == "__main__":
    unittest.main()
