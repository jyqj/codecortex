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


    def set_engine_config(self, value):
        self.suite["engine_config"] = copy.deepcopy(value)
        self.write(self.input / "suite.compat.json", self.suite)
        self.lock["suites"][0]["sha256"] = self.sha(self.input / "suite.compat.json")
        self.save_lock()

    @staticmethod
    def text_hidden_config():
        return {"auto_index": {"enabled": False},
                "indexing": {"include_text_files": True, "include_hidden_files": True}}

    def test_text_hidden_is_explicit_and_does_not_widen_default_inspection(self):
        self.set_engine_config(self.text_hidden_config())
        with self.assertRaisesRegex(compat.Invalid, "only the explicit local default"):
            compat.inspect_lock(self.lockfile)
        checked = compat.inspect_lock(self.lockfile, configuration_profile="local-text-hidden")
        self.assertEqual(checked["identity"]["configuration_profile"], "local-text-hidden")
        self.assertEqual(checked["identity"]["suites"]["compat"]["configuration"], self.text_hidden_config())

    def test_original_default_profile_keeps_original_identity_and_predicate(self):
        checked = compat.inspect_lock(self.lockfile)
        self.assertNotIn("configuration_profile", checked["identity"])
        with self.assertRaisesRegex(compat.Invalid, "must match exactly"):
            compat.inspect_lock(self.lockfile, configuration_profile="local-text-hidden")
        for config in [
            {"auto_index": {"enabled": True}},
            {"auto_index": {"enabled": False}, "query": {"strategy": "semantic"}},
            self.text_hidden_config(),
        ]:
            self.set_engine_config(config)
            with self.assertRaisesRegex(compat.Invalid, "only the explicit local default"):
                compat.inspect_lock(self.lockfile)

    def test_text_hidden_profile_rejects_missing_extra_or_network_configuration(self):
        valid = self.text_hidden_config()
        candidates = []
        for flag in ("include_text_files", "include_hidden_files"):
            missing = copy.deepcopy(valid)
            del missing["indexing"][flag]
            candidates.append(missing)
            disabled = copy.deepcopy(valid)
            disabled["indexing"][flag] = False
            candidates.append(disabled)
        for section, key, value in (("auto_index", "enabled", 0),
                                    ("indexing", "include_text_files", 1),
                                    ("indexing", "include_hidden_files", 1)):
            altered = copy.deepcopy(valid)
            altered[section][key] = value
            candidates.append(altered)
        for key, value in (("semantic", {"enabled": True}), ("query", {"strategy": "semantic"}),
                           ("provider", "remote"), ("extra", True)):
            extra = copy.deepcopy(valid)
            extra[key] = value
            candidates.append(extra)
        altered = copy.deepcopy(valid)
        altered["auto_index"]["enabled"] = True
        candidates.append(altered)
        altered = copy.deepcopy(valid)
        altered["indexing"]["max_file_bytes"] = 1000000
        candidates.append(altered)
        for config in candidates:
            with self.subTest(config=config):
                self.set_engine_config(config)
                with self.assertRaisesRegex(compat.Invalid, "must match exactly"):
                    compat.inspect_lock(self.lockfile, configuration_profile="local-text-hidden")

    def test_compat_native_require_the_same_explicit_configuration(self):
        self.set_engine_config(self.text_hidden_config())
        native = copy.deepcopy(self.suite)
        native["scoring"] = "codecortex-native-v1"
        native["engine_config"] = {"auto_index": {"enabled": False}}
        native_path = self.input / "suite.native.json"
        self.write(native_path, native)
        self.lock["suites"].append({**copy.deepcopy(self.lock["suites"][0]),
            "profile": "native", "path": "suite.native.json", "sha256": self.sha(native_path)})
        self.save_lock()
        with self.assertRaisesRegex(compat.Invalid, "must match exactly"):
            compat.inspect_lock(self.lockfile, configuration_profile="local-text-hidden")
        native["engine_config"] = self.text_hidden_config()
        self.write(native_path, native)
        self.lock["suites"][1]["sha256"] = self.sha(native_path)
        self.save_lock()
        checked = compat.inspect_lock(self.lockfile, configuration_profile="local-text-hidden")
        self.assertEqual(checked["identity"]["suites"]["native"]["configuration"],
                         checked["identity"]["suites"]["compat"]["configuration"])

    def test_unknown_configuration_mode_is_rejected_before_process_creation(self):
        with mock.patch.object(compat, "command") as execute, self.assertRaisesRegex(compat.Invalid, "unknown explicit"):
            compat.run_locked(self.lockfile, self.root / "never", configuration_profile="anything")
        execute.assert_not_called()
        self.assertFalse((self.root / "never").exists())

    def test_explicit_mode_still_rejects_zero_exit_without_real_artifacts(self):
        self.set_engine_config(self.text_hidden_config())
        result, code = compat.run_locked(self.lockfile, self.root / "explicit-fake",
                                         configuration_profile="local-text-hidden")
        self.assertEqual(code, 2)
        self.assertEqual(result["status"], "invalid_measurement")
        self.assertFalse(result["release_certified"])
        self.assertEqual([c["process_exit_code"] for c in result["commands"]], [0, 0])

    def test_cli_requires_the_named_opt_in(self):
        self.set_engine_config(self.text_hidden_config())
        self.assertEqual(compat.main(["run", "--lock", str(self.lockfile),
                                      "--output", str(self.root / "default-refused"), "--validate-only"]), 2)
        self.assertFalse((self.root / "default-refused").exists())
        self.assertEqual(compat.main(["run", "--lock", str(self.lockfile),
                                      "--output", str(self.root / "named-mode"), "--validate-only",
                                      "--configuration-profile", "local-text-hidden"]), 0)
        receipt = json.loads((self.root / "named-mode" / "receipt.json").read_bytes())
        self.assertEqual(receipt["status"], "validated_inputs_only")
        self.assertEqual(receipt["profiles"]["compat"]["ranking"], "not_run")


