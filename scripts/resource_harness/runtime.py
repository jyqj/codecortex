"""No product launcher or experiment runner; callers retain protocol and cleanup policy."""
import json
import hashlib
import os
from pathlib import Path
import queue
import threading
import time

MEMORY_GUARD_BYTES = 12 * 1024**3
RPC_TIMEOUT_SECONDS = 300
READY_TIMEOUT_SECONDS = 300
EOF_TIMEOUT_SECONDS = 15


class RpcFailure(RuntimeError):
    def __init__(self, event):
        self.event = event
        super().__init__(f"{event['kind']}: {event.get('reason', '')}")


class Journal:
    """Caller-owned sink, serialized append and flush; close only after workers join."""
    def __init__(self, stream):
        self.stream = stream
        self.lock = threading.Lock()

    def emit(self, event, **fields):
        row = dict(event=event, time_ns=time.monotonic_ns(), **fields)
        with self.lock:
            self.stream.write(json.dumps(row, sort_keys=True) + '\n')
            self.stream.flush()
        return row


def _read(path, parser):
    try:
        return {"value": parser(path.read_text()), "error": None}
    except (OSError, ValueError, IndexError) as exc:
        return {"value": None, "error": f"{type(exc).__name__}: {exc}"}


def memory_snapshot(group, *, scope):
    """Explicit group supplied by caller, never silently assumed product-exclusive.

    memory.stat counters overlap (e.g. slab and its subcounters); do not sum them.
    Separate reads are non-atomic. Missing counters stay null, never zero-filled.
    """
    group = Path(group)
    start = time.monotonic_ns()
    def counters(text):
        return {k: int(v) for k, v in (line.split() for line in text.splitlines())}
    stat = _read(group / 'memory.stat', counters)
    current = _read(group / 'memory.current', lambda s: int(s.strip()))
    identity = _read(group / 'cgroup.procs', lambda s: [int(p) for p in s.split()])
    try:
        inode = group.stat().st_ino
        identity_error = None
    except OSError as exc:
        inode, identity_error = None, str(exc)
    return {
        'read_start_ns': start, 'read_end_ns': time.monotonic_ns(),
        'measurement_scope': 'cgroup', 'scope': scope,
        'group_path': str(group.resolve()), 'group_inode': inode,
        'group_identity_error': identity_error, 'members': identity,
        'memory_current_bytes': current,
        'memory_stat_counters': stat,
        'composition_bytes': {k: (stat['value'] or {}).get(k) for k in
                              ('anon', 'file', 'slab', 'slab_reclaimable', 'slab_unreclaimable')},
        'atomic': False,
    }


def process_snapshot(pid, *, role, proc_root=Path('/proc')):
    """Root process RSS only; neither cgroup usage nor a complete process tree."""
    root = Path(proc_root) / str(pid)
    def stat(text):
        fields = text[text.rfind(')') + 2:].split()
        return {'state': fields[0], 'start_time_ticks': int(fields[19]),
                'rss_bytes': int(fields[21]) * os.sysconf('SC_PAGE_SIZE')}
    return {'measurement_scope': 'process_root', 'role': role, 'pid': pid,
            'stat': _read(root / 'stat', stat),
            'cgroup_membership': _read(root / 'cgroup', str),
            'tree_rss_bytes': None, 'tree_coverage': 'not_measured'}


class Observer:
    """Phase boundaries synchronously observed, independent of periodic sampling."""
    def __init__(self, journal, group, *, scope, processes=None):
        self.journal, self.group, self.scope = journal, group, scope
        self.processes = dict(processes or {})
        self.lock = threading.Lock()
        self.phase = None
        self.phase_start_ns = None

    def _sample(self, event):
        return self.journal.emit(
            event, phase=self.phase, phase_start_ns=self.phase_start_ns,
            cgroup=memory_snapshot(self.group, scope=self.scope),
            processes=[process_snapshot(pid, role=role)
                       for role, pid in self.processes.items() if pid is not None])

    def begin_phase(self, phase):
        with self.lock:
            self.phase = phase
            self.phase_start_ns = time.monotonic_ns()
            return self._sample('phase_start')

    def sample(self):
        with self.lock:
            return self._sample('resource_sample')


