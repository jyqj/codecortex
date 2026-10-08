#!/usr/bin/env python3
"""Apply process-local seccomp before exec; never weaken inherited restrictions.

The parent test runner uses EPERM probes. Each real product instead uses
KILL_PROCESS. A surviving parent can still swallow a descendant's SIGSYS, so
zero attempts additionally requires p7_offline_trace.py's complete tree check.
This helper is a test launcher, not the product whose build receipt is checked.
"""
import argparse
import ctypes
import ctypes.util
import errno
import hashlib
import json
import os
from pathlib import Path
import resource
import signal
import socket
import stat
import subprocess
import sys


ALLOW, KILL_PROCESS, ERRNO = 0x7FFF0000, 0x80000000, 0x00050000
LOCAL_IPC_POLICY = {"syscall": "socketpair", "family": "AF_UNIX", "protocol": 0,
                    "scope": "anonymous local IPC only; no socket/bind/connect exception"}
NETWORK_SYSCALLS = (
    "socket", "socketpair", "connect", "bind", "listen", "accept", "accept4",
    "sendto", "recvfrom", "sendmsg", "recvmsg", "sendmmsg", "recvmmsg",
    "getsockname", "getpeername", "setsockopt", "getsockopt", "shutdown",
    "socketcall", "io_uring_setup", "io_uring_enter", "io_uring_register",
)


class ArgumentComparison(ctypes.Structure):
    _fields_ = [("arg", ctypes.c_uint), ("op", ctypes.c_int),
                ("datum_a", ctypes.c_uint64), ("datum_b", ctypes.c_uint64)]


def digest(path):
    hasher = hashlib.sha256()
    with Path(path).open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            hasher.update(block)
    return hasher.hexdigest()


def security_status():
    wanted = {"NoNewPrivs", "Seccomp", "Seccomp_filters", "Pid", "PPid", "NSpid"}
    return {key: value.strip()
            for line in Path("/proc/self/status").read_text().splitlines()
            if ":" in line for key, value in [line.split(":", 1)] if key in wanted}


def socket_fds():
    result = []
    for name in os.listdir("/proc/self/fd"):
        descriptor = int(name)
        try:
            mode = os.fstat(descriptor).st_mode
        except OSError as error:
            if error.errno == errno.EBADF:
                continue
            raise
        if stat.S_ISSOCK(mode):
            result.append(descriptor)
    return sorted(result)


def install_filter(action):
    name = ctypes.util.find_library("seccomp")
    if not name:
        raise RuntimeError("libseccomp unavailable; isolation is not certified")
    library = ctypes.CDLL(name, use_errno=True)
    library.seccomp_init.argtypes = [ctypes.c_uint32]
    library.seccomp_init.restype = ctypes.c_void_p
    library.seccomp_syscall_resolve_name.argtypes = [ctypes.c_char_p]
    library.seccomp_syscall_resolve_name.restype = ctypes.c_int
    library.seccomp_rule_add_array.argtypes = [ctypes.c_void_p, ctypes.c_uint32,
                                              ctypes.c_int, ctypes.c_uint, ctypes.c_void_p]
    library.seccomp_rule_add_array.restype = ctypes.c_int
    library.seccomp_attr_set.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_uint32]
    library.seccomp_attr_set.restype = ctypes.c_int
    library.seccomp_load.argtypes = [ctypes.c_void_p]
    library.seccomp_load.restype = ctypes.c_int
    library.seccomp_release.argtypes = [ctypes.c_void_p]
    library.seccomp_release.restype = None
    context = library.seccomp_init(ALLOW)
    if not context:
        raise RuntimeError("seccomp_init failed")
    denied, unavailable, local_ipc = [], [], None
    try:
        # SCMP_FLTATR_ACT_BADARCH = 2. An ABI switch cannot evade the native
        # syscall rules by only terminating one thread in a multithreaded child.
        if library.seccomp_attr_set(context, 2, KILL_PROCESS) != 0:
            raise RuntimeError("seccomp bad-architecture process-kill rule failed")
        for syscall in NETWORK_SYSCALLS:
            number = library.seccomp_syscall_resolve_name(syscall.encode("ascii"))
            if number < 0:
                unavailable.append(syscall)
                continue
            if syscall == "socketpair":
                # Tokio signal delivery needs an anonymous Unix stream pair.
                # No endpoint address is supplied by socketpair. All other
                # domains and nonzero protocols remain denied, as do ordinary
                # Unix socket/bind/connect calls. Trace acceptance additionally
                # restricts observed pairs to the actual SOCK_STREAM IPC form.
                for argument, value in ((0, int(socket.AF_UNIX)), (2, 0)):
                    comparison = ArgumentComparison(argument, 1, value, 0)  # SCMP_CMP_NE
                    result = library.seccomp_rule_add_array(context, action, number, 1,
                                                           ctypes.byref(comparison))
                    if result != 0:
                        raise RuntimeError(f"conditional socketpair rule failed: {result}")
                local_ipc = dict(LOCAL_IPC_POLICY)
                continue
            result = library.seccomp_rule_add_array(context, action, number, 0, None)
            if result != 0:
                raise RuntimeError(f"seccomp rule {syscall} failed: {result}")
            denied.append(syscall)
        if not {"socket", "connect", "io_uring_setup"}.issubset(denied) or local_ipc is None:
            raise RuntimeError("essential network syscall rules unavailable")
        libc = ctypes.CDLL(None, use_errno=True)
        libc.prctl.argtypes = [ctypes.c_int, ctypes.c_ulong, ctypes.c_ulong,
                              ctypes.c_ulong, ctypes.c_ulong]
        libc.prctl.restype = ctypes.c_int
        if libc.prctl(38, 1, 0, 0, 0) != 0:  # PR_SET_NO_NEW_PRIVS
            raise OSError(ctypes.get_errno(), "PR_SET_NO_NEW_PRIVS")
        result = library.seccomp_load(context)
        if result != 0:
            raise RuntimeError(f"seccomp_load failed: {result}")
    finally:
        library.seccomp_release(context)
    return denied, unavailable, local_ipc


