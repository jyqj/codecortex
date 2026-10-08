"""Negative controls for the public registry's added admission boundary."""

import copy
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import p8_native_registry as registry


def sample_rows():
    native = {"id": "public.fixture.1.en", "query_family": "public.fixture.1",
        "category": "api_usage", "difficulty": 2, "language": "rust", "split": "dev",
        "query": "Where does this synthetic fixture expose its entry?", "path_prefix": None,
        "no_answer": False, "expected_files": [],
        "answers": [{"id": "entry", "primary": True, "grade": 3,
                     "alternatives": [{"path": "src/lib.rs", "span": {"start": 0, "end": 4}}]}],
        "annotations": {"v19": {"review_status": "pending", "source_sha": "a" * 40}}}
    compat = copy.deepcopy(native)
    compat.update(answers=[], expected_files=["src/lib.rs"])
    return native, compat


def sample_review():
    native, _ = sample_rows()
    raw = (json.dumps(native) + "\n").encode()
    manifest = b'{"fixture":true}\n'
    record = {"author": "author", "reviewer": "reviewer", "upstream_sha": "a" * 40}
    receipt = {"status": "accepted_scoped", "author": "author", "reviewer": "reviewer",
        "source_sha": "a" * 40, "scope": "synthetic test fixture",
        "input_sha256": {"queries.native.dev.jsonl": registry.sha(raw),
                         "source-manifest.json": registry.sha(manifest)},
        "questions_reviewed": 1, "questions_accepted": 1,
        "question_records": [{"id": native["id"], "decision": "accepted_scoped"}]}
    return record, receipt, raw, manifest


class RegistryAdmissionTests(unittest.TestCase):
    def test_external_review_binds_pending_author_bytes_and_rejects_self_approval(self):
        record, receipt, raw, manifest = sample_review()
        self.assertEqual(registry.review_acceptance("vite", record, receipt, raw, manifest)["rows"], 1)
        receipt["reviewer"] = receipt["author"]
        with self.assertRaisesRegex(registry.Invalid, "independent reviewer"):
            registry.review_acceptance("vite", record, receipt, raw, manifest)

    def test_review_rejects_packet_drift_missing_ids_and_unaccepted_row(self):
        for corruption in ("hash", "id", "decision"):
            record, receipt, raw, manifest = sample_review()
            if corruption == "hash":
                receipt["input_sha256"]["queries.native.dev.jsonl"] = "0" * 64
            elif corruption == "id":
                receipt["question_records"][0]["id"] = "different.id"
            else:
                receipt["question_records"][0]["decision"] = "pending"
            with self.subTest(corruption=corruption), self.assertRaises(registry.Invalid):
                registry.review_acceptance("vite", record, receipt, raw, manifest)

    def test_global_duplicate_ids_nondev_and_compat_gold_drift_are_rejected(self):
        expected = {"native_rows": 1, "compat_rows": 1, "repositories": 1}
        for corruption in ("duplicate", "split", "compat_gold"):
            native, compat = sample_rows()
            group = ["fixture", [native], [compat]]
            if corruption == "duplicate":
                group[1].append(copy.deepcopy(native))
            elif corruption == "split":
                native["split"] = "holdout"
            else:
                compat["expected_files"] = ["src/wrong.rs"]
            with self.subTest(corruption=corruption), self.assertRaises(registry.Invalid):
                registry.global_checks([group], expected)
        native, compat = sample_rows()
        self.assertEqual(registry.global_checks([("fixture", [native], [compat])], expected)["native_rows"], 1)

    def test_same_length_source_tamper_and_parent_symlink_are_rejected(self):
        with tempfile.TemporaryDirectory(prefix="p8-registry-control-") as directory:
            root = Path(directory)
            (root / "actual").mkdir()
            (root / "actual/source.rs").write_bytes(b"code")
            expected = registry.sha(b"code")
            self.assertEqual(registry.locked_file(root, "actual/source.rs", expected), b"code")
            (root / "actual/source.rs").write_bytes(b"evil")
            with self.assertRaisesRegex(registry.Invalid, "hash drift"):
                registry.locked_file(root, "actual/source.rs", expected)
            (root / "linked").symlink_to(root / "actual", target_is_directory=True)
            with self.assertRaisesRegex(registry.Invalid, "symlink"):
                registry.locked_file(root, "linked/source.rs", registry.sha(b"evil"))

    def test_relocation_preserves_locks_gold_pointer_and_original_manifest(self):
        with tempfile.TemporaryDirectory(prefix="p8-registry-source-") as directory:
            suite = {"scoring": "codecortex-native-v1", "queries": "queries.native.dev.jsonl",
                "queries_digest": "b" * 64, "repetitions": 1, "warmup": 0, "seed": 42,
                "timeout_ms": 30000, "top_k": 10,
                "source": {"root": "../../source", "commit": "a" * 40,
                           "digest": "c" * 64, "files": ["src/lib.rs"]}}
            original = copy.deepcopy(suite)
            result = registry.relocate_suite(suite, Path(directory))
            self.assertEqual(suite, original)
            result["source"]["root"] = original["source"]["root"]
            self.assertEqual(result, original)
            suite["source"]["commit"] = None
            with self.assertRaisesRegex(registry.Invalid, "full Git commit"):
                registry.relocate_suite(suite, Path(directory))

    def test_failed_validator_is_retained_and_never_replaced_by_freeze(self):
        with tempfile.TemporaryDirectory(prefix="p8-registry-validation-") as directory:
            output = Path(directory)
            suites = [{"repository": "fixture", "profile": "native", "path": "/fixture/native.json", "rows": 1},
                      {"repository": "fixture", "profile": "compat", "path": "/fixture/compat.json", "rows": 1}]
            responses = [subprocess.CompletedProcess([], 2, b"source digest mismatch\n"),
                         subprocess.CompletedProcess([], 0, b"locks valid\n")]
            with mock.patch.object(registry.subprocess, "run", side_effect=responses) as run:
                calls = registry.validate_suites(Path("/fixed/cc-eval"), suites, output)
            self.assertEqual([c["status"] for c in calls], ["failed", "passed"])
            self.assertEqual(calls[0]["exit_code"], 2)
            self.assertEqual((output / "logs/01-fixture-native.log").read_bytes(), b"source digest mismatch\n")
            self.assertTrue(all(c.args[0][1:3] == ["validate", "--suite"] for c in run.call_args_list))


if __name__ == "__main__":
    unittest.main()
