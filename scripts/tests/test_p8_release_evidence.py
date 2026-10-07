"""Local Git/file counterexamples for P8 input locks and append-only archives."""

import argparse
import contextlib
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest import mock


SCRIPT = Path(__file__).resolve().parents[1] / "p8_release_evidence.py"
SPEC = importlib.util.spec_from_file_location("p8_release_evidence", SCRIPT)
P8 = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(P8)


class ReleaseEvidenceTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="p8-release-tests-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.repo = self.root / "repo"
        (self.repo / "src").mkdir(parents=True)
        (self.repo / "src/main.rs").write_text("fn main() {}\n")
        (self.repo / "Cargo.toml").write_text('[package]\nname = "fixture"\n')
        (self.repo / ".gitignore").write_text("*.ignored\n")
        self.git("init", "-q")
        self.git("add", ".")
        self.git("-c", "user.name=P8 fixture", "-c", "user.email=p8@example.invalid",
                 "commit", "-qm", "fixture")
        self.inputs = self.root / "inputs"
        self.inputs.mkdir()
        (self.inputs / "binary").write_bytes(b"fixture product bytes; never executed\n")
        (self.inputs / "binary").chmod(0o755)
        self.write(self.inputs / "config", {"auto_index": {"enabled": False}})
        self.write(self.inputs / "model", {"mode": "disabled"})
        self.write(self.inputs / "corpus", {"split": "fixture", **{
            key: hashlib.sha256(key.encode()).hexdigest() for key in
            ("corpus_sha256", "query_sha256", "gold_sha256")}})
        (self.inputs / "scoring").write_text("version = deterministic_fixture_v1\n")
        self.corpus = self.root / "dev-corpus"
        (self.corpus / "source").mkdir(parents=True)
        (self.corpus / "source/example.rs").write_text("pub fn fixture() {}\n")
        (self.corpus / "queries.jsonl").write_text('{"query":"fixture function"}\n')
        (self.corpus / "gold.jsonl").write_text('{"primary":"source/example.rs"}\n')
        self.candidate = self.root / "candidate"
        self.evidence = self.root / "evidence"
        self.runs = self.root / "runs"

    def git(self, *args):
        return subprocess.check_output(["git", "-C", str(self.repo), *args], stderr=subprocess.DEVNULL)

    @staticmethod
    def write(file, value):
        file.write_text(json.dumps(value) + "\n")

    def freeze_args(self, **changes):
        fields = {"source_root": str(self.repo), "source_path": ["src", "Cargo.toml"],
                  "output": str(self.candidate), "build_profile": "fixture", "feature": [],
                  "corpus_root": str(self.corpus)}
        fields.update({role: str(self.inputs / role) for role in
                       ("binary", "config", "corpus", "scoring", "model")})
        fields.update(changes)
        return argparse.Namespace(**fields)

    def freeze(self):
        return P8.freeze(self.freeze_args())[0]

    def prepare_evidence(self, status="passed_local"):
        manifest = P8.verify_candidate(self.candidate)
        (self.evidence / "raw").mkdir(parents=True, exist_ok=True)
        (self.evidence / "raw/sample.jsonl").write_text('{"fixture":true,"observed":7}\n')
        self.write(self.evidence / "metrics.json", {"fixture_count": 1})
        (self.evidence / "report.md").write_text("Fixture only; no product certification.\n")
        self.write(self.evidence / "gate.json", {
            "candidate_sha256": manifest["candidate_sha256"],
            "status": status, "exit_code": P8.GATES[status]})

    def archive_args(self, **changes):
        fields = {"candidate": str(self.candidate), "evidence": str(self.evidence),
                  "output_root": str(self.runs), "run_id": "fixture", "update_latest": True}
        fields.update(changes)
        return argparse.Namespace(**fields)

    def archive(self, **changes):
        result, code = P8.archive(self.archive_args(**changes))
        return Path(result["archive"]), result, code

    def test_freeze_binds_exact_bytes_and_declares_build_limit(self):
        result = self.freeze()
        manifest = P8.verify_candidate(self.candidate, result["candidate_sha256"])
        self.assertEqual(manifest["source"]["head"], self.git("rev-parse", "HEAD").decode().strip())
        self.assertEqual(manifest["inputs"]["binary"]["sha256"],
                         hashlib.sha256((self.inputs / "binary").read_bytes()).hexdigest())
        self.assertEqual((self.candidate / "inputs/binary").read_bytes(), (self.inputs / "binary").read_bytes())
        self.assertTrue(os.access(self.candidate / "inputs/binary", os.X_OK))
        self.assertEqual(manifest["build"]["provenance"], "operator_declaration_not_build_proof")
        self.assertFalse(manifest["release_certified"])

    def test_tracked_content_drift_rejects_archive_without_latest(self):
        self.freeze()
        self.prepare_evidence()
        (self.repo / "src/main.rs").write_text("fn changed() {}\n")
        with self.assertRaisesRegex(P8.Invalid, "source.*drift"):
            self.archive()
        self.assertFalse(self.runs.exists())

    def test_tracked_deletion_is_drift(self):
        self.freeze()
        (self.repo / "src/main.rs").unlink()
        with self.assertRaises(P8.Invalid):
            P8.verify_candidate(self.candidate)

    def test_untracked_and_ignored_additions_are_drift(self):
        self.freeze()
        for relative in ("src/new.rs", "src/new.ignored"):
            with self.subTest(relative=relative):
                extra = self.repo / relative
                extra.write_text("new source\n")
                with self.assertRaises(P8.Invalid):
                    P8.verify_candidate(self.candidate)
                extra.unlink()
        P8.verify_candidate(self.candidate)

    def test_staged_change_without_worktree_change_is_drift(self):
        (self.repo / "src/main.rs").write_text("dirty before freeze\n")
        self.freeze()
        self.git("add", "src/main.rs")
        with self.assertRaises(P8.Invalid):
            P8.verify_candidate(self.candidate)

    def test_head_change_with_same_source_bytes_is_drift(self):
        self.freeze()
        self.git("-c", "user.name=P8 fixture", "-c", "user.email=p8@example.invalid",
                 "commit", "--allow-empty", "-qm", "same tree, different candidate")
        with self.assertRaises(P8.Invalid):
            P8.verify_candidate(self.candidate)

    def test_dirty_added_and_deleted_inputs_remain_explicit(self):
        self.git("rm", "Cargo.toml")
        (self.repo / "src/added.rs").write_text("new\n")
        self.freeze()
        entries = P8.verify_candidate(self.candidate)["source"]["entries"]
        self.assertIsNone(entries["Cargo.toml"]["content"])
        self.assertIsNotNone(entries["Cargo.toml"]["head"])
        self.assertIsNone(entries["src/added.rs"]["head"])
        self.assertIsNotNone(entries["src/added.rs"]["content"])
        self.git("restore", "--staged", "Cargo.toml")
        with self.assertRaises(P8.Invalid):
            P8.verify_candidate(self.candidate)

    def test_every_external_input_is_rechecked(self):
        self.freeze()
        for role in ("binary", "config", "corpus", "scoring", "model"):
            with self.subTest(role=role):
                file = self.inputs / role
                original = file.read_bytes()
                file.write_bytes(original + b" ")
                with self.assertRaisesRegex(P8.Invalid, "input drift"):
                    P8.verify_candidate(self.candidate)
                file.write_bytes(original)
        P8.verify_candidate(self.candidate)

    def test_query_and_gold_body_drift_with_unchanged_metadata_is_rejected(self):
        self.freeze()
        metadata_before = (self.inputs / "corpus").read_bytes()
        for relative in ("queries.jsonl", "gold.jsonl", "source/example.rs"):
            with self.subTest(relative=relative):
                file = self.corpus / relative
                before = file.read_bytes()
                file.write_bytes(before + b"different body\n")
                with self.assertRaisesRegex(P8.Invalid, "corpus content.*drift"):
                    P8.verify_candidate(self.candidate)
                self.assertEqual((self.inputs / "corpus").read_bytes(), metadata_before)
                file.write_bytes(before)

    def test_corpus_added_deleted_and_symlink_files_are_rejected(self):
        self.freeze()
        addition = self.corpus / "added.jsonl"
        addition.write_text("additional query\n")
        with self.assertRaisesRegex(P8.Invalid, "corpus content.*drift"):
            P8.verify_candidate(self.candidate)
        addition.unlink()
        saved = (self.corpus / "gold.jsonl").read_bytes()
        (self.corpus / "gold.jsonl").unlink()
        with self.assertRaisesRegex(P8.Invalid, "corpus content.*drift"):
            P8.verify_candidate(self.candidate)
        (self.corpus / "gold.jsonl").write_bytes(saved)
        addition.symlink_to(self.inputs / "binary")
        with self.assertRaisesRegex(P8.Invalid, "symlink"):
            P8.verify_candidate(self.candidate)

    def test_metadata_only_candidate_is_explicit_and_cannot_promote_latest(self):
        result, _ = P8.freeze(self.freeze_args(corpus_root=None))
        self.assertEqual(result["corpus_content_status"], "unverified_corpus_content")
        self.prepare_evidence()
        output, result, code = self.archive()
        self.assertEqual(code, 0)
        self.assertFalse(result["latest_updated"])
        self.assertFalse((self.runs / "latest.json").exists())
        P8.verify_archive(output)

    def test_corpus_output_overlap_and_holdout_subtree_are_rejected(self):
        with self.assertRaisesRegex(P8.Invalid, "overlap"):
            P8.freeze(self.freeze_args(output=str(self.corpus / "candidate")))
        (self.corpus / "holdout").mkdir()
        (self.corpus / "holdout/body.jsonl").write_text("not admitted\n")
        with self.assertRaisesRegex(P8.Invalid, "holdout"):
            self.freeze()

    def test_executable_metadata_drift_is_rejected(self):
        self.freeze()
        (self.inputs / "binary").chmod(0o644)
        with self.assertRaisesRegex(P8.Invalid, "input drift"):
            P8.verify_candidate(self.candidate)

    def test_environment_allowlist_omits_credentials_and_detects_drift(self):
        with mock.patch.dict(os.environ, {"OPENAI_API_KEY": "must-never-be-recorded", "TZ": "UTC"}):
            self.freeze()
            text = (self.candidate / "candidate.json").read_text()
            self.assertNotIn("must-never-be-recorded", text)
            self.assertNotIn("OPENAI_API_KEY", text)
            with mock.patch.dict(os.environ, {"TZ": "Asia/Shanghai"}):
                with self.assertRaisesRegex(P8.Invalid, "environment drift"):
                    P8.verify_candidate(self.candidate)

    def test_symlink_source_and_input_parent_are_rejected(self):
        (self.repo / "src/link.rs").symlink_to(self.inputs / "binary")
        with self.assertRaisesRegex(P8.Invalid, "symlink"):
            self.freeze()
        (self.repo / "src/link.rs").unlink()
        link = self.root / "linked-inputs"
        link.symlink_to(self.inputs, target_is_directory=True)
        with self.assertRaisesRegex(P8.Invalid, "symlink"):
            P8.freeze(self.freeze_args(binary=str(link / "binary")))

    def test_submodule_index_is_rejected(self):
        head = self.git("rev-parse", "HEAD").decode().strip()
        self.git("update-index", "--add", "--cacheinfo", "160000," + head + ",src/nested")
        with self.assertRaisesRegex(P8.Invalid, "submodule"):
            self.freeze()

    def test_unmerged_index_is_rejected(self):
        oid = self.git("rev-parse", "HEAD:src/main.rs").decode().strip()
        lines = "0 " + "0" * 40 + "\tsrc/main.rs\n"
        lines += "".join(f"100644 {oid} {stage}\tsrc/main.rs\n" for stage in (1, 2, 3))
        subprocess.run(["git", "-C", str(self.repo), "update-index", "--index-info"],
                       input=lines.encode(), stdout=subprocess.DEVNULL, check=True)
        with self.assertRaisesRegex(P8.Invalid, "unmerged"):
            self.freeze()

    def test_unsafe_scopes_and_holdout_paths_are_rejected_before_read(self):
        for prefix in ("../inputs", "/etc", ".git", "src/../Cargo.toml", "src/holdout"):
            with self.subTest(prefix=prefix):
                with self.assertRaises(P8.Invalid):
                    P8.freeze(self.freeze_args(source_path=[prefix]))
        protected = self.root / "holdout"
        protected.mkdir()
        (protected / "metadata.json").write_text("body must not be parsed")
        with self.assertRaisesRegex(P8.Invalid, "holdout"):
            P8.freeze(self.freeze_args(corpus=str(protected / "metadata.json")))

    def test_directory_and_fifo_inputs_fail_without_blocking(self):
        with self.assertRaises(P8.Invalid):
            P8.freeze(self.freeze_args(binary=str(self.inputs)))
        fifo = self.inputs / "fifo"
        os.mkfifo(fifo)
        with self.assertRaisesRegex(P8.Invalid, "regular file"):
            P8.file_record(fifo)

    def test_overlapping_scopes_and_output_source_are_rejected(self):
        with self.assertRaises(P8.Invalid):
            P8.freeze(self.freeze_args(source_path=["src", "src/main.rs"]))
        with self.assertRaisesRegex(P8.Invalid, "overlap"):
            P8.freeze(self.freeze_args(output=str(self.repo / "src/candidate")))

    def test_existing_candidate_and_even_empty_output_are_never_overwritten(self):
        self.freeze()
        before = P8.inventory(self.candidate)
        with self.assertRaises(FileExistsError):
            self.freeze()
        self.assertEqual(P8.inventory(self.candidate), before)
        empty = self.root / "empty"
        empty.mkdir()
        with self.assertRaises(FileExistsError):
            P8.freeze(self.freeze_args(output=str(empty)))

    def test_live_model_and_credential_config_are_rejected(self):
        self.write(self.inputs / "model", {"mode": "live", "model_id": "example"})
        with self.assertRaisesRegex(P8.Invalid, "live models"):
            self.freeze()
        self.write(self.inputs / "model", {"mode": "disabled"})
        self.write(self.inputs / "config", {"provider": {"api_key": "sensitive-fixture"}})
        with self.assertRaisesRegex(P8.Invalid, "credential"):
            self.freeze()
        self.assertFalse(self.candidate.exists())

    def test_fake_model_requires_full_identity(self):
        self.write(self.inputs / "model", {"mode": "fake"})
        with self.assertRaises(P8.Invalid):
            self.freeze()
        self.write(self.inputs / "model", {"mode": "fake", "model_id": "fixture",
                                           "revision": "v1", "encoding_space": "fixture_4d"})
        self.freeze()
        self.assertEqual(P8.verify_candidate(self.candidate)["model"]["revision"], "v1")

    def test_invalid_corpus_and_duplicate_json_keys_fail_closed(self):
        (self.inputs / "config").write_text('{"x":1,"x":2}')
        with self.assertRaises(P8.Invalid):
            self.freeze()
        self.write(self.inputs / "config", {})
        self.write(self.inputs / "corpus", {"split": "holdout"})
        with self.assertRaises(P8.Invalid):
            self.freeze()

    def test_file_json_git_tree_and_total_budgets(self):
        with self.assertRaises(P8.Invalid):
            P8.file_record(self.inputs / "binary", limit=3)
        with mock.patch.object(P8, "MAX_JSON", 3):
            with self.assertRaises(P8.Invalid):
                P8.read_json(self.inputs / "config")
        with mock.patch.object(P8, "MAX_GIT", 3):
            with self.assertRaises(P8.Invalid):
                P8.source_identity(self.repo, ["src"])
        with mock.patch.object(P8, "MAX_FILES", 1):
            with self.assertRaises(P8.Invalid):
                P8.tree_files(self.inputs)
        with mock.patch.object(P8, "MAX_TOTAL", 3):
            with self.assertRaises(P8.Invalid):
                P8.inventory(self.inputs)

    def test_input_mutation_during_stream_is_detected(self):
        target = self.inputs / "binary"
        original_hash = hashlib.sha256
        class MutatingHash:
            def __init__(self):
                self.hash = original_hash()
            def update(inner, value):
                inner.hash.update(value)
                target.write_bytes(b"replacement while copying\n")
            def hexdigest(inner):
                return inner.hash.hexdigest()
        with mock.patch.object(P8.hashlib, "sha256", MutatingHash):
            with self.assertRaises(P8.Invalid):
                P8.file_record(target)

    def test_candidate_snapshot_corruption_and_manifest_digest_are_detected(self):
        self.freeze()
        (self.candidate / "inputs/binary").write_bytes(b"tampered")
        with self.assertRaisesRegex(P8.Invalid, "checksum"):
            P8.verify_candidate(self.candidate)
        with self.assertRaises(P8.Invalid):
            P8.verify_candidate(self.candidate, "0" * 64)
        manifest = json.loads((self.candidate / "candidate.json").read_text())
        manifest["build"]["profile"] = "release"
        self.write(self.candidate / "candidate.json", manifest)
        with self.assertRaisesRegex(P8.Invalid, "manifest checksum"):
            P8.verify_candidate(self.candidate)

    def test_rehashed_manifest_still_cannot_escape_snapshot_directory(self):
        self.freeze()
        manifest = json.loads((self.candidate / "candidate.json").read_text())
        manifest["inputs"]["binary"]["snapshot"] = "../inputs/binary"
        manifest["candidate_sha256"] = P8.digest({k: v for k, v in manifest.items()
                                                   if k != "candidate_sha256"})
        self.write(self.candidate / "candidate.json", manifest)
        with self.assertRaisesRegex(P8.Invalid, "unsafe relative path"):
            P8.verify_candidate(self.candidate)

    def test_archive_roundtrip_and_offline_portability_with_external_checksum(self):
        self.freeze()
        self.prepare_evidence()
        output, result, code = self.archive()
        self.assertEqual(code, 0)
        self.assertTrue(result["latest_updated"])
        latest = json.loads((self.runs / "latest.json").read_text())
        self.assertEqual(latest["run"], output.name)
        self.assertEqual(latest["archive_sha256"], result["archive_sha256"])
        self.repo.rename(self.root / "repo-retired")
        shutil.rmtree(self.inputs)
        shutil.rmtree(self.corpus)
        shutil.rmtree(self.candidate)
        shutil.rmtree(self.evidence)
        P8.verify_archive(output, result["archive_sha256"])
        with self.assertRaises(P8.Invalid):
            P8.verify_archive(output, "0" * 64)
        if shutil.which("sha256sum"):
            subprocess.run(["sha256sum", "--check", "checksums.sha256"], cwd=output,
                           stdout=subprocess.DEVNULL, check=True)

    def test_failure_and_native_success_archive_without_promoting_latest(self):
        self.freeze()
        self.prepare_evidence()
        self.archive(run_id="prior")
        latest_before = (self.runs / "latest.json").read_bytes()
        for status in ("gate_failed", "invalid_measurement", "cancelled", "failed", "inconclusive",
                       "passed", "baseline_recorded_not_quality_certified"):
            with self.subTest(status=status):
                self.prepare_evidence(status)
                output, result, code = self.archive(run_id=status)
                self.assertEqual(code, P8.GATES[status])
                self.assertFalse(result["latest_updated"])
                self.assertEqual((self.runs / "latest.json").read_bytes(), latest_before)
                P8.verify_archive(output)

    def test_gate_binding_and_exit_mismatch_reject_before_archive(self):
        self.freeze()
        self.prepare_evidence()
        gate = json.loads((self.evidence / "gate.json").read_text())
        for patch in ({"candidate_sha256": "0" * 64}, {"status": "unknown"},
                      {"exit_code": 1}, {"exit_code": False}, {"candidate_sha256": None}):
            with self.subTest(patch=patch):
                self.write(self.evidence / "gate.json", {**gate, **patch})
                with self.assertRaises(P8.Invalid):
                    self.archive()
                self.assertFalse(self.runs.exists())

    def test_archive_existing_run_is_unchanged(self):
        self.freeze()
        self.prepare_evidence()
        output, _, _ = self.archive()
        before = P8.inventory(output)
        with self.assertRaises(FileExistsError):
            self.archive()
        self.assertEqual(P8.inventory(output), before)

    def test_archive_payload_missing_extra_and_checksum_tampering(self):
        self.freeze()
        self.prepare_evidence()
        output, _, _ = self.archive()
        original = (output / "evidence/raw/sample.jsonl").read_bytes()
        (output / "evidence/raw/sample.jsonl").write_bytes(b"corrupt")
        with self.assertRaises(P8.Invalid):
            P8.verify_archive(output)
        (output / "evidence/raw/sample.jsonl").unlink()
        with self.assertRaises(P8.Invalid):
            P8.verify_archive(output)
        (output / "evidence/raw/sample.jsonl").write_bytes(original)
        (output / "extra.txt").write_text("unexpected")
        with self.assertRaises(P8.Invalid):
            P8.verify_archive(output)
        (output / "extra.txt").unlink()
        (output / "checksums.sha256").write_text("false checksum list\n")
        with self.assertRaises(P8.Invalid):
            P8.verify_archive(output)

    def test_evidence_self_containment_symlinks_and_unsafe_names(self):
        self.freeze()
        self.prepare_evidence()
        with self.assertRaisesRegex(P8.Invalid, "overlap"):
            self.archive(output_root=str(self.evidence / "nested"))
        (self.evidence / "linked").symlink_to(self.inputs / "binary")
        with self.assertRaisesRegex(P8.Invalid, "symlink"):
            self.archive()
        (self.evidence / "linked").unlink()
        (self.evidence / "bad\nfilename").write_text("not a checksum-safe name")
        with self.assertRaisesRegex(P8.Invalid, "unsafe relative"):
            self.archive()

    def test_local_pass_requires_raw_metrics_and_report(self):
        self.freeze()
        self.prepare_evidence()
        (self.evidence / "raw/sample.jsonl").unlink()
        with self.assertRaisesRegex(P8.Invalid, "raw evidence"):
            self.archive()
        self.prepare_evidence()
        (self.evidence / "report.md").write_text("")
        with self.assertRaises(P8.Invalid):
            self.archive()

    def test_partial_copy_remains_incomplete_and_retry_cannot_overwrite(self):
        original = P8.file_record
        def fail_copy(source, destination=None, **kwargs):
            if destination is not None and destination == self.candidate / "inputs/binary":
                raise OSError("simulated disk-full")
            return original(source, destination, **kwargs)
        with mock.patch.object(P8, "file_record", fail_copy):
            with self.assertRaises(OSError):
                self.freeze()
        self.assertTrue((self.candidate / "INCOMPLETE.json").exists())
        with self.assertRaisesRegex(P8.Invalid, "incomplete"):
            P8.verify_candidate(self.candidate)
        with self.assertRaises(FileExistsError):
            self.freeze()

    def test_evidence_drift_during_copy_retains_partial_and_prior_latest(self):
        self.freeze()
        self.prepare_evidence()
        self.archive(run_id="prior")
        latest_before = (self.runs / "latest.json").read_bytes()
        original = P8.inventory
        def mutate_after_snapshot(root, destination=None, budget=None):
            result = original(root, destination, budget)
            if root == self.evidence and destination is not None:
                (self.evidence / "raw/sample.jsonl").write_text("changed after copy\n")
            return result
        with mock.patch.object(P8, "inventory", mutate_after_snapshot):
            with self.assertRaisesRegex(P8.Invalid, "changed while archiving"):
                self.archive(run_id="raced")
        self.assertEqual((self.runs / "latest.json").read_bytes(), latest_before)
        partial = list(self.runs.glob("raced-*"))
        self.assertEqual(len(partial), 1)
        self.assertTrue((partial[0] / "INCOMPLETE.json").exists())

    def test_latest_symlink_or_unrelated_file_is_not_overwritten(self):
        self.freeze()
        self.prepare_evidence()
        self.runs.mkdir()
        target = self.root / "keep.json"
        self.write(target, {"keep": True})
        latest = self.runs / "latest.json"
        latest.symlink_to(target)
        with self.assertRaises(P8.Invalid):
            self.archive(run_id="linked")
        self.assertTrue(latest.is_symlink())
        self.assertEqual(json.loads(target.read_text()), {"keep": True})
        latest.unlink()
        self.write(latest, {"unrelated": True})
        with self.assertRaises(P8.Invalid):
            self.archive(run_id="unrelated")
        self.assertEqual(json.loads(latest.read_text()), {"unrelated": True})

    def test_cli_success_invalid_and_failed_gate_exit_codes(self):
        args = self.freeze_args()
        argv = ["freeze", "--source-root", args.source_root, "--source-path", "src",
                "--source-path", "Cargo.toml", "--output", args.output, "--build-profile", "fixture"]
        for role in ("binary", "config", "corpus", "scoring", "model"):
            argv.extend(["--" + role, getattr(args, role)])
        result = subprocess.run([sys.executable, str(SCRIPT), *argv], capture_output=True, text=True, timeout=30)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout)["status"], "frozen_local_inputs")
        self.prepare_evidence("gate_failed")
        with contextlib.redirect_stdout(io.StringIO()):
            code = P8.main(["archive", "--candidate", str(self.candidate), "--evidence", str(self.evidence),
                            "--output-root", str(self.runs), "--run-id", "failed", "--update-latest"])
        self.assertEqual(code, 1)
        self.assertFalse((self.runs / "latest.json").exists())
        (self.repo / "src/main.rs").write_text("different\n")
        stderr = io.StringIO()
        with contextlib.redirect_stderr(stderr):
            code = P8.main(["verify", "--candidate", str(self.candidate)])
        self.assertEqual(code, 2)
        self.assertEqual(json.loads(stderr.getvalue())["status"], "invalid_input")


if __name__ == "__main__":
    unittest.main()
