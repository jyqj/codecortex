"""P7 build/source controls; synthetic tool bytes are not a Rust product."""
import contextlib
import hashlib
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
import p7_build_identity as identity
import p7_stdio_build_receipt as builder


class BuildIdentityTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.directory = Path(self.temp.name)
        self.root = self.directory / "repo"
        self.root.mkdir()
        self.main = self.root / "crates/cc-server/src/main.rs"
        self.main.parent.mkdir(parents=True)
        self.main.write_text("fn main() {}\n")
        (self.root / "crates/cc-server/Cargo.toml").write_text('[package]\nname="cc-server"\n')
        (self.root / "Cargo.toml").write_text('[workspace]\nmembers=["crates/cc-server"]\n')
        (self.root / "Cargo.lock").write_text("version = 4\n")
        (self.main.parent / "中文 name.rs").write_text("// exact UTF-8 input\n")
        (self.root / ".gitignore").write_text("crates/**/ignored.rs\n")
        self.git("init", "--quiet")
        self.git("add", ".")
        self.git("-c", "user.name=Receipt control", "-c", "user.email=fixture@example.invalid",
                 "commit", "--quiet", "-m", "fixed synthetic source")
        self.binary = self.directory / "codecortex"
        self.binary.write_bytes(b"synthetic binary identity, not a Rust build\n")
        self.artifact = {
            "reason": "compiler-artifact", "package_id": str(self.root / "crates/cc-server") + "#1.0.0",
            "manifest_path": str(self.root / "crates/cc-server/Cargo.toml"),
            "target": {"name": "codecortex", "kind": ["bin"], "src_path": str(self.main)},
            "features": [], "profile": {"test": False, "opt_level": "0", "debug_assertions": True},
            "executable": str(self.binary),
        }

    def git(self, *args):
        return subprocess.check_output(["git", *args], cwd=self.root, stderr=subprocess.PIPE)

    def fake_tools(self, mode="ok", artifact=None):
        tool_dir = self.directory / "tools"
        tool_dir.mkdir(exist_ok=True)
        data = self.directory / "artifact.json"
        data.write_text(json.dumps(self.artifact if artifact is None else artifact))
        script = ("#!" + sys.executable + "\n" +
                  "import json, os, pathlib, shutil, sys\n"
                  "if '--version' in sys.argv:\n"
                  " print(pathlib.Path(sys.argv[0]).name + ' synthetic receipt fixture; no compiler')\n"
                  " sys.exit(0)\n"
                  "mode=os.environ['P7_BUILD_FIXTURE_MODE']\n"
                  "pathlib.Path(os.environ['P7_BUILD_FIXTURE_ENV']).write_text(json.dumps({k:os.environ.get(k) for k in ['RUSTC','RUSTC_WRAPPER','RUSTC_WORKSPACE_WRAPPER','CARGO_TARGET_DIR','CARGO_BUILD_BUILD_DIR']}))\n"
                  "if mode=='failed':\n"
                  " print('synthetic compiler failure', file=sys.stderr)\n"
                  " sys.exit(7)\n"
                  "if mode=='mutated':\n"
                  " pathlib.Path(os.environ['P7_BUILD_FIXTURE_MAIN']).write_text('changed while build ran\\n')\n"
                  "artifact=json.loads(pathlib.Path(os.environ['P7_BUILD_FIXTURE_ARTIFACT']).read_text())\n"
                  "target=pathlib.Path(os.environ['CARGO_TARGET_DIR'])/'debug'/'codecortex'\n"
                  "assert not target.parent.parent.exists(), 'reused build target'\n"
                  "target.parent.mkdir(parents=True)\n"
                  "shutil.copy2(artifact['executable'],target)\n"
                  "if mode!='foreign-target': artifact['executable']=str(target)\n"
                  "print(json.dumps(artifact))\n")
        for name in ("cargo", "rustc"):
            path = tool_dir / name
            path.write_text(script)
            path.chmod(0o755)
        return patch.dict(os.environ, {
            "PATH": str(tool_dir) + os.pathsep + os.environ["PATH"],
            "P7_BUILD_FIXTURE_MODE": mode,
            "P7_BUILD_FIXTURE_MAIN": str(self.main),
            "P7_BUILD_FIXTURE_ARTIFACT": str(data),
            "P7_BUILD_FIXTURE_ENV": str(self.directory / "invoked-environment.json"),
            "RUSTC": "", "RUSTC_WRAPPER": "", "RUSTC_WORKSPACE_WRAPPER": "",
        })

    def test_snapshot_is_bound_to_commit_and_exact_unicode_bytes(self):
        result = identity.source_snapshot(self.root)
        self.assertEqual(result["source_commit"], self.git("rev-parse", "HEAD").decode().strip())
        self.assertEqual(result["input_count"], 5)
        self.assertEqual(result["manifest_sha256"], hashlib.sha256(identity.json_bytes(result["inputs"])).hexdigest())
        path = "crates/cc-server/src/中文 name.rs"
        self.assertEqual(result["inputs"][path], hashlib.sha256((self.root / path).read_bytes()).hexdigest())

    def test_dirty_missing_extra_and_ignored_inputs_are_rejected(self):
        original = self.main.read_bytes()
        self.main.write_bytes(original + b"// uncommitted\n")
        with self.assertRaisesRegex(ValueError, "uncommitted"):
            identity.source_snapshot(self.root)
        self.main.unlink()
        with self.assertRaisesRegex(ValueError, "inventory"):
            identity.source_snapshot(self.root)
        self.main.write_bytes(original)
        for name in ("extra.rs", "ignored.rs"):
            path = self.main.parent / name
            path.write_text("// undeclared build input\n")
            with self.assertRaisesRegex(ValueError, "inventory"):
                identity.source_snapshot(self.root)
            path.unlink()
        identity.source_snapshot(self.root)

    def test_symlink_cannot_stand_in_for_equal_source_bytes(self):
        other = self.directory / "same-bytes.rs"
        other.write_bytes(self.main.read_bytes())
        self.main.unlink()
        self.main.symlink_to(other)
        with self.assertRaisesRegex(ValueError, "non-regular"):
            identity.source_snapshot(self.root)

    def test_equal_byte_symlink_inventory_root_is_rejected(self):
        original = self.root / "crates"
        moved = self.directory / "external-crates"
        original.rename(moved)
        original.symlink_to(moved, target_is_directory=True)
        with self.assertRaisesRegex(ValueError, "inventory root"):
            identity.source_snapshot(self.root)

    def test_selected_compiler_and_disabled_wrappers_reach_actual_build(self):
        output = self.directory / "selected-compiler"
        with self.fake_tools():
            selected = self.directory / "tools" / "selected-rustc"
            selected.write_bytes((self.directory / "tools" / "rustc").read_bytes())
            selected.chmod(0o755)
            with patch.dict(os.environ, {"RUSTC": str(selected), "RUSTC_WRAPPER": "unexpected-wrapper",
                                         "RUSTC_WORKSPACE_WRAPPER": "other-wrapper"}), \
                    contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(builder.build(self.root, output, "default", True), 0)
        receipt = json.loads((output / "build-receipt.json").read_text())
        observed = json.loads((self.directory / "invoked-environment.json").read_text())
        self.assertEqual(observed.pop("CARGO_TARGET_DIR"), str(output / "cargo-target"))
        self.assertEqual(observed.pop("CARGO_BUILD_BUILD_DIR"), str(output / "cargo-target"))
        self.assertEqual(observed, receipt["compiler_environment"])
        self.assertEqual(observed, {"RUSTC": str(selected), "RUSTC_WRAPPER": "", "RUSTC_WORKSPACE_WRAPPER": ""})
        self.assertEqual(receipt["toolchain"]["rustc"]["command"][0], str(selected))
        self.assertIn("selected-rustc synthetic", receipt["toolchain"]["rustc"]["version"])
        self.assertEqual(receipt["toolchain"]["rustc"]["executable_sha256"], identity.file_sha256(selected))

    def test_shared_target_is_overridden_and_foreign_product_rejected(self):
        shared = self.directory / "shared-target"
        shared.mkdir()
        (shared / "unrelated-product").write_bytes(b"immutable existing cache")
        with self.fake_tools(), patch.dict(os.environ, {"CARGO_TARGET_DIR": str(shared),
                                                       "CARGO_BUILD_BUILD_DIR": str(shared)}), \
                contextlib.redirect_stdout(io.StringIO()):
            output = self.directory / "private-target-run"
            self.assertEqual(builder.build(self.root, output, "default", True), 0)
        receipt = json.loads((output / "build-receipt.json").read_text())
        self.assertTrue(receipt["cargo_target_initially_absent"])
        self.assertEqual(receipt["cargo_target_dir"], str(output / "cargo-target"))
        self.assertEqual(receipt["cargo_build_dir"], str(output / "cargo-target"))
        observed = json.loads((self.directory / "invoked-environment.json").read_text())
        self.assertEqual(observed["CARGO_BUILD_BUILD_DIR"], str(output / "cargo-target"))
        self.assertEqual((shared / "unrelated-product").read_bytes(), b"immutable existing cache")
        bad = self.directory / "foreign-target-run"
        with self.fake_tools("foreign-target"), contextlib.redirect_stdout(io.StringIO()):
            with self.assertRaisesRegex(ValueError, "private target"):
                builder.build(self.root, bad, "default", True)
        self.assertFalse((bad / "build-receipt.json").exists())

    def test_compiler_proxy_keeps_its_invocation_basename(self):
        with self.fake_tools():
            proxy = self.directory / "tools" / "rustc"
            real = self.directory / "tools" / "tool-proxy"
            proxy.rename(real)
            proxy.symlink_to(real)
            env, overrides = builder.compiler_environment(self.root)
            toolchain = builder.toolchain_identity(self.root, env)
        self.assertEqual(overrides["RUSTC"], str(proxy))
        self.assertEqual(toolchain["rustc"]["executable"], str(real))
        self.assertEqual(toolchain["rustc"]["command"][0], str(proxy))
        self.assertIn("rustc synthetic", toolchain["rustc"]["version"])

    def test_foreign_checkout_and_test_artifacts_are_rejected(self):
        self.assertEqual(identity.verify_cargo_artifact(self.root, self.artifact, "default"), self.binary)
        for key, value in (("manifest_path", str(self.root / "Cargo.toml")),
                           ("features", ["semantic"]), ("profile", {"test": True})):
            bad = dict(self.artifact)
            bad[key] = value
            with self.assertRaises(ValueError):
                identity.verify_cargo_artifact(self.root, bad, "default")

    def test_receipt_captures_source_compiler_profile_and_binary(self):
        output = self.directory / "good-run"
        with self.fake_tools(), contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(builder.build(self.root, output, "default", True), 0)
        receipt = json.loads((output / "build-receipt.json").read_text())
        self.assertEqual(receipt["source_before"], receipt["source_after"])
        self.assertEqual(receipt["source_before"]["manifest_sha256"], identity.file_sha256(output / "source-inputs.json"))
        self.assertEqual(receipt["binary_sha256"], identity.file_sha256(output / "codecortex"))
        self.assertEqual(receipt["build_profile"], "dev")
        self.assertEqual(receipt["build_command"][-1], "--offline")
        self.assertIn("synthetic receipt fixture", receipt["toolchain"]["rustc"]["version"])

    def test_source_mutation_and_compiler_failure_cannot_produce_receipt(self):
        for mode in ("failed", "mutated"):
            with self.subTest(mode=mode):
                output = self.directory / mode
                with self.fake_tools(mode), contextlib.redirect_stdout(io.StringIO()):
                    if mode == "failed":
                        self.assertEqual(builder.build(self.root, output, "default", False), 7)
                    else:
                        with self.assertRaisesRegex(ValueError, "uncommitted"):
                            builder.build(self.root, output, "default", False)
                self.assertFalse((output / "build-receipt.json").exists())
                failure = json.loads((output / "build-failure.json").read_text())
                self.assertEqual(failure["build_exit_code"], 7 if mode == "failed" else 0)
                self.assertTrue((output / "cargo-build.stderr.log").is_file())

    def test_existing_evidence_is_unchanged_on_retry(self):
        output = self.directory / "existing"
        output.mkdir()
        receipt = output / "build-receipt.json"
        receipt.write_bytes(b"immutable previous observation\n")
        with self.assertRaises(FileExistsError):
            builder.build(self.root, output, "default", True)
        self.assertEqual(receipt.read_bytes(), b"immutable previous observation\n")


if __name__ == "__main__":
    unittest.main()
