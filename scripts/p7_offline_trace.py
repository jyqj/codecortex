#!/usr/bin/env python3
"""Certify P7 offline product trees from complete per-PID strace evidence.

Seccomp remains the enforcement mechanism. Normal exit of the MCP parent is
not evidence about a child whose network error or SIGSYS the parent ignores.
Only this separate, complete tree verifier may report zero network attempts.
"""
import argparse
import ast
import json
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess
import sys

from p7_build_identity import file_sha256, json_bytes, source_snapshot


TRACE_EXPRESSION = "%process,%network,io_uring_setup,io_uring_enter,io_uring_register,unshare,setns"
CREATION = {"clone", "clone3", "fork", "vfork"}
# Anything selected by strace outside this explicit process set is treated as
# a network/unknown attempt, never silently whitelisted as harmless.
PROCESS = CREATION | {
    "execve", "execveat", "exit", "exit_group", "wait4", "waitid", "waitpid",
    "getpid", "getppid", "gettid", "kill", "tkill", "tgkill", "rt_sigqueueinfo",
    "rt_tgsigqueueinfo", "pidfd_open", "pidfd_getfd", "pidfd_send_signal",
    "unshare", "setns",
}
NETWORK = {
    "socket", "socketpair", "connect", "bind", "listen", "accept", "accept4",
    "send", "recv", "sendto", "recvfrom", "sendmsg", "recvmsg", "sendmmsg",
    "recvmmsg", "recvmmsg_time64", "getsockname", "getpeername", "setsockopt",
    "getsockopt", "shutdown", "socketcall", "io_uring_setup", "io_uring_enter",
    "io_uring_register",
}
TOOLS = {"status", "index", "search", "context", "node", "explore", "trace",
         "relations", "impact", "architecture", "files", "graph_query",
         "ingest_traces", "adr"}
CALL = re.compile(r"^([A-Za-z_][A-Za-z_0-9]*)\(")
RESUMED = re.compile(r"^<\.\.\. ([A-Za-z_][A-Za-z_0-9]*) resumed>(.*)$")
RESULT = re.compile(r"\)\s+=\s+(-?\d+)(?:\s|$)")
QUOTED = r'"(?:[^"\\]|\\.)*"'


def require(condition, message):
    if not condition:
        raise ValueError(message)


def result_number(call):
    match = RESULT.search(call["raw"])
    return int(match[1]) if match else None


def parse_trace(path, pid):
    calls, terminal, pending = [], None, None
    for number, raw in enumerate(path.read_text().splitlines(), 1):
        line = raw.strip()
        if not line:
            continue
        require(terminal is None, f"PID reuse or trailing data: {path}:{number}")
        if line.startswith("--- ") and line.endswith(" ---"):
            continue
        exited = re.fullmatch(r"\+\+\+ exited with (\d+) \+\+\+", line)
        killed = re.fullmatch(r"\+\+\+ killed by (SIG[A-Z0-9]+)(?: \(core dumped\))? \+\+\+", line)
        if exited or killed:
            require(pending is None, f"unfinished syscall at terminal event: {path}:{number}")
            terminal = {"exit_code": int(exited[1])} if exited else {"signal": killed[1]}
            continue
        resumed = RESUMED.match(line)
        if resumed:
            require(pending is not None and pending["name"] == resumed[1],
                    f"unmatched resumed syscall: {path}:{number}")
            pending["raw"] += resumed[2]
            pending = None
            continue
        match = CALL.match(line)
        require(match is not None and pending is None, f"unparsed trace: {path}:{number}: {line}")
        call = {"name": match[1], "line": number, "raw": line}
        calls.append(call)
        if line.endswith("<unfinished ...>"):
            call["raw"] = line[:-len("<unfinished ...>")]
            pending = call
    require(terminal is not None and pending is None, f"missing complete exit: {path}")
    return {"pid": pid, "path": str(path), "sha256": file_sha256(path),
            "calls": calls, "terminal": terminal}


