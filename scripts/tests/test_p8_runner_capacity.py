import os
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import p8_runner_capacity as capacity


class HostedCapacitySafetyTests(unittest.TestCase):
    def test_current_workspace_and_self_hosted_execution_cannot_invoke_a_command(self):
        environments = ({}, {"GITHUB_ACTIONS": "true", "RUNNER_ENVIRONMENT": "self-hosted"},
                        {"GITHUB_ACTIONS": "true", "GITHUB_REPOSITORY": "another/repo",
                         "RUNNER_ENVIRONMENT": "github-hosted", "RUNNER_OS": "Linux"})
        for environment in environments:
            with self.subTest(environment=environment), patch.dict(os.environ, environment, clear=True), \
                    patch.object(capacity.subprocess, "run") as execution:
                with self.assertRaises(RuntimeError):
                    capacity.prepare(100000, Path("/workspace/forbidden"), True)
                execution.assert_not_called()

    def test_registered_requirement_is_not_discounted_for_available_space(self):
        self.assertEqual(capacity.required_bytes(100000), 30_509_367_296)
        for invalid in (0, True, 100001, "100000"):
            with self.assertRaises(RuntimeError):
                capacity.required_bytes(invalid)

    def test_caller_supplied_cleanup_paths_are_never_accepted(self):
        for target in (Path("/"), Path("/home/runner/work"), Path("/tmp"), Path("/usr")):
            with self.assertRaises(RuntimeError):
                capacity.safe_sdk(target, [])

    def test_symlink_ancestor_and_protected_overlap_are_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            sdk = root / "sdk"
            sdk.mkdir()
            (root / "alias").symlink_to(sdk, target_is_directory=True)
            with patch.object(capacity, "SDK_PATHS", (sdk, root / "alias")):
                self.assertTrue(capacity.safe_sdk(sdk, [root / "unrelated"]))
                for protected in ([root], [sdk], [sdk / "nested"]):
                    with self.assertRaises(RuntimeError):
                        capacity.safe_sdk(sdk, protected)
                with self.assertRaises(RuntimeError):
                    capacity.safe_sdk(root / "alias", [])

    def test_fixed_privileged_argv_uses_same_privilege_group_supervision(self):
        operation = ["rm", "-rf", "--one-file-system", "--preserve-root=all", "--", str(capacity.SDK_PATHS[0])]
        self.assertEqual(capacity.privileged_argv(operation),
                         ["sudo", "-n", "timeout", "--signal=TERM", "--kill-after=5s", "300s", *operation])
        with self.assertRaises(RuntimeError):
            capacity.privileged_argv(["rm", "-rf", "/"])

    def test_real_unprivileged_failure_keeps_started_argv_and_terminal_receipt(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            actual = [sys.executable, "-c", "import sys; print('original failure'); sys.exit(7)"]
            with patch.object(capacity, "privileged_argv", return_value=actual):
                result = capacity.command(["test-only-unprivileged-executor"], root, "failure")
            saved = json.loads((root / "failure.command.json").read_text())
            self.assertEqual(saved, result)
            self.assertEqual(result["argv"], actual)
            self.assertEqual(result["exit_code"], 7)
            self.assertTrue(result["cleanup_complete"])
            self.assertIn("original failure", (root / "failure.stdout").read_text())

    def test_real_unprivileged_timeout_kills_parent_and_its_child_group(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            # EOF from the exact inherited stdout pipe proves no descendant
            # still owns its writer, without /proc or cross-namespace PID guesses.
            child_source = r"""
import signal,sys,time
signal.signal(signal.SIGTERM, signal.SIG_IGN)
sys.stdout.write('owned-child-ready\n')
sys.stdout.flush()
for _ in range(2000):
    sys.stdout.write('x')
    sys.stdout.flush()
    time.sleep(0.01)
"""
            source = ("import subprocess,signal,time,sys; "
                      "signal.signal(signal.SIGTERM,signal.SIG_IGN); "
                      "subprocess.Popen([sys.executable,'-c',sys.argv[1]]); time.sleep(60)")
            inner = ["timeout", "--signal=TERM", "--kill-after=0.2s", "2s", sys.executable,
                     "-c", source, child_source]
            probe = r"""
import json,subprocess,sys
process = subprocess.Popen(json.loads(sys.argv[1]), stdin=subprocess.DEVNULL,
                           stdout=subprocess.PIPE, stderr=subprocess.PIPE)
try:
    stdout, stderr = process.communicate(timeout=4)
except subprocess.TimeoutExpired:
    process.kill()
    process.stdout.close()
    process.stderr.close()
    raise SystemExit('owned descendant retained the stdout writer after deadline')
assert process.returncode in (124,137,-9), stderr
assert stdout.startswith(b'owned-child-ready\n'), (stdout, stderr)
print(json.dumps({'deadline_exit':process.returncode,'pipe_eof':True,'original_stdout':stdout.decode()}))
"""
            actual = [sys.executable, "-c", probe, json.dumps(inner)]
            with patch.object(capacity, "privileged_argv", return_value=actual):
                result = capacity.command(["test-only-unprivileged-executor"], root, "timeout")
            self.assertEqual(result["exit_code"], 0, (root / "timeout.stderr").read_text())
            self.assertTrue(result["cleanup_complete"])
            self.assertLess(result["elapsed_ns"], 5_000_000_000)
            observed = json.loads((root / "timeout.stdout").read_text())
            self.assertTrue(observed["pipe_eof"])
            self.assertIn(observed["deadline_exit"], (124, 137, -9))

    def test_outer_timeout_is_explicitly_unsealed_and_keeps_actual_argv(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            actual = ["test-only-unstarted-supervisor"]
            with patch.object(capacity, "privileged_argv", return_value=actual), \
                    patch.object(capacity.subprocess, "run", side_effect=subprocess.TimeoutExpired(actual, 320)):
                result = capacity.command(["test-only-executor"], root, "outer-timeout")
            self.assertFalse(result["cleanup_complete"])
            self.assertIsNone(result["exit_code"])
            self.assertEqual(result["argv"], actual)
            self.assertIn("cleanup_unconfirmed", result["status"])


if __name__ == "__main__":
    unittest.main()
