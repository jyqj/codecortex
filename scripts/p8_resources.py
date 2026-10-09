#!/usr/bin/env python3
"""Bounded observations of an owned subprocess, including nested PID namespaces.

Only the selected owner's kernel child lists are traversed. Numeric namespace
PIDs are never interpreted as host procfs PIDs. Failed identity/IO/topology
observations remain unavailable. This does not measure an unsampled peak.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import resource
import sys
import time


MAX_PROCESSES = 128
MAX_TASKS = 2048
MAX_PROC_BYTES = 32768


def _text(directory, name):
    flags = os.O_RDONLY | os.O_CLOEXEC
    fd = os.open(name, flags, dir_fd=directory)
    try:
        data = os.read(fd, MAX_PROC_BYTES + 1)
        if len(data) > MAX_PROC_BYTES:
            raise ValueError("proc observation exceeds its byte bound")
        return data.decode("utf-8", errors="strict")
    finally:
        os.close(fd)


def _status(text):
    fields = {}
    for line in text.splitlines():
        key, separator, value = line.partition(":")
        if separator:
            if key in fields:
                raise ValueError("duplicate proc status field")
            fields[key] = value.strip()
    pids = [int(value) for value in fields["NSpid"].split()]
    result = {"host_pid": int(fields["Pid"]), "host_ppid": int(fields["PPid"]),
              "namespace_pids": pids, "tgid": int(fields["Tgid"])}
    if not pids or any(pid <= 0 for pid in pids) or pids[0] != result["host_pid"]:
        raise ValueError("inconsistent kernel namespace identity")
    return result


def _stat(text):
    identity, suffix = text.rsplit(") ", 1)
    pid, _comm = identity.split(" (", 1)
    fields = suffix.split()
    if len(fields) < 22 or fields[0] in ("Z", "X", "x"):
        raise ValueError("process is gone or has incomplete accounting")
    result = {"host_pid": int(pid), "host_ppid": int(fields[1]),
              "user_ticks": int(fields[11]), "system_ticks": int(fields[12]),
              "threads": int(fields[17]), "start_ticks": int(fields[19]),
              "resident_pages": int(fields[21])}
    if any(value < 0 for value in result.values()) or result["threads"] == 0:
        raise ValueError("invalid kernel process counters")
    return result


def _key(identity):
    return tuple(identity[name] for name in
                 ("host_pid", "host_ppid", "start_ticks", "pid_namespace_inode",
                  "executable_device", "executable_inode"))


def _identity(directory):
    status = _status(_text(directory, "status"))
    counters = _stat(_text(directory, "stat"))
    if (status["host_pid"] != counters["host_pid"]
            or status["host_ppid"] != counters["host_ppid"]
            or status["tgid"] != status["host_pid"]):
        raise ValueError("proc status/stat identity mismatch")
    exe = os.stat("exe", dir_fd=directory)
    namespace = os.stat("ns/pid", dir_fd=directory)
    return {**status, "start_ticks": counters["start_ticks"],
            "pid_namespace_inode": namespace.st_ino,
            "executable_device": exe.st_dev, "executable_inode": exe.st_ino,
            "executable": os.readlink("exe", dir_fd=directory)}, counters


def _children(directory):
    task_fd = os.open("task", os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC,
                      dir_fd=directory)
    try:
        tasks = os.listdir(task_fd)
        if len(tasks) > MAX_TASKS or any(not name.isdecimal() for name in tasks):
            raise ValueError("owner thread inventory exceeds its bound")
        children = set()
        for task in tasks:
            children.update(int(pid) for pid in _text(task_fd, f"{task}/children").split())
        if len(children) > MAX_PROCESSES or any(pid <= 0 for pid in children):
            raise ValueError("child inventory exceeds its bound")
        return sorted(children)
    finally:
        os.close(task_fd)


def _directory(pid):
    return os.open(f"/proc/{pid}", os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC)


def _io(directory):
    fields = {}
    for line in _text(directory, "io").splitlines():
        key, separator, value = line.partition(":")
        if not separator or key in fields:
            raise ValueError("invalid proc IO record")
        value = int(value.strip())
        if value < 0:
            raise ValueError("negative proc IO counter")
        fields[key] = value
    needed = ("rchar", "wchar", "read_bytes", "write_bytes", "cancelled_write_bytes",
              "syscr", "syscw")
    if any(key not in fields for key in needed):
        raise ValueError("incomplete proc IO accounting")
    return {key: fields[key] for key in needed}


def _owned_owner(owner):
    with open("/proc/self/status", encoding="utf-8") as stream:
        current = _status(stream.read(MAX_PROC_BYTES))
    if current["namespace_pids"][-1] != os.getpid():
        raise ValueError("current process namespace identity cannot be established")
    host = current["host_pid"] if owner == "self" else current["host_ppid"]
    descriptor = _directory(host)
    try:
        identity, _ = _identity(descriptor)
        expected_pid = os.getpid() if owner == "self" else os.getppid()
        if (identity["namespace_pids"][-1] != expected_pid
                or identity["pid_namespace_inode"] != os.stat("/proc/self/ns/pid").st_ino):
            raise ValueError("selected owner is not this process or its direct parent")
        return descriptor, identity
    except BaseException:
        os.close(descriptor)
        raise


class OwnedProcessProbe:
    """Probe one child of the caller (or of this helper's direct parent).

    Keep the object across observations to reject a reused PID or exec. Binary
    is optional for callers with another immutable identity witness; when
    supplied, the kernel executable device/inode must identify that exact file.
    """
    def __init__(self, pid, binary=None, owner="self"):
        if not isinstance(pid, int) or isinstance(pid, bool) or pid <= 0:
            raise ValueError("owned child PID must be a positive integer")
        if owner not in ("self", "parent"):
            raise ValueError("owner must be self or parent")
        self.pid, self.owner, self.bound_identity = pid, owner, None
        self.binary = str(Path(binary).resolve(strict=True)) if binary else None
        self.expected_executable = None
        if self.binary:
            metadata = os.stat(self.binary)
            self.expected_executable = (metadata.st_dev, metadata.st_ino)

    def snapshot(self):
        started = time.monotonic_ns()
        result = {"schema_version": 1, "namespace_pid": self.pid,
                  "status": "unavailable", "identity": None, "process": None,
                  "tree": {"complete": False, "members": [], "resident_bytes": None,
                           "user_cpu_ns": None, "system_cpu_ns": None,
                           "read_bytes": None, "write_bytes": None},
                  "method": "owned parent task children; namespace/PPid/exe/start identity; proc directory FD; native counters",
                  "measurement": "point samples; CPU and IO lifetime cumulative; RSS current kernel estimate; transient peaks may be missed",
                  "started_monotonic_ns": started}
        owner_fd = None
        try:
            if not sys.platform.startswith("linux"):
                raise ValueError("owned procfs mapping is available on Linux only")
            owner_fd, owner = _owned_owner(self.owner)
            result["owner_identity"] = owner
            candidates = []
            for host_pid in _children(owner_fd):
                child_fd = None
                try:
                    child_fd = _directory(host_pid)
                    identity, _ = _identity(child_fd)
                    if (identity["host_ppid"] == owner["host_pid"]
                            and identity["namespace_pids"][-1] == self.pid
                            and identity["pid_namespace_inode"] == owner["pid_namespace_inode"]):
                        candidates.append(identity)
                except (OSError, ValueError, KeyError):
                    continue  # A different owned child may exit during the walk.
                finally:
                    if child_fd is not None:
                        os.close(child_fd)
            if len(candidates) != 1:
                raise ValueError("owned namespace PID has no unique live child mapping")
            identity = candidates[0]
            if (self.expected_executable is not None and self.expected_executable !=
                    (identity["executable_device"], identity["executable_inode"])):
                raise ValueError("owned child executable differs from the bound binary")
            if self.bound_identity is not None and _key(self.bound_identity) != _key(identity):
                raise ValueError("owned child identity changed (PID reuse or exec)")
            self.bound_identity = identity
            page_size, ticks = os.sysconf("SC_PAGE_SIZE"), os.sysconf("SC_CLK_TCK")
            if page_size <= 0 or ticks <= 0:
                raise ValueError("kernel accounting units are unavailable")
            visited, queue, members, reasons = set(), [(identity, True)], [], []
            while queue:
                expected, is_root = queue.pop(0)
                host = expected["host_pid"]
                if host in visited or len(visited) >= MAX_PROCESSES:
                    reasons.append("owned descendant cycle or inventory bound")
                    break
                visited.add(host)
                child_fd = None
                try:
                    child_fd = _directory(host)
                    before, counters = _identity(child_fd)
                    if _key(before) != _key(expected):
                        raise ValueError("owned child changed before sample")
                    first_children = _children(child_fd)
                    io_error = None
                    try:
                        io = _io(child_fd)
                    except (OSError, ValueError, KeyError) as error:
                        io, io_error = None, f"{type(error).__name__}: {error}"
                    after, _ = _identity(child_fd)
                    last_children = _children(child_fd)
                    if _key(before) != _key(after):
                        raise ValueError("owned child changed during sample")
                    if first_children != last_children:
                        reasons.append("descendant topology changed during sample")
                    process = {"identity": before,
                               "resident_bytes": counters["resident_pages"] * page_size,
                               "user_cpu_ns": counters["user_ticks"] * 1_000_000_000 // ticks,
                               "system_cpu_ns": counters["system_ticks"] * 1_000_000_000 // ticks,
                               "threads": counters["threads"], "page_size": page_size,
                               "clock_ticks_per_second": ticks, "io": io,
                               "io_unavailable_reason": io_error}
                    members.append(process)
                    if is_root:
                        result["identity"], result["process"] = before, process
                    for descendant in last_children:
                        descendant_fd = None
                        try:
                            descendant_fd = _directory(descendant)
                            nested, _ = _identity(descendant_fd)
                            if (nested["host_ppid"] != host or nested["pid_namespace_inode"]
                                    != owner["pid_namespace_inode"]):
                                raise ValueError("descendant parent/namespace mismatch")
                            queue.append((nested, False))
                        except (OSError, ValueError, KeyError) as error:
                            reasons.append(f"descendant unavailable: {type(error).__name__}: {error}")
                        finally:
                            if descendant_fd is not None:
                                os.close(descendant_fd)
                except (OSError, ValueError, KeyError) as error:
                    reasons.append(f"owned sample unavailable: {type(error).__name__}: {error}")
                finally:
                    if child_fd is not None:
                        os.close(child_fd)
            complete = not reasons and bool(members) and result["process"] is not None
            result["tree"] = {"complete": complete, "members": members,
                              "unavailable_reasons": reasons}
            for metric in ("resident_bytes", "user_cpu_ns", "system_cpu_ns"):
                result["tree"][metric] = sum(row[metric] for row in members) if complete else None
            for metric in ("read_bytes", "write_bytes", "rchar", "wchar"):
                result["tree"][metric] = (sum(row["io"][metric] for row in members)
                                           if complete and all(row["io"] is not None for row in members)
                                           else None)
            result["status"] = ("observed" if complete and all(row["io"] is not None for row in members)
                                else "partial" if result["process"] is not None else "unavailable")
        except (OSError, ValueError, KeyError, OverflowError) as error:
            result["unavailable_reason"] = f"{type(error).__name__}: {error}"
        finally:
            if owner_fd is not None:
                os.close(owner_fd)
        result["finished_monotonic_ns"] = time.monotonic_ns()
        result["observation_duration_ns"] = result["finished_monotonic_ns"] - started
        return result


def self_usage():
    """Kernel SELF accounting; never label lifetime ru_maxrss as current RSS."""
    usage = resource.getrusage(resource.RUSAGE_SELF)
    multiplier = 1024 if sys.platform.startswith("linux") else 1 if sys.platform == "darwin" else None
    native, native_error = None, None
    descriptor = None
    try:
        if not sys.platform.startswith("linux"):
            raise ValueError("Python SELF current RSS/IO observer is Linux only")
        descriptor = os.open("/proc/self", os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC)
        identity, counters = _identity(descriptor)
        if identity["namespace_pids"][-1] != os.getpid():
            raise ValueError("SELF namespace identity mismatch")
        native = {"resident_bytes": counters["resident_pages"] * os.sysconf("SC_PAGE_SIZE"),
                  "io": _io(descriptor), "identity": identity}
    except (OSError, ValueError, KeyError) as error:
        native_error = f"{type(error).__name__}: {error}"
    finally:
        if descriptor is not None:
            os.close(descriptor)
    return {"namespace_pid": os.getpid(), "method": "getrusage(RUSAGE_SELF); /proc/self native current RSS and IO separately",
            "user_cpu_seconds": usage.ru_utime, "system_cpu_seconds": usage.ru_stime,
            "lifetime_peak_resident_bytes": usage.ru_maxrss * multiplier if multiplier else None,
            "native": native, "native_unavailable_reason": native_error,
            "block_inputs": usage.ru_inblock, "block_outputs": usage.ru_oublock,
            "block_counts_are_not_bytes": True}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pid", required=True, type=int)
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--owner", choices=("self", "parent"), default="self")
    args = parser.parse_args()
    result = OwnedProcessProbe(args.pid, args.binary, args.owner).snapshot()
    result["sampler_sha256"] = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    print(json.dumps(result, sort_keys=True))
    return 0  # Unavailability is explicit data, not a fabricated zero.


if __name__ == "__main__":
    sys.exit(main())