def read_traces(prefix):
    traces = {}
    for path in sorted(prefix.parent.glob(prefix.name + ".*")):
        suffix = path.name[len(prefix.name) + 1:]
        require(suffix.isdigit() and int(suffix) > 0 and path.is_file() and not path.is_symlink(),
                f"unexpected trace filename: {path}")
        pid = int(suffix)
        require(pid not in traces, f"duplicate trace PID {pid}")
        traces[pid] = parse_trace(path, pid)
    require(traces, "no per-PID traces")
    parents = {}
    for pid, trace in traces.items():
        for call in trace["calls"]:
            if call["name"] in CREATION:
                child = result_number(call)
                require(child is not None, f"undecodable process creation: PID {pid}: {call['raw']}")
                if child > 0:
                    require(child in traces, f"missing descendant trace: {pid} -> {child}")
                    require(child not in parents, f"PID reused or multiple parents: {child}")
                    parents[child] = pid
            if call["name"] in {"unshare", "setns"} and result_number(call) == 0:
                raise ValueError("PID namespace changes are not certifiable by this trace format")
            require("CLONE_NEWPID" not in call["raw"], "new PID namespace makes trace identity ambiguous")
    roots = set(traces) - set(parents)
    require(len(roots) == 1, f"trace forest is incomplete or has extra roots: {sorted(roots)}")
    # A cycle cannot be excused by the presence of an unrelated valid root.
    for pid in traces:
        seen, current = set(), pid
        while current in parents:
            require(current not in seen, "cycle in traced process graph")
            seen.add(current)
            current = parents[current]
        require(current in roots, "disconnected trace graph")
    return traces


def exec_command(call):
    if call["name"] != "execve" or result_number(call) != 0:
        return None
    match = re.fullmatch(r"execve\((" + QUOTED + r"), \[(.*)\], .*\)\s+=\s+0", call["raw"])
    require(match is not None, "successful execve arguments are incomplete")
    def decode(value):
        # strace uses C quoted bytes (including octal UTF-8), not shell text.
        return ast.literal_eval("b" + value).decode("utf-8")
    arguments, rest = [], match[2]
    while rest:
        argument = re.match(QUOTED, rest)
        require(argument is not None, "truncated or undecodable execve argv")
        arguments.append(decode(argument[0]))
        rest = rest[argument.end():]
        if rest:
            require(rest.startswith(", "), "undecodable execve argument separator")
            rest = rest[2:]
    require(arguments, "empty successful execve argv")
    return decode(match[1]), arguments


def guard_root(guard, security=None, *, require_parent_policy=True):
    require(guard.get("action") == "kill" and guard.get("result") == "filter_loaded_before_exec",
            "missing product kill-filter receipt")
    before, after = guard["before"], guard["after"]
    nspid = [int(value) for value in after["NSpid"].split()]
    require(nspid and min(nspid) > 0 and str(nspid[0]) == after["Pid"], "invalid guard PID identity")
    require(before["NSpid"] == after["NSpid"] and before["Pid"] == after["Pid"],
            "guard PID changed while loading filter")
    previous_filters = int(before["Seccomp_filters"])
    require(previous_filters >= 0 and int(after["Seccomp_filters"]) == previous_filters + 1,
            "guard receipt does not prove its own filter increment")
    require(after["Seccomp"] == "2" and after["NoNewPrivs"] == "1"
            and guard["socket_fds_before_exec"] == [],
            "missing enforced product network isolation")
    if require_parent_policy:
        require(previous_filters >= 1 and before["Seccomp"] == "2" and before["NoNewPrivs"] == "1",
                "product or positive probe is missing inherited parent isolation")
    if security is not None:
        require(security["identity"] == "same_pid_namespace_and_direct_parent"
                and security["RequestedPid"] == str(nspid[-1])
                and security["ProcPid"] == after["Pid"]
                and security["NSpid"] == after["NSpid"], "product/procfs/guard identity disagreement")
    return nspid[-1]


def anonymous_stream_pair(call):
    if call["name"] != "socketpair" or result_number(call) != 0:
        return False
    match = re.fullmatch(r"socketpair\(AF_UNIX, ([A-Z_|]+), 0, \[(\d+), (\d+)\]\)\s+=\s+0",
                         call["raw"])
    if not match:
        return False
    flags = set(match[1].split("|"))
    return ("SOCK_STREAM" in flags and flags <= {"SOCK_STREAM", "SOCK_NONBLOCK", "SOCK_CLOEXEC"}
            and match[2] != match[3])