def errno_probe(family):
    try:
        with socket.socket(family, socket.SOCK_STREAM):
            pass
    except OSError as error:
        if error.errno == errno.EPERM:
            return {"blocked": True, "errno": errno.EPERM}
        raise
    raise RuntimeError("network socket creation remained possible")


def verify_kill_policy(output):
    output.mkdir(parents=True, exist_ok=False)
    results = {}
    for name, family in [("ipv4", socket.AF_INET), ("ipv6", socket.AF_INET6)]:
        receipt = output / f"{name}.json"
        marker = f"P7_NETWORK_PROBE_REACHED family={int(family)}"
        code = ("import socket; " + f"print({marker!r}, flush=True); "
                + f"socket.socket({int(family)},socket.SOCK_STREAM)")
        command = [sys.executable, str(Path(__file__).resolve()), "--action", "kill",
                   "--receipt", str(receipt), "--", sys.executable, "-c", code]
        result = subprocess.run(command, capture_output=True, text=True, timeout=20,
                                stdin=subprocess.DEVNULL, close_fds=True)
        (output / f"{name}.stdout").write_text(result.stdout)
        (output / f"{name}.stderr").write_text(result.stderr)
        if result.returncode != -signal.SIGSYS or result.stdout.strip() != marker:
            raise RuntimeError(f"{name} did not reach the socket and die with SIGSYS: {result.returncode}")
        proof = json.loads(receipt.read_text())
        if proof["action"] != "kill" or proof["result"] != "filter_loaded_before_exec":
            raise RuntimeError("positive probe does not bind the loaded kill filter")
        results[name] = {"exit_code": result.returncode, "reached_socket": True,
                         "stdout": result.stdout, "pre_exec": proof}
    record = {"schema_version": 1, "status": "passed", "probes": results,
              "wrapper_sha256": digest(__file__)}
    (output / "verification.json").write_text(json.dumps(record, indent=2) + "\n")
    return record


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--verify-kill-policy", type=Path)
    parser.add_argument("--action", choices=("errno", "kill"), default="errno")
    parser.add_argument("--receipt", type=Path)
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    if args.verify_kill_policy is not None:
        if args.receipt is not None or args.command:
            parser.error("positive controls and exec are separate invocations")
        print(json.dumps(verify_kill_policy(args.verify_kill_policy)))
        return
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    if not command or args.receipt is None or args.receipt.exists() or args.receipt.is_symlink():
        parser.error("an actual command and a new receipt path are required")
    args.receipt.parent.mkdir(parents=True, exist_ok=True)
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    record = {"schema_version": 1, "action": args.action,
              "scope": "inherited process-local seccomp; no namespace or system settings changed",
              "wrapper_sha256": digest(__file__), "exec_command": command,
              "launcher_python": sys.executable, "launcher_python_sha256": digest(sys.executable),
              "exec_sha256": digest(command[0]), "before": security_status()}
    try:
        inherited = socket_fds()
        if any(descriptor < 3 for descriptor in inherited):
            raise RuntimeError("standard stream is a socket; refusing to inherit it")
        for descriptor in inherited:
            os.close(descriptor)
        record["closed_socket_fds"] = inherited
        record["denied_syscalls"], record["unavailable_syscalls"], record["local_ipc_policy"] = install_filter(
            KILL_PROCESS if args.action == "kill" else ERRNO | errno.EPERM)
        record["after"] = security_status()
        if (record["after"].get("NoNewPrivs") != "1"
                or record["after"].get("Seccomp") != "2"
                or int(record["after"]["Seccomp_filters"]) != int(record["before"]["Seccomp_filters"]) + 1):
            raise RuntimeError("proc status does not confirm the newly loaded filter")
        if args.action == "errno":
            record["ipv4"] = errno_probe(socket.AF_INET)
            record["ipv6"] = errno_probe(socket.AF_INET6)
        record["socket_fds_before_exec"] = socket_fds()
        if record["socket_fds_before_exec"]:
            raise RuntimeError("socket descriptor remains before exec")
        record["result"] = "filter_loaded_before_exec"
    except Exception as error:
        record["result"] = "blocked_before_exec"
        record["error"] = f"{type(error).__name__}: {error}"
        with args.receipt.open("x") as handle:
            json.dump(record, handle, indent=2)
            handle.write("\n")
        raise
    with args.receipt.open("x") as handle:
        json.dump(record, handle, indent=2)
        handle.write("\n")
    if args.action == "errno":
        os.environ["P7_017_NETWORK_POLICY"] = "process_seccomp_deny_network"
        os.environ["P7_017_NETWORK_GUARD"] = str(Path(__file__).resolve())
    os.execvpe(command[0], command, os.environ)


if __name__ == "__main__":
    main()