class PendingRPC:
    """First delivered outcome per request wins under one lock.

    A response already delivered remains available; terminal state prevents new
    requests and wakes all unresolved waiters. Timeout is per request, not ready.
    """
    def __init__(self, journal):
        self.journal = journal
        self.lock = threading.Lock()
        self.pending = {}
        self.delivered = set()
        self.next_id = 0
        self.terminal_event = None

    def register(self, method, params, *, phase):
        with self.lock:
            if self.terminal_event:
                raise RpcFailure(self.terminal_event)
            self.next_id += 1
            request = dict(jsonrpc='2.0', id=self.next_id, method=method, params=params)
            waiter = queue.Queue(maxsize=1)
            self.pending[request['id']] = waiter
            self.journal.emit('request', phase=phase, payload=request)
            return request, waiter

    def response(self, payload, *, phase):
        with self.lock:
            self.journal.emit('response', phase=phase, payload=payload)
            waiter = self.pending.get(payload.get('id'))
            request_id = payload.get('id')
            if waiter is not None and request_id not in self.delivered:
                self.delivered.add(request_id)
                waiter.put_nowait(payload)

    def terminal(self, kind, reason='', **details):
        with self.lock:
            event = self.journal.emit('terminal', kind=kind, reason=reason, **details)
            if self.terminal_event is None:
                self.terminal_event = event
                for request_id, waiter in self.pending.items():
                    if request_id not in self.delivered:
                        self.delivered.add(request_id)
                        waiter.put_nowait(RpcFailure(event))
            return self.terminal_event

    def raise_if_terminal(self):
        """Also check before accepting ready; a received response is not gate success."""
        with self.lock:
            if self.terminal_event:
                raise RpcFailure(self.terminal_event)

    def wait(self, request, waiter, timeout=RPC_TIMEOUT_SECONDS):
        try:
            try:
                value = waiter.get(timeout=timeout)
            except queue.Empty:
                # Serialize timeout with terminal/response delivery; recheck the
                # queue under the same lock before declaring a timeout.
                with self.lock:
                    try:
                        value = waiter.get_nowait()
                    except queue.Empty:
                        event = self.journal.emit('rpc_timeout', kind='rpc_timeout',
                                                  reason='response deadline exceeded',
                                                  request_id=request['id'], timeout_seconds=timeout)
                        raise RpcFailure(event) from None
            if isinstance(value, RpcFailure):
                raise value
            if 'error' in value:
                raise RpcFailure(dict(kind='rpc_error', reason=value['error']))
            return value['result']
        finally:
            with self.lock:
                self.pending.pop(request['id'], None)
                self.delivered.discard(request['id'])


class StdioRPC:
    """Attach to an already owned Popen; no launch, kill, or cleanup escalation."""
    def __init__(self, process, journal, *, phase=lambda: 'unknown'):
        self.process, self.journal, self.phase = process, journal, phase
        self.pending = PendingRPC(journal)
        self.write_lock = threading.Lock()
        self.reader = threading.Thread(target=self._read, name='harness-rpc-reader')
        self.exit_watcher = threading.Thread(target=self._exit, name='harness-rpc-exit')
        self.reader.start()
        self.exit_watcher.start()

    def _read(self):
        try:
            for line in self.process.stdout:
                wire = line.encode()
                self.journal.emit('stdout_wire', phase=self.phase(), text=line,
                                  wire_bytes=len(wire), wire_sha256=hashlib.sha256(wire).hexdigest())
                payload = json.loads(line)
                if not isinstance(payload, dict):
                    raise ValueError('JSON-RPC payload must be an object')
                self.pending.response(payload, phase=self.phase())
        except (OSError, ValueError, TypeError) as exc:
            self.pending.terminal('reader_error', repr(exc))
        finally:
            self.pending.terminal('stdout_eof')

    def _exit(self):
        code = self.process.wait()
        # A descendant may retain stdout after root exit. Notify independently;
        # the reader still drains/logs late lines before the caller closes logs.
        self.pending.terminal('process_exit', exit_code=code)

    def rpc(self, method, params, *, timeout=RPC_TIMEOUT_SECONDS):
        request, waiter = self.pending.register(method, params, phase=self.phase())
        try:
            with self.write_lock:
                self.process.stdin.write(json.dumps(request) + '\n')
                self.process.stdin.flush()
        except (OSError, ValueError) as exc:
            self.pending.terminal('write_error', repr(exc))
        return self.pending.wait(request, waiter, timeout)

    def notify_guard(self, reason, **details):
        return self.pending.terminal('resource_guard', reason, **details)

    def join(self):
        """Call after the caller's existing EOF/exit policy completes, before log close."""
        self.reader.join()
        self.exit_watcher.join()


def guard_stop(rpc, reason, terminate, **details):
    """Publish first, then invoke the caller's existing controlled termination."""
    rpc.notify_guard(reason, **details)
    terminate()
