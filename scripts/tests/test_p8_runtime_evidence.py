"""Adversarial receipt/seal controls. Tiny bytes here are not Cargo build evidence."""
from contextlib import contextmanager
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import p8_runtime_build as evidence


@contextmanager
def fixture_modules(root):
    names = {}
    for relative in evidence.OBSERVER_FILES:
        name = relative.removeprefix("scripts/").removesuffix(".py").replace("/", ".")
        names[name.removesuffix(".__init__")] = None
    with mock.patch.object(evidence, "__file__", str(root / "scripts/p8_runtime_build.py")), \
            mock.patch.dict(sys.modules, names):
        yield


class ReceiptFixture:
    def __init__(self, directory):
        self.root = directory / "checkout"
        self.root.mkdir()
        self.out = directory / "build"
        self.out.mkdir()
        self.target = directory / "target"
        self.target.mkdir()
        for relative in evidence.OBSERVER_FILES:
            path = self.root / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("# committed protocol-control fixture: " + relative + "\n")
        for filename in ("Cargo.toml", "Cargo.lock"):
            (self.root / filename).write_text("# protocol fixture only; no compilation\n")
        for _, (package, source, _, _) in evidence.TARGETS.items():
            for relative in (f"crates/{package}/Cargo.toml", f"crates/{package}/{source}"):
                path = self.root / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text("// committed protocol fixture\n")
        subprocess.run(["git", "init", "-q", str(self.root)], check=True)
        subprocess.run(["git", "add", "."], cwd=self.root, check=True)
        subprocess.run(["git", "-c", "user.name=Protocol controls", "-c", "user.email=controls@example.invalid",
                        "commit", "-qm", "receipt fixture"], cwd=self.root, check=True)
        source = evidence.source_snapshot(self.root)
        with fixture_modules(self.root):
            observer = evidence.observer_snapshot(self.root)
        self.receipt = dict(schema_version=2, status="passed", build_exit_code=0,
                            source_before=source, source_after=source,
                            observer_before=observer, observer_after=observer,
                            target_dir=str(self.target), build_command=evidence.command_for(self.target), artifacts={})
        for when in ("before", "after"):
            evidence.write_json(self.out / f"source-{when}.json", source)
        for relative in evidence.OBSERVER_FILES:
            destination = self.out / "observer-source" / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(self.root / relative, destination)
        self.messages = []
        self.binaries = {}
        for name, (package, source_file, prefix, event_field) in evidence.TARGETS.items():
            original, retained = self.target / name, self.out / name
            original.write_bytes(("protocol-control bytes for " + name).encode())
            shutil.copy2(original, retained)
            artifact = dict(reason="compiler-artifact", manifest_path=str(self.root / "crates" / package / "Cargo.toml"),
                            target=dict(name=name, kind=["bin"], src_path=str(self.root / "crates" / package / source_file)),
                            profile=dict(opt_level="3", test=False, debug_assertions=False), features=[],
                            executable=str(original))
            self.messages.append(artifact)
            self.receipt["artifacts"][name] = dict(cargo_artifact=artifact, binary_path=str(retained),
                                                  binary_sha256=evidence.digest(retained), binary_bytes=retained.stat().st_size,
                                                  copy_source=dict(path=str(original), bytes=original.stat().st_size,
                                                                   sha256=evidence.digest(original)))
            self.receipt.update({prefix + "_path": str(retained), prefix + "_sha256": evidence.digest(retained),
                                 event_field: artifact})
            self.binaries[name] = retained
        self.messages.append(dict(reason="build-finished", success=True))
        (self.out / "product-build.stderr").write_text("protocol-control log, no compiler executed\n")
        self.resign_control()

    def resign_control(self):
        """Intentionally self-consistent adversarial fixtures must still fail semantic binding."""
        (self.out / "product-build.jsonl").write_text("".join(json.dumps(row) + "\n" for row in self.messages))
        self.receipt.update(cargo_log_sha256=evidence.digest(self.out / "product-build.jsonl"),
                            stderr_sha256=evidence.digest(self.out / "product-build.stderr"))
        evidence.write_json(self.out / "build-receipt.json", self.receipt)
        (self.out / "seal.json").unlink(missing_ok=True)
        evidence.seal_output(self.out)

    def verify(self):
        with fixture_modules(self.root):
            return evidence.verify_receipt(self.root, self.out / "build-receipt.json", self.binaries)


class RuntimeBuildBindingTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.fixture = ReceiptFixture(Path(self.temporary.name))

    def test_complete_contract_checks_every_original_and_retained_artifact(self):
        result = self.fixture.verify()
        self.assertEqual(set(result["artifacts"]), set(evidence.TARGETS))
        self.assertEqual(result["observer"]["source_commit"], result["source"]["source_commit"])

    def test_foreign_checkout_with_identical_suffix_is_rejected(self):
        fixture = self.fixture
        artifact = fixture.receipt["artifacts"]["p8-runtime-statistics"]["cargo_artifact"]
        for owner, key in ((artifact, "manifest_path"), (artifact["target"], "src_path")):
            original = owner[key]
            owner[key] = original.replace("/checkout/", "/another-checkout/")
            fixture.resign_control()
            with self.assertRaisesRegex(ValueError, "another source checkout"):
                fixture.verify()
            owner[key] = original

    def test_missing_or_outside_target_original_executable_is_rejected(self):
        fixture = self.fixture
        artifact = fixture.receipt["artifacts"]["p8-oracle"]["cargo_artifact"]
        original = artifact.pop("executable")
        fixture.resign_control()
        with self.assertRaisesRegex(ValueError, "executable is missing"):
            fixture.verify()
        outside = Path(self.temporary.name) / "p8-oracle"
        outside.write_bytes(Path(original).read_bytes())
        artifact["executable"] = str(outside)
        fixture.resign_control()
        with self.assertRaisesRegex(ValueError, "outside the recorded target"):
            fixture.verify()

    def test_replaced_binary_cannot_be_rebound_with_only_a_self_reported_hash(self):
        fixture = self.fixture
        binary = fixture.binaries["codecortex"]
        binary.write_bytes(b"unrelated bytes with a consistently rewritten receipt")
        fixture.receipt["binary_sha256"] = evidence.digest(binary)
        fixture.receipt["artifacts"]["codecortex"].update(binary_sha256=evidence.digest(binary),
                                                          binary_bytes=binary.stat().st_size)
        fixture.resign_control()
        with self.assertRaisesRegex(ValueError, "actual original Cargo copy"):
            fixture.verify()

    def test_second_check_rejects_original_executable_changed_after_admission(self):
        fixture = self.fixture
        fixture.verify()
        (fixture.target / "p8-oracle").write_bytes(b"changed original producer after admission")
        with self.assertRaisesRegex(ValueError, "actual original Cargo copy"):
            fixture.verify()

    def test_deleted_original_target_does_not_gain_cross_job_build_approval(self):
        fixture = self.fixture
        fixture.verify()
        shutil.rmtree(fixture.target)
        evidence.verify_output(fixture.out)  # Archive integrity remains independently checkable.
        with self.assertRaises(FileNotFoundError):
            fixture.verify()

    def test_source_and_observer_changes_are_rejected_after_initial_verification(self):
        fixture = self.fixture
        fixture.verify()
        for relative in ("crates/cc-server/src/main.rs", "scripts/p8_runtime.py"):
            path = fixture.root / relative
            before = path.read_bytes()
            path.write_bytes(before + b"// changed after admission\n")
            with self.assertRaises(ValueError):
                fixture.verify()
            path.write_bytes(before)
        fixture.verify()

    def test_rewritten_command_or_missing_statistics_artifact_is_rejected(self):
        fixture = self.fixture
        fixture.receipt["build_command"].remove("--release")
        fixture.resign_control()
        with self.assertRaisesRegex(ValueError, "Cargo invocation"):
            fixture.verify()
        fixture.receipt["build_command"] = evidence.command_for(fixture.target)
        fixture.receipt["artifacts"].pop("p8-runtime-statistics")
        fixture.resign_control()
        with self.assertRaisesRegex(ValueError, "complete product/oracle/statistics"):
            fixture.verify()

    def test_loaded_foreign_module_cannot_borrow_a_committed_file_hash(self):
        fixture = self.fixture
        with fixture_modules(fixture.root), mock.patch.dict(sys.modules, {
                "p8_cold_build": SimpleNamespace(__file__=str(Path(self.temporary.name) / "foreign.py"))}):
            with self.assertRaisesRegex(ValueError, "loaded observer module belongs"):
                evidence.observer_snapshot(fixture.root)

    def test_original_log_mutation_is_detected_by_the_build_seal(self):
        fixture = self.fixture
        (fixture.out / "product-build.jsonl").write_text("{}\n")
        with self.assertRaisesRegex(ValueError, "inventory changed"):
            fixture.verify()


class RuntimeArtifactSealTests(unittest.TestCase):
    def test_every_decision_input_including_stderr_plan_rpc_and_sqlite_is_sealed(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            names = ("plan.json", "raw.jsonl", "report.json", "product/product-stderr.log",
                     "product/rpc.jsonl", "product/process.json", "parity.json", "statistics.json",
                     "statistics-replay.json", "project/stable.py", "project/.codecortex/index.sqlite3",
                     "project/.codecortex/index.sqlite3-wal", "project/.codecortex/index.sqlite3-shm",
                     "build-evidence/seal.json")
            for name in names:
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(name.encode())
            sealed = evidence.seal_output(root)
            self.assertEqual(set(sealed["artifact_inventory"]), set(names))
            evidence.verify_output(root)
            for name in names:
                path = root / name
                before = path.read_bytes()
                path.write_bytes(before + b" changed")
                with self.assertRaises(ValueError):
                    evidence.verify_output(root)
                path.write_bytes(before)
            evidence.verify_output(root)
            with self.assertRaisesRegex(ValueError, "already sealed"):
                evidence.seal_output(root)

    def test_missing_extra_and_symlink_artifacts_are_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            raw = root / "raw.jsonl"
            raw.write_bytes(b"original raw\n")
            evidence.seal_output(root)
            raw.unlink()
            with self.assertRaises(ValueError):
                evidence.verify_output(root)
            raw.write_bytes(b"original raw\n")
            extra = root / "extra"
            extra.write_bytes(b"unexpected")
            with self.assertRaises(ValueError):
                evidence.verify_output(root)
            extra.unlink()
            extra.symlink_to(raw)
            with self.assertRaisesRegex(ValueError, "nonregular"):
                evidence.verify_output(root)


if __name__ == "__main__":
    unittest.main()