def inspect_tree(traces, root, command):
    require(root in traces, f"missing product root trace: {root}")
    matches = [index for index, call in enumerate(traces[root]["calls"])
               if call["name"] == "execve" and result_number(call) == 0
               and exec_command(call) == (command[0], command)]
    require(len(matches) == 1, f"expected exactly one successful exact product exec: PID {root}")
    pending, members, attempts, local_ipc = [(root, matches[0])], {}, [], []
    while pending:
        pid, begin = pending.pop()
        require(pid not in members, f"reused PID in product tree: {pid}")
        trace = traces[pid]
        members[pid] = {"trace_sha256": trace["sha256"], "from_line": trace["calls"][begin]["line"]
                       if trace["calls"] else 1, "terminal": trace["terminal"]}
        if trace["terminal"].get("signal") == "SIGSYS":
            attempts.append({"pid": pid, "reason": "SIGSYS in guarded product tree"})
        for call in trace["calls"][begin:]:
            if anonymous_stream_pair(call):
                local_ipc.append({"pid": pid, "line": call["line"], "raw": call["raw"],
                                  "scope": "anonymous AF_UNIX SOCK_STREAM IPC; no external address"})
            elif call["name"] not in PROCESS:
                attempts.append({"pid": pid, "syscall": call["name"], "line": call["line"],
                                 "raw": call["raw"], "known_network_syscall": call["name"] in NETWORK})
        # Include launcher children too, even if created before the product
        # exec. A pre-exec helper might outlive exec; it must not disappear
        # from the product's observed descendant set merely due to timing.
        for call in trace["calls"]:
            if call["name"] in CREATION and result_number(call) > 0:
                pending.append((result_number(call), 0))
    return {"root_pid": root, "exec_command": command, "processes_and_threads": members,
            "attempts": attempts, "local_anonymous_ipc": local_ipc,
            "root_terminal": traces[root]["terminal"]}


def verify_product_observations(observations, traces, product_digest):
    require(observations["build_receipt"]["binary_sha256"] == product_digest,
            "observations do not bind the executed product bytes")
    cases = observations["cases"]
    require(len(cases) == 2 and {case["explicitly_disabled"] for case in cases} == {False, True},
            "both original local/explicitly-disabled configurations are required")
    products, used = [], set()
    for case in cases:
        require(len(case["tools_listed"]) == len(TOOLS) and len(case["tools_executed"]) == len(TOOLS)
                and set(case["tools_listed"]) == TOOLS and set(case["tools_executed"]) == TOOLS,
                "the original fourteen tools did not complete")
        for field in ("local_source_verified", "error_contracts_verified",
                      "reopen_and_persisted_adr_verified", "semantic_cache_absent"):
            require(case[field] is True, f"original product assertion missing: {field}")
        for guard_field, security_field, exit_field in (
                ("child_network_guard", "child_security", "child_exit"),
                ("reopened_network_guard", "reopened_child_security", "reopened_exit")):
            guard = case[guard_field]
            require(guard["exec_sha256"] == product_digest
                    and guard["exec_command"][0] == observations["binary"], "product exec identity mismatch")
            require(case[exit_field] == {"exit_code": 0, "success": True}, "product did not exit normally")
            tree = inspect_tree(traces, guard_root(guard, case[security_field]), guard["exec_command"])
            require(tree["root_terminal"] == {"exit_code": 0}, "trace disagrees with product exit receipt")
            members = set(tree["processes_and_threads"])
            require(not used.intersection(members), "overlapping product trees or reused root PID")
            used.update(members)
            tree["explicitly_disabled"] = case["explicitly_disabled"]
            tree["reopened"] = guard_field.startswith("reopened")
            products.append(tree)
    positive, positive_used = [], set()
    for family in ("ipv4", "ipv6"):
        probe = observations["network_positive_controls"]["probes"][family]
        require(probe["reached_socket"] is True and probe["exit_code"] == -signal.SIGSYS,
                "guard's actual socket positive control is missing")
        tree = inspect_tree(traces, guard_root(probe["pre_exec"]), probe["pre_exec"]["exec_command"])
        members = set(tree["processes_and_threads"])
        require(not used.intersection(members), "positive probe overlaps product tree")
        require(not positive_used.intersection(members), "positive probe trees overlap or reuse a PID")
        positive_used.update(members)
        address_family = {"ipv4": "AF_INET", "ipv6": "AF_INET6"}[family]
        reached = [row for row in tree["attempts"] if row.get("syscall") == "socket"
                   and row["pid"] == tree["root_pid"]
                   and row["raw"].startswith(f"socket({address_family}, ")]
        require(tree["root_terminal"] == {"signal": "SIGSYS"}
                and len(reached) == 1,
                "raw trace did not observe the actual positive socket probe with its exact address family")
        positive.append({"family": family, "tree": tree})
    violations = [event for tree in products for event in tree["attempts"]]
    return {"status": "passed" if not violations else "failed", "product_trees": products,
            "positive_controls_outside_product_trees": positive,
            "network_socket_attempts": 0 if not violations else None,
            "violations": violations, "scope": "exact product exec through every traced descendant/thread exit"}


