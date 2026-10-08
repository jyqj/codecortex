"""Real per-process controls for the P7 offline launcher; no product claims."""
import importlib.util
import json
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import unittest
from unittest import mock


SCRIPT = Path(__file__).resolve().parents[1] / "p7_offline_network.py"
spec = importlib.util.spec_from_file_location("p7_offline_network", SCRIPT)
guard = importlib.util.module_from_spec(spec)
spec.loader.exec_module(guard)


@unittest.skipUnless(sys.platform == "linux", "Linux seccomp evidence only")
class NetworkGuardTests(unittest.TestCase):
    def test_actual_ipv4_ipv6_socket_attempts_are_process_fatal(self):
        with tempfile.TemporaryDirectory() as temporary:
            report = guard.verify_kill_policy(Path(temporary) / "probes")
        self.assertEqual(report["status"], "passed")
        for family in ("ipv4", "ipv6"):
            probe = report["probes"][family]
            self.assertEqual(probe["exit_code"], -signal.SIGSYS)
            self.assertTrue(probe["reached_socket"])
            self.assertEqual(probe["pre_exec"]["action"], "kill")
            self.assertEqual(probe["pre_exec"]["socket_fds_before_exec"], [])

    def test_errno_parent_policy_is_inherited_across_exec(self):
        code = "import json,socket,errno; r=[]\nfor f in (socket.AF_INET,socket.AF_INET6):\n try: socket.socket(f,socket.SOCK_STREAM)\n except OSError as e: r.append(e.errno)\nprint(json.dumps(r))"
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "parent.json"
            result = subprocess.run([sys.executable, str(SCRIPT), "--receipt", str(path),
                                     "--", sys.executable, "-c", code],
                                    capture_output=True, text=True, timeout=20)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(json.loads(result.stdout), [1, 1])
            record = json.loads(path.read_text())
        self.assertEqual(record["ipv4"], {"blocked": True, "errno": 1})
        self.assertEqual(record["ipv6"], {"blocked": True, "errno": 1})
        self.assertEqual(int(record["after"]["Seccomp_filters"]),
                         int(record["before"]["Seccomp_filters"]) + 1)

    def test_kill_policy_runs_a_non_network_command_with_distinct_identity(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "product.json"
            result = subprocess.run([sys.executable, str(SCRIPT), "--action", "kill",
                                     "--receipt", str(path), "--", "/usr/bin/true"],
                                    capture_output=True, text=True, timeout=20)
            self.assertEqual(result.returncode, 0, result.stderr)
            record = json.loads(path.read_text())
        self.assertEqual(record["exec_sha256"], guard.digest("/usr/bin/true"))
        self.assertEqual(record["wrapper_sha256"], guard.digest(SCRIPT))
        self.assertEqual(record["launcher_python_sha256"], guard.digest(sys.executable))
        self.assertNotEqual(record["exec_sha256"], record["wrapper_sha256"])

    def test_existing_receipt_is_rejected_without_overwrite(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "old.json"
            path.write_bytes(b"original evidence\n")
            result = subprocess.run([sys.executable, str(SCRIPT), "--receipt", str(path),
                                     "--", "/usr/bin/true"], capture_output=True, timeout=20)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(path.read_bytes(), b"original evidence\n")

    def test_anonymous_unix_stream_ipc_works_under_both_network_policies(self):
        # Match mio's actual libc socketpair + ordinary read/write boundary.
        # Python's high-level socket wrapper additionally probes socket metadata;
        # it is not the signal-IPC implementation being admitted here.
        code = ("import ctypes,os,socket; libc=ctypes.CDLL(None,use_errno=True); "
                "libc.socketpair.argtypes=[ctypes.c_int,ctypes.c_int,ctypes.c_int,ctypes.POINTER(ctypes.c_int)]; "
                "fds=(ctypes.c_int*2)(); "
                "assert libc.socketpair(socket.AF_UNIX,socket.SOCK_STREAM|socket.SOCK_NONBLOCK|socket.SOCK_CLOEXEC,0,fds)==0; "
                "os.write(fds[0],b'ipc'); assert os.read(fds[1],3)==b'ipc'; os.close(fds[0]); os.close(fds[1])")
        for action in ('errno', 'kill'):
            with self.subTest(action=action), tempfile.TemporaryDirectory() as temporary:
                receipt = Path(temporary) / 'ipc.json'
                result = subprocess.run([sys.executable, str(SCRIPT), '--action', action,
                                         '--receipt', str(receipt), '--', sys.executable, '-c', code],
                                        capture_output=True, text=True, timeout=20)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(json.loads(receipt.read_text())['local_ipc_policy'], guard.LOCAL_IPC_POLICY)

    def test_external_pair_addressable_unix_socket_and_connect_are_still_fatal(self):
        for attempt in ('socket.socketpair(socket.AF_INET,socket.SOCK_STREAM,0)',
                        'socket.socketpair(socket.AF_UNIX,socket.SOCK_STREAM,1)',
                        'socket.socket(socket.AF_UNIX,socket.SOCK_STREAM)',
                        "libc.connect(fds[0],None,0)"):
            with self.subTest(attempt=attempt), tempfile.TemporaryDirectory() as temporary:
                setup = ("import socket,ctypes; libc=ctypes.CDLL(None); fds=(ctypes.c_int*2)(); "
                         "libc.socketpair.argtypes=[ctypes.c_int,ctypes.c_int,ctypes.c_int,ctypes.POINTER(ctypes.c_int)]; "
                         "assert libc.socketpair(socket.AF_UNIX,socket.SOCK_STREAM,0,fds)==0; "
                         "print('forbidden syscall reached',flush=True); ")
                result = subprocess.run([sys.executable, str(SCRIPT), '--action', 'kill',
                                         '--receipt', str(Path(temporary) / 'denied.json'), '--',
                                         sys.executable, '-c', setup + attempt],
                                        capture_output=True, text=True, timeout=20)
                self.assertEqual(result.stdout.strip(), 'forbidden syscall reached')
                self.assertEqual(result.returncode, -signal.SIGSYS, result.stderr)

    def test_normal_exit_cannot_pass_the_kill_positive_control(self):
        with tempfile.TemporaryDirectory() as temporary, mock.patch.object(
                guard.subprocess, "run", return_value=subprocess.CompletedProcess([], 0, "", "")):
            with self.assertRaisesRegex(RuntimeError, "did not reach the socket"):
                guard.verify_kill_policy(Path(temporary) / "probes")

    def test_early_sigsys_cannot_pass_without_reaching_the_socket(self):
        with tempfile.TemporaryDirectory() as temporary, mock.patch.object(
                guard.subprocess, "run", return_value=subprocess.CompletedProcess([], -signal.SIGSYS, "", "")):
            with self.assertRaisesRegex(RuntimeError, "did not reach the socket"):
                guard.verify_kill_policy(Path(temporary) / "probes")


if __name__ == "__main__":
    unittest.main()
