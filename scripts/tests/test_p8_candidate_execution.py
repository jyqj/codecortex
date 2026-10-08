"""Counterexamples for candidate/build bindings; fixtures never certify a product."""
import copy
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import p8_candidate_execution as candidate


class CandidateExecutionTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="p8-execution-controls-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.repo = self.root / "repo"
        files = {"Cargo.toml": "[workspace]\nmembers=[]\n", "Cargo.lock": "version=4\n",
                 "crates/cc-server/Cargo.toml": "[package]\nname='cc-server'\n",
                 "crates/cc-server/src/main.rs": "fn main() {}\n"}
        for relative, text in files.items():
            path = self.repo / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text)
        self.git("init", "-q")
        self.git("add", ".")
        self.git("-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid",
                 "commit", "-qm", "source fixture; not a product build")
        self.source = candidate.source_summary(self.repo)
        self.build = self.root / "build"
        self.build.mkdir()
        self.binary = self.build / "codecortex"
        self.binary.write_bytes(b"unit fixture bytes; never executed as a product")
        self.artifact = {"reason": "compiler-artifact", "features": [],
            "package_id": "path+file://" + str(self.repo / "crates/cc-server") + "#1.0.0",
            "manifest_path": str(self.repo / "crates/cc-server/Cargo.toml"),
            "target": {"name": "codecortex", "kind": ["bin"],
                       "src_path": str(self.repo / "crates/cc-server/src/main.rs")},
            "profile": {"test": False, "opt_level": "0"}, "executable": str(self.binary)}
        self.receipt = {"build_exit_code": 0, "source_before": self.source,
            "source_after": self.source, "binary_sha256": candidate.file_sha256(self.binary),
            "cargo_artifact": self.artifact, "build_profile": "dev",
            "build_command": ["cargo", "build", "--locked", "--no-default-features", "--bin", "codecortex"],
            "source_manifest": "source-inputs.json", "toolchain": {"fixture": "never executed"}}
        self.write(self.build / "source-inputs.json", candidate.source_snapshot(self.repo)["inputs"])
        self.write_log()
        self.save_receipt()

    def git(self, *args):
        return subprocess.check_output(["git", "-C", str(self.repo), *args], stderr=subprocess.DEVNULL)

    @staticmethod
    def write(path, value):
        path.write_text(json.dumps(value) + "\n")

    def write_log(self, finished=True):
        (self.build / "cargo-build.jsonl").write_text(json.dumps(self.artifact) + "\n" +
            json.dumps({"reason": "build-finished", "success": finished}) + "\n")

    def save_receipt(self):
        self.write(self.build / "build-receipt.json", self.receipt)

    def verify(self):
        return candidate.validate_build(self.binary, self.build / "build-receipt.json",
                                        self.repo, self.source, "product")

    def test_exact_binding_accepts_only_the_supplied_fixture_bytes(self):
        proof = self.verify()
        self.assertEqual(proof["binary_sha256"], candidate.file_sha256(self.binary))
        self.binary.write_bytes(b"different bytes at same path")
        with self.assertRaisesRegex(ValueError, "bytes disagree"):
            self.verify()

    def test_mixed_source_and_changed_raw_artifact_are_rejected(self):
        self.receipt["source_before"] = {**self.source, "source_commit": "a" * 40}
        self.save_receipt()
        with self.assertRaisesRegex(ValueError, "source differs"):
            self.verify()
        self.receipt["source_before"] = self.source
        self.save_receipt()
        self.artifact = {**self.artifact, "fresh": True}
        self.write_log()
        with self.assertRaisesRegex(ValueError, "retained Cargo output"):
            self.verify()

    def test_test_artifact_or_semantic_features_cannot_impersonate_default_product(self):
        initial = copy.deepcopy(self.receipt)
        for alteration in ("kind", "features"):
            self.receipt = copy.deepcopy(initial)
            if alteration == "kind":
                self.receipt["cargo_artifact"]["target"]["kind"] = ["test"]
            else:
                self.receipt["cargo_artifact"]["features"] = ["semantic"]
            self.save_receipt()
            with self.assertRaisesRegex(ValueError, "actual default local"):
                self.verify()

    def test_dev_cannot_be_relabelled_as_release(self):
        self.receipt["build_profile"] = "release"
        self.save_receipt()
        with self.assertRaisesRegex(ValueError, "build profile"):
            self.verify()

    def test_failed_build_finish_and_escaped_source_manifest_are_rejected(self):
        self.write_log(False)
        with self.assertRaisesRegex(ValueError, "successful finish"):
            self.verify()
        self.write_log()
        self.receipt["source_manifest"] = "../elsewhere.json"
        self.save_receipt()
        with self.assertRaisesRegex(ValueError, "manifest location"):
            self.verify()

    def test_printed_pass_does_not_replace_real_process_exit(self):
        output = self.root / "actual-command"
        receipt = candidate.execute([sys.executable, "-c", "print('passed'); raise SystemExit(7)"],
                                    output, self.repo, timeout=5)
        self.assertEqual(receipt["exit_code"], 7)
        self.assertEqual((output / "stdout.log").read_text().strip(), "passed")
        self.assertEqual(json.loads((output / "execution.json").read_text())["exit_code"], 7)
        with self.assertRaises(FileExistsError):
            candidate.execute([sys.executable, "-c", "pass"], output, self.repo, timeout=5)

    def test_cli_refuses_existing_output_without_adding_a_failure_record(self):
        output = self.root / "prior-run"
        output.mkdir()
        (output / "result.json").write_text('{"prior":"preserved"}\n')
        before = candidate.lock.inventory(output)
        process = subprocess.run([sys.executable, "-B", str(Path(candidate.__file__).resolve()),
                                  "build-runner", "--output", str(output)],
                                 capture_output=True, text=True, timeout=5)
        self.assertEqual(process.returncode, 2)
        self.assertEqual(candidate.lock.inventory(output), before)

    def test_drift_controls_only_mutate_disposable_copies(self):
        from test_p8_release_evidence import ReleaseEvidenceTests
        fixture = ReleaseEvidenceTests()
        fixture.setUp()
        self.addCleanup(fixture.doCleanups)
        for relative in (candidate.QUERIES, candidate.FIXTURE + "/provider.rs"):
            path = fixture.corpus / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("fixture content, never a product run\n")
        frozen = fixture.freeze()
        before = candidate.lock.inventory(fixture.candidate)
        records = candidate.drift_controls(fixture.candidate, frozen["candidate_sha256"],
                                           self.root / "controls")
        self.assertEqual(len(records), 7)
        self.assertTrue(all(row["rejected"] for row in records))
        self.assertEqual(candidate.lock.inventory(fixture.candidate), before)
        self.assertEqual(sorted(path.name for path in (self.root / "controls").iterdir()), ["results.json"])


if __name__ == "__main__":
    unittest.main()