def run_trace(command, output, environment=None, timeout=300):
    output.mkdir(parents=True, exist_ok=False)
    executable = shutil.which("strace")
    require(executable is not None, "strace unavailable; full-tree certification is not run")
    version = subprocess.check_output([executable, "--version"], text=True).splitlines()[0]
    prefix = output / "syscalls"
    argv = [executable, "-ff", "-s", "4096", "-e", "trace=" + TRACE_EXPRESSION,
            "-o", str(prefix), "--", *command]
    record = {"schema_version": 1, "command": argv, "strace_version": version,
              "strace_sha256": file_sha256(executable), "trace_expression": TRACE_EXPRESSION,
              "tracer_namespace_pid": os.getpid(),
              "pid_namespace": os.readlink("/proc/self/ns/pid"), "timeout_seconds": timeout}
    with (output / "stdout.log").open("wb") as stdout, (output / "stderr.log").open("wb") as stderr:
        process = subprocess.Popen(argv, env=environment, stdin=subprocess.DEVNULL,
                                   stdout=stdout, stderr=stderr, start_new_session=True)
        try:
            record["returncode"] = process.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            # Only this invocation's owned process group is terminated. A
            # partial trace is preserved and never becomes a passing receipt.
            os.killpg(process.pid, signal.SIGKILL)
            process.wait()
            record["returncode"] = None
            record["error"] = "trace timeout; incomplete evidence cannot certify offline"
    record["raw_files"] = {path.name: file_sha256(path) for path in sorted(output.iterdir()) if path.is_file()}
    (output / "trace-run.json").write_bytes(json_bytes(record))
    require(record["returncode"] == 0, f"traced process failed or trace unavailable: {record.get('returncode')}")
    return read_traces(prefix), record


def real_controls(output):
    """These are real kernel traces. A fixture-only parser test is insufficient."""
    output.mkdir(parents=True, exist_ok=False)
    guard = Path(__file__).with_name("p7_offline_network.py").resolve()
    reports = []
    for attack in (False, True):
        directory = output / ("child-socket-parent-zero" if attack else "clean-child")
        receipt = output / (directory.name + ".json")
        code = ("import os,signal,socket,ctypes; child=os.fork()\n"
                "if child == 0:\n"
                + (" socket.socket(socket.AF_INET,socket.SOCK_STREAM)\n" if attack else
                   " libc=ctypes.CDLL(None,use_errno=True); fds=(ctypes.c_int*2)()\n"
                   " libc.socketpair.argtypes=[ctypes.c_int,ctypes.c_int,ctypes.c_int,ctypes.POINTER(ctypes.c_int)]\n"
                   " assert libc.socketpair(socket.AF_UNIX,socket.SOCK_STREAM,0,fds)==0\n"
                   " os.write(fds[0],b'ipc'); assert os.read(fds[1],3)==b'ipc'\n"
                   " os.close(fds[0]); os.close(fds[1])\n")
                + " os._exit(0)\n"
                "_,status=os.waitpid(child,0)\n"
                + ("assert os.WIFSIGNALED(status) and os.WTERMSIG(status)==signal.SIGSYS\n"
                   if attack else "assert os.waitstatus_to_exitcode(status)==0\n")
                + "print('owned child waited; parent exits zero',flush=True)\n")
        product = [sys.executable, "-c", code]
        traces, record = run_trace([sys.executable, str(guard), "--action", "kill", "--receipt",
                                   str(receipt), "--", *product], directory, timeout=30)
        pre_exec = json.loads(receipt.read_text())
        # These standalone controls install only their own kill filter. A
        # native host can start at zero; product matrices still require the
        # inherited runner policy in addition to their exact +1 kill filter.
        tree = inspect_tree(traces, guard_root(pre_exec, require_parent_policy=False), product)
        require(tree["root_terminal"] == {"exit_code": 0}, "negative control parent must exit zero")
        require(len(tree["processes_and_threads"]) >= 2, "real child was not observed")
        require(bool(tree["attempts"]) is attack, "trace verifier did not distinguish the child socket control")
        if attack:
            require(any(row.get("syscall") == "socket" and row["pid"] != tree["root_pid"]
                        for row in tree["attempts"]), "missing child's actual socket syscall")
        else:
            require(tree["local_anonymous_ipc"], "clean child did not exercise actual anonymous IPC")
        reports.append({"attack": attack, "parent_returncode": record["returncode"], "tree": tree})
    report = {"schema_version": 1, "status": "passed", "controls": reports,
              "verifier_sha256": file_sha256(__file__), "guard_sha256": file_sha256(guard)}
    (output / "controls.json").write_bytes(json_bytes(report))
    return report


