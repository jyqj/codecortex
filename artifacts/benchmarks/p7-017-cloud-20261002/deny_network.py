#!/usr/bin/env python3
"""Restrict this test process and descendants; no namespace/system changes.

Requires installed libseccomp and an already supported unprivileged filter
load. One attempt only: a rejected load fails this leg, without escalation.
Unix socket pairs remain available for local runtime IPC. Existing network
socket descriptors are closed before exec; new non-Unix sockets, connect,
socketcall and io_uring are denied. The report records the exact scope.
"""
import argparse
import ctypes
import errno
import json
import os
from pathlib import Path
import socket
import subprocess
import sys


class ArgCmp(ctypes.Structure):
    _fields_ = [("arg", ctypes.c_uint), ("op", ctypes.c_uint),
                ("datum_a", ctypes.c_uint64), ("datum_b", ctypes.c_uint64)]


def status():
    return {line.split(":", 1)[0]: line.split(":", 1)[1].strip()
            for line in Path("/proc/self/status").read_text().splitlines()
            if line.startswith(("NoNewPrivs:", "Seccomp:", "Seccomp_filters:"))}


def socket_probe(family):
    try:
        sock = socket.socket(family, socket.SOCK_STREAM)
    except OSError as exc:
        return {"blocked": exc.errno == errno.EPERM, "errno": exc.errno}
    sock.close()
    return {"blocked": False}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--report", required=True)
    parser.add_argument("--probe-only", action="store_true")
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    report = {"scope": "process_inherited_seccomp; not a network namespace",
              "before": status(), "system_settings_changed": False,
              "namespace_attempted": False, "credential_values_recorded": False}
    try:
        # There must be no already-open network descriptor available to the
        # product. Pipes and ordinary files are unchanged.
        closed = []
        for name in os.listdir("/proc/self/fd"):
            fd = int(name)
            try:
                if os.readlink(f"/proc/self/fd/{fd}").startswith("socket:"):
                    if fd <= 2:
                        raise RuntimeError("stdio is a socket; this run cannot claim network denial")
                    os.close(fd)
                    closed.append(fd)
            except OSError:
                pass
        report["closed_socket_fds"] = closed
        library = ctypes.CDLL("libseccomp.so.2", use_errno=True)
        library.seccomp_init.argtypes = [ctypes.c_uint32]
        library.seccomp_init.restype = ctypes.c_void_p
        library.seccomp_syscall_resolve_name.argtypes = [ctypes.c_char_p]
        library.seccomp_syscall_resolve_name.restype = ctypes.c_int
        library.seccomp_rule_add_array.argtypes = [ctypes.c_void_p, ctypes.c_uint32,
                                                  ctypes.c_int, ctypes.c_uint,
                                                  ctypes.POINTER(ArgCmp)]
        library.seccomp_rule_add_array.restype = ctypes.c_int
        library.seccomp_load.argtypes = [ctypes.c_void_p]
        library.seccomp_load.restype = ctypes.c_int
        library.seccomp_release.argtypes = [ctypes.c_void_p]
        context = library.seccomp_init(0x7FFF0000)  # SCMP_ACT_ALLOW
        if not context:
            raise RuntimeError("seccomp_init failed")
        denied = []
        unavailable = []
        try:
            for name in ["socket", "connect", "socketcall", "io_uring_setup",
                         "io_uring_enter", "io_uring_register"]:
                syscall = library.seccomp_syscall_resolve_name(name.encode())
                if syscall < 0:
                    unavailable.append(name)
                    continue
                # SCMP_CMP_NE: allow AF_UNIX local IPC only.
                compare = ArgCmp(0, 1, socket.AF_UNIX, 0)
                count = 1 if name == "socket" else 0
                result = library.seccomp_rule_add_array(
                    context, 0x00050000 | errno.EPERM, syscall, count,
                    ctypes.pointer(compare) if count else None)
                if result != 0:
                    raise RuntimeError(f"seccomp rule {name} rejected: {result}")
                denied.append(name)
            result = library.seccomp_load(context)
            if result != 0:
                raise RuntimeError(f"unprivileged seccomp_load rejected: {result}")
        finally:
            library.seccomp_release(context)
        report.update({"denied_syscalls": denied, "unavailable_syscalls": unavailable,
                       "after": status(), "ipv4": socket_probe(socket.AF_INET),
                       "ipv6": socket_probe(socket.AF_INET6)})
        left, right = socket.socketpair()
        left.close()
        right.close()
        report["local_unix_pair"] = "allowed"
        child = subprocess.run([sys.executable, "-c",
            "import socket,errno,json; r={}; "
            "exec('for f in [socket.AF_INET,socket.AF_INET6]:\\n"
            " try: socket.socket(f,socket.SOCK_STREAM); r[str(f)]=False\\n"
            " except OSError as e: r[str(f)]=e.errno==errno.EPERM'); "
            "print(json.dumps(r)); assert all(r.values())"],
            capture_output=True, text=True, check=True)
        report["exec_child_ipv4_ipv6_blocked"] = json.loads(child.stdout)
        if not report["ipv4"]["blocked"] or not report["ipv6"]["blocked"]:
            raise RuntimeError("network probes were not blocked")
        report["result"] = "passed_process_seccomp_scope"
    except Exception as exc:
        report.update({"result": "blocked", "reason": str(exc)})
        Path(args.report).write_text(json.dumps(report, indent=2) + "\n")
        return 1
    Path(args.report).write_text(json.dumps(report, indent=2) + "\n")
    if args.probe_only:
        return 0
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    if not command:
        raise ValueError("a command or --probe-only is required")
    env = dict(os.environ)
    for key in list(env):
        upper = key.upper()
        if upper.endswith(("_KEY", "_TOKEN")) or "API_KEY" in upper or "CREDENTIAL" in upper:
            del env[key]
    env["P7_017_NETWORK_POLICY"] = "process_seccomp_deny_network"
    os.execvpe(command[0], command, env)


if __name__ == "__main__":
    sys.exit(main())