class CompatRunIdentityTests(unittest.TestCase):
    """Synthetic retained runs exercise identity checks, never quality claims."""

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="p8-compat-run-identity-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / "raw").mkdir()
        self.queries = [{"id": ident, "query_family": "synthetic:" + ident,
            "category": "api_usage", "difficulty": 2, "language": "rust", "split": "dev",
            "query": "Find the fictional " + ident + " entry point.", "path_prefix": None,
            "no_answer": False, "expected_files": [ident + ".rs"], "answers": [], "annotations": {}}
            for ident in ("first", "second")]
        self.suite = {"schema_version": 1, "name": "synthetic retained run identity control",
            "source": {"root": "source", "commit": None, "digest": "a" * 64,
                       "files": ["first.rs", "second.rs"]},
            "queries": "queries.dev.jsonl", "queries_digest": "b" * 64, "scoring": "oce-compat-v1",
            "repetitions": 2, "warmup": 0, "seed": 2, "timeout_ms": 1000, "top_k": 10,
            "engine_config": {"auto_index": {"enabled": False}}}
        self.checked = {"suite": self.suite, "query_rows": len(self.queries),
                        "query_snapshot": copy.deepcopy(self.queries), "backend_kind": "rg"}
        self.manifest = {"suite": self.suite, "input": {"query_digest": "b" * 64,
                          "source_digest": "a" * 64}, "adapter": "rg-literal",
                         "adapter_version": "cc-eval-public-v7", "measurement_profile": "smoke"}
        self.gate = {"status": "baseline_recorded_not_quality_certified", "exit_code": 0, "reasons": []}
        self.metrics = {"queries": 2, "measured_rows": 4, "mean_top1": 0.0, "mean_ndcg10": 0.0}
        self.rows = [{"case_id": q["id"], "repetition": repetition, "status": "no_match",
                      "raw_path": "raw/" + str(i * 2 + repetition) + ".json"}
                     for i, q in enumerate(self.queries) for repetition in range(2)]
        for row in self.rows:
            (self.root / row["raw_path"]).write_text('{"synthetic":true}\n')
        (self.root / "report.md").write_text("Synthetic identity test only.\n")

    def check(self, exit_code=0):
        for filename, data in (("manifest.json", self.manifest), ("gate.json", self.gate),
                               ("metrics.json", self.metrics)):
            (self.root / filename).write_text(json.dumps(data) + "\n")
        for filename, data in (("queries.jsonl", self.queries), ("normalized.jsonl", self.rows)):
            (self.root / filename).write_text("".join(json.dumps(row) + "\n" for row in data))
        return compat.check_run(self.root, self.checked, exit_code)

    def test_original_locked_query_and_complete_request_matrix_are_accepted(self):
        self.assertEqual(self.check()["metrics"]["measured_rows"], 4)

    def test_shuffled_complete_request_matrix_is_still_accepted(self):
        self.rows.reverse()
        self.assertEqual(self.check()["metrics"]["measured_rows"], 4)

    def test_optional_query_defaults_do_not_change_locked_identity(self):
        del self.checked["query_snapshot"][0]["path_prefix"]
        del self.checked["query_snapshot"][0]["annotations"]
        self.assertEqual(self.check()["metrics"]["queries"], 2)

    def set_native_gold(self):
        self.suite["scoring"] = "codecortex-native-v1"
        for query in self.queries:
            query["expected_files"] = []
            query["answers"] = [{"id": "primary", "primary": True, "grade": 3,
                                  "alternatives": [{"path": query["id"] + ".rs",
                                                    "symbol": {"name": query["id"]}}]}]
        self.checked["query_snapshot"] = copy.deepcopy(self.queries)

    def test_optional_native_symbol_and_span_defaults_preserve_identity(self):
        self.set_native_gold()
        for query in self.queries:
            alternative = query["answers"][0]["alternatives"][0]
            alternative["span"] = None
            alternative["symbol"].update(kind=None, qname=None)
        self.assertEqual(self.check()["metrics"]["queries"], 2)

    def test_native_gold_span_and_primary_changes_are_rejected(self):
        self.set_native_gold()
        original = copy.deepcopy(self.queries[0]["answers"])
        for mutation in ("primary", "path", "span", "symbol"):
            with self.subTest(mutation=mutation):
                group = self.queries[0]["answers"][0]
                alternative = group["alternatives"][0]
                if mutation == "primary":
                    group["primary"] = False
                elif mutation == "path":
                    alternative["path"] = "easier.rs"
                elif mutation == "span":
                    alternative["span"] = {"start": 0, "end": 1}
                else:
                    alternative["symbol"]["name"] = "easier_symbol"
                with self.assertRaisesRegex(compat.Invalid, "query snapshot"):
                    self.check()
                self.queries[0]["answers"] = copy.deepcopy(original)

    def test_same_count_query_identity_replacement_is_rejected(self):
        self.queries[0]["id"] = "replacement"
        with self.assertRaisesRegex(compat.Invalid, "query snapshot"):
            self.check()

    def test_same_count_query_text_and_gold_changes_are_rejected(self):
        mutations = {"query": "An easier fictional replacement question.",
                     "expected_files": ["easier.rs"], "query_family": "easier-family",
                     "annotations": {"changed_gold_note": True}}
        for field, value in mutations.items():
            with self.subTest(field=field):
                original = copy.deepcopy(self.queries[0][field])
                self.queries[0][field] = value
                with self.assertRaisesRegex(compat.Invalid, "query snapshot"):
                    self.check()
                self.queries[0][field] = original

    def test_duplicate_output_query_cannot_replace_another_locked_query(self):
        self.queries[1] = copy.deepcopy(self.queries[0])
        with self.assertRaisesRegex(compat.Invalid, "query snapshot"):
            self.check()

    def test_unknown_case_cannot_replace_missing_case_at_same_denominator(self):
        self.rows[0]["case_id"] = "unrequested"
        with self.assertRaisesRegex(compat.Invalid, "request matrix"):
            self.check()

    def test_missing_case_cannot_be_hidden_by_extra_repetition_of_another_case(self):
        self.rows[0].update(case_id="second", repetition=2)
        with self.assertRaisesRegex(compat.Invalid, "request matrix"):
            self.check()

    def test_duplicate_case_and_repetition_is_rejected(self):
        self.rows[1].update(case_id=self.rows[0]["case_id"], repetition=self.rows[0]["repetition"])
        with self.assertRaisesRegex(compat.Invalid, "request matrix"):
            self.check()

    def test_out_of_range_and_wrongly_typed_repetitions_are_rejected(self):
        for repetition in (-1, 2, True, 0.0, "0"):
            with self.subTest(repetition=repr(repetition)):
                self.rows[0]["repetition"] = repetition
                with self.assertRaisesRegex(compat.Invalid, "request matrix"):
                    self.check()

    def test_missing_request_cannot_pass_with_smaller_metrics_denominator(self):
        self.rows.pop()
        self.metrics["measured_rows"] = 3
        with self.assertRaisesRegex(compat.Invalid, "missing measured rows"):
            self.check()

    def test_raw_response_path_cannot_be_reused_for_distinct_requests(self):
        self.rows[1]["raw_path"] = self.rows[0]["raw_path"]
        with self.assertRaisesRegex(compat.Invalid, "raw response"):
            self.check()

    def test_pinned_backend_and_profile_cannot_be_relabelled(self):
        for field, value in (("adapter", "mcp-stdio"), ("measurement_profile", "quality")):
            with self.subTest(field=field):
                original = self.manifest[field]
                self.manifest[field] = value
                with self.assertRaisesRegex(compat.Invalid, "adapter or measurement profile"):
                    self.check()
                self.manifest[field] = original

    def test_exit_code_and_gate_status_must_both_match(self):
        for patch in ({"exit_code": False}, {"status": "gate_failed"}):
            with self.subTest(patch=patch):
                original = copy.deepcopy(self.gate)
                self.gate.update(patch)
                with self.assertRaisesRegex(compat.Invalid, "process/gate"):
                    self.check()
                self.gate = original


if __name__ == "__main__":
    unittest.main()
