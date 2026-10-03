"""Owned process adapter: single stdout reader; original MCP/tool contract."""
import json
import os
from pathlib import Path
import subprocess
import threading
import time

from .runtime import Journal, StdioRPC
from .protocol import write, snapshot, tree_snapshot, free


class Product:
    def __init__(self, binary, root, out, cache, observer, model_pid):
        env = {k: v for k, v in os.environ.items()
               if not k.startswith('CODECORTEX_') and k != 'OPENAI_API_KEY'}
        env.update(CODECORTEX_SEMANTIC_CACHE_ROOT=str(cache),
                   CODECORTEX_PPID_POLL_MS='0', P7_RESOURCE_DUMMY='synthetic-resource-only')
        self.observer = observer
        observer.begin_phase('productstartup-before')
        self.stderr = (out / 'product-stderr.log').open('wb')
        try:
            self.p = subprocess.Popen([str(binary), 'mcp', '--project-path', str(root)],
                                      cwd=root, env=env, stdin=subprocess.PIPE,
                                      stdout=subprocess.PIPE, stderr=self.stderr, text=True)
        except BaseException:
            self.stderr.close()
            raise
        self.raw = (out / 'rpc.jsonl').open('w')
        self.transport = StdioRPC(self.p, Journal(self.raw), phase=lambda: self.phase)
        self.observer.processes.update(product_root=self.p.pid, model_root=model_pid)
        self.model_pid = model_pid
        self.lock = threading.Lock()
        self.local = threading.local()
        self.active = self.peak = 0
        self.closed = False
        self.stop = threading.Event()
        self.resources = (out / 'resources.jsonl').open('w')
        self.observer.begin_phase('productstartup')
        self.sampler_error = None
        def sample():
            try:
                while not self.stop.is_set():
                    self.observer.sample()
                    current = self.observer.group / 'memory.current'
                    try:
                        memory = int(current.read_text())
                    except (OSError, ValueError):
                        memory = None
                    self.resources.write(json.dumps(dict(time_ns=time.monotonic_ns(),
                        phase=self.phase, product=snapshot(self.p.pid),
                        product_tree=tree_snapshot(self.p.pid), runner=snapshot(os.getpid()),
                        model=snapshot(model_pid), tmp_free_bytes=free(out),
                        cgroup_memory_current=memory)) + '\n')
                    self.resources.flush()
                    self.stop.wait(.02)
            except BaseException as exc:
                self.sampler_error = repr(exc)
                self.transport.pending.terminal('sampler_error', repr(exc))
        self.sampler = threading.Thread(target=sample, name='product-resource-sampler')
        self.sampler.start()

    @property
    def phase(self):
        return self.observer.phase

    @phase.setter
    def phase(self, value):
        self.observer.begin_phase(value)

    @property
    def last_timing(self):
        return self.local.timing

    def initialize(self, out):
        self.rpc('initialize', dict(protocolVersion='2024-11-05', capabilities={},
                 clientInfo=dict(name='resource-preparation', version='1')))
        with self.transport.write_lock:
            self.transport.pending.raise_if_terminal()
            notification = dict(jsonrpc='2.0', method='notifications/initialized')
            self.transport.journal.emit('notification', phase=self.phase, payload=notification)
            self.p.stdin.write(json.dumps(notification) + '\n')
            self.p.stdin.flush()
        tools = self.rpc('tools/list', {})['tools']
        assert len(tools) == 14
        write(out / 'tools.json', tools)

    def rpc(self, method, params, offered=None, timeout=300):
        with self.lock:
            self.active += 1
            self.peak = max(self.peak, self.active)
        try:
            return self.transport.rpc(method, params, offered=offered, timeout=timeout,
                                      on_timing=lambda t: setattr(self.local, 'timing', t))
        finally:
            with self.lock:
                self.active -= 1

    def tool(self, name, args, **kw):
        value = self.rpc('tools/call', dict(name=name, arguments=args), **kw)
        if value.get('isError'):
            raise RuntimeError(value)
        data = value['structuredContent']
        return data.get('result', data)

    def close_logs(self):
        if self.closed:
            return
        self.stop.set()
        self.sampler.join()
        # Root exit can precede pipe EOF. Do not block past the original EOF bound.
        self.transport.reader.join(timeout=15)
        self.transport.exit_watcher.join(timeout=15)
        if self.transport.reader.is_alive() or self.transport.exit_watcher.is_alive():
            self.transport.journal.emit('cleanup_incomplete', reason='owned pipe/root still open')
            raise RuntimeError('cleanup incomplete; retained pipe/logs; no descendant kill escalation')
        self.transport.join()
        self.raw.close()
        self.resources.close()
        self.stderr.close()
        self.p.stdout.close()
        self.closed = True
        self.observer.processes.pop('product_root', None)
        if self.sampler_error:
            raise RuntimeError(self.sampler_error)