def run_product(args):
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    root = Path(__file__).resolve().parents[1]
    before = source_snapshot(root)
    source = {key: value for key, value in before.items() if key != "inputs"}
    product, runner = args.product.resolve(strict=True), args.runner.resolve(strict=True)
    receipt = json.loads(args.build_receipt.read_text())
    runner_receipt = json.loads(args.runner_receipt.read_text())
    require(receipt["source_before"] == source == receipt["source_after"], "product source identity mismatch")
    require(runner_receipt["source_before"] == source == runner_receipt["source_after"],
            "test runner source identity mismatch")
    require(receipt["binary_sha256"] == file_sha256(product), "product binary hash mismatch")
    require(runner_receipt["sha256"] == file_sha256(runner)
            and runner_receipt["cargo_artifact"]["target"]["name"] == "benchmark_adapters"
            and runner_receipt["cargo_artifact"]["target"]["kind"] == ["test"]
            and Path(runner_receipt["cargo_artifact"]["executable"]).resolve() == runner,
            "runner binary/artifact identity mismatch")
    guard = root / "scripts/p7_offline_network.py"
    bound_inputs = {str(path): file_sha256(path) for path in (
        product, runner, guard, Path(__file__).resolve(),
        Path(__file__).with_name("p7_build_identity.py").resolve(),
        args.build_receipt.resolve(), args.runner_receipt.resolve(), Path(sys.executable).resolve())}
    environment = dict(os.environ, CODECORTEX_BENCH_BINARY=str(product),
                       P7_017_PACKAGE_KIND=args.package_kind,
                       P7_017_BUILD_RECEIPT=str(args.build_receipt.resolve()),
                       CODECORTEX_BENCH_OBSERVATIONS=str(output / "observations"))
    command = [sys.executable, str(guard), "--receipt", str(output / "parent-policy.json"), "--",
               str(runner), "p7_offline::real_stdio_default_disabled_semantic_contract",
               "--ignored", "--exact", "--nocapture"]
    traces, raw = run_trace(command, output / "trace", environment)
    observations_path = output / "observations" / f"p7-017-{args.package_kind}.json"
    observations = json.loads(observations_path.read_text())
    require(observations["build_receipt"] == receipt, "adapter consumed a different build receipt")
    require(observations["network_positive_controls"]["wrapper_sha256"] == bound_inputs[str(guard)],
            "positive controls used a different guard")
    guards = [case[field] for case in observations["cases"]
              for field in ("child_network_guard", "reopened_network_guard")]
    guards.extend(probe["pre_exec"] for probe in observations["network_positive_controls"]["probes"].values())
    for proof in guards:
        require(proof["wrapper_sha256"] == bound_inputs[str(guard)]
                and proof["launcher_python_sha256"] == bound_inputs[str(Path(sys.executable).resolve())],
                "guard/interpreter execution identity mismatch")
    report = verify_product_observations(observations, traces, file_sha256(product))
    require(source_snapshot(root) == before, "source changed during traced product matrix")
    require(all(file_sha256(path) == digest for path, digest in bound_inputs.items()),
            "binary, launcher, verifier or receipt changed during matrix")
    report.update(schema_version=1, package_kind=args.package_kind, source=source,
                  verifier_sha256=file_sha256(__file__), guard_sha256=file_sha256(guard),
                  product_sha256=file_sha256(product), runner_sha256=file_sha256(runner),
                  build_receipt_sha256=file_sha256(args.build_receipt),
                  runner_receipt_sha256=file_sha256(args.runner_receipt),
                  observations_sha256=file_sha256(observations_path), trace_run=raw,
                  bound_inputs_before_and_after=bound_inputs)
    (output / "verification.json").write_bytes(json_bytes(report))
    require(report["status"] == "passed", "network/unknown attempt found in real product tree")
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="operation", required=True)
    controls = sub.add_parser("controls")
    controls.add_argument("--output", type=Path, required=True)
    run = sub.add_parser("run")
    for name in ("product", "runner", "build-receipt", "runner-receipt", "output"):
        run.add_argument("--" + name, type=Path, required=True)
    run.add_argument("--package-kind", choices=("default", "semantic"), required=True)
    args = parser.parse_args()
    try:
        result = real_controls(args.output.resolve()) if args.operation == "controls" else run_product(args)
    except (OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
        failure = {"schema_version": 1, "status": "failed", "error": str(error),
                   "network_socket_attempts": None}
        if args.output.is_dir():
            path = args.output / "failure.json"
            if not path.exists():
                path.write_bytes(json_bytes(failure))
        print(json.dumps(failure), file=sys.stderr)
        return 1
    print(json.dumps({"status": result["status"], "output": str(args.output)}))
    return 0


if __name__ == "__main__":
    sys.exit(main())
