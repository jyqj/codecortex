"""Exact binary stdio against synthetic local source and loopback HTTP only."""
import contextlib
import hashlib
import http.server
import json
import os
import pathlib
import queue
import subprocess
import sys
import tempfile
import threading
import time


binary = pathlib.Path(sys.argv[1]).resolve()
output = pathlib.Path(sys.argv[2]).resolve()
gate = threading.Event()
entered = threading.Event()
gate.set()
counts = {"requests": 0, "status": 200}


class Mock(http.server.BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def do_POST(self):
        assert self.path == "/v1/embeddings"
        request = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        counts["requests"] += 1
        entered.set()
        gate.wait(10)
        status = counts["status"]
        body = json.dumps({"model": request["model"], "data": [{"index": n, "embedding": [1.0, 0.0]} for n in range(len(request["input"]))]}).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Mock)
threading.Thread(target=server.serve_forever, daemon=True).start()
endpoint = f"http://127.0.0.1:{server.server_port}/v1"
rows = []
child = None


class Child:
    def __init__(self, config, root):
        self.root = root
        config["auto_index"] = {"enabled": False, "idle_timeout_secs": 1}
        (root / ".codecortex.json").write_text(json.dumps(config))
        (root / "one.rs").write_text("pub fn local_fixture() -> u32 { 7 }\n")
        self.cache = root / "cache"
        env = dict(os.environ, CODECORTEX_SEMANTIC_CACHE_ROOT=str(self.cache))
        # In-process loopback fixture credential; never emitted into evidence.
        env["CODECORTEX_P7_LOOPBACK_KEY"] = "local-fixture-only"
        env.pop("CODECORTEX_P7_ABSENT_KEY", None)
        self.stderr = root / "stderr.log"
        self.log = self.stderr.open("w")
        self.process = subprocess.Popen([str(binary), "mcp", "--project-path", str(root)], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=self.log, text=True, env=env)
        self.responses = queue.Queue()
        threading.Thread(target=lambda: [self.responses.put(line) for line in self.process.stdout], daemon=True).start()
        self.n = 0
        self.rpc("initialize", {"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "p7-local-lifecycle", "version": "1"}})
        self.process.stdin.write(json.dumps({"jsonrpc": "2.0", "method": "notifications/initialized"}) + "\n")
        self.process.stdin.flush()
        assert len(self.rpc("tools/list", {})["tools"]) == 14

    def rpc(self, method, params):
        self.n += 1
        self.process.stdin.write(json.dumps({"jsonrpc": "2.0", "id": self.n, "method": method, "params": params}) + "\n")
        self.process.stdin.flush()
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            reply = json.loads(self.responses.get(timeout=max(0.1, deadline - time.monotonic())))
            if reply.get("id") == self.n:
                assert "error" not in reply, reply
                return reply["result"]
        raise TimeoutError(method)

    def tool(self, name, arguments):
        reply = self.rpc("tools/call", {"name": name, "arguments": arguments})
        assert not reply.get("isError"), reply
        value = reply.get("structuredContent")
        if value is None:
            value = json.loads(next(block["text"] for block in reply["content"] if block["type"] == "text"))
        return value.get("result", value)

    def status(self):
        return self.tool("status", {"aspect": "capabilities"})["retrieval"]

    def index(self):
        return self.tool("index", {"path": str(self.root)})

    def wait_state(self, expected, timeout=10):
        deadline = time.monotonic() + timeout
        last = None
        while time.monotonic() < deadline:
            last = self.status()
            if last["semantic_state"] == expected:
                return last
            time.sleep(0.03)
        raise AssertionError((expected, last))

    def close(self):
        self.process.stdin.close()
        try:
            self.process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            self.process.terminate()
            self.process.wait(timeout=10)
        self.log.close()
        log = self.stderr.read_bytes()
        assert b"Cannot drop a runtime" not in log
        return {"exit_code": self.process.returncode, "stderr_sha256": hashlib.sha256(log).hexdigest(), "stderr_bytes": len(log), "idle_closed": b"idle eviction: closed" in log}


def semantic(**overrides):
    config = {"enabled": True, "model_id": "local/stdio-fixture", "dimensions": 2, "max_input_tokens": 8192, "max_batch_items": 16, "endpoint": endpoint, "allow_http": True, "network_opt_in": True, "api_key_ref": "env:CODECORTEX_P7_LOOPBACK_KEY", "breaker_failure_threshold": 100}
    config.update(overrides)
    return {"semantic": config}


try:
    with contextlib.nullcontext(tempfile.mkdtemp(prefix="p7-stdio-")) as tmp:
        base = pathlib.Path(tmp)
        for name, config, expected in [("unconfigured", {}, "not_configured"), ("disabled", semantic(enabled=False), "not_configured"), ("network_disabled", semantic(network_opt_in=False), "port_attached_unverified")]:
            root = base / name
            root.mkdir()
            child = Child(config, root)
            before = counts["requests"]
            assert child.status()["semantic_state"] == expected
            child.index()
            state = child.status()
            assert state["semantic_state"] == expected
            assert state["dense_state"] == "disabled"
            assert counts["requests"] == before
            assert not child.cache.exists()
            rows.append({"case": name, "semantic_state": expected, "requests": 0, "cache_created": False, "process": child.close()})
        root = base / "assembly_failed"
        root.mkdir()
        child = Child(semantic(api_key_ref="env:CODECORTEX_P7_ABSENT_KEY"), root)
        before = counts["requests"]
        child.index()
        failed = child.wait_state("failed")
        assert failed["semantic_worker_reason"] == "semantic_provider_assembly_failed"
        assert counts["requests"] == before
        rows.append({"case": "assembly_failed", "reason": failed["semantic_worker_reason"], "requests": 0, "process": child.close()})
        root = base / "wired"
        root.mkdir()
        gate.clear()
        entered.clear()
        child = Child(semantic(), root)
        before = counts["requests"]
        assert child.status()["semantic_state"] == "port_attached_unverified"
        child.index()
        assert entered.wait(5)
        started = time.monotonic()
        pending = child.status()
        status_ms = (time.monotonic() - started) * 1000
        assert pending["semantic_state"] == "backfilling"
        assert pending["semantic_pending"] > 0
        assert pending["query_pins"] == 1
        gate.set()
        ready = child.wait_state("ready")
        assert ready["dense_state"] == "ready"
        assert ready["dense_published"] == ready["dense_desired"] > 0
        requests = counts["requests"] - before
        time.sleep(31)
        reopened = child.status()
        assert reopened["semantic_state"] == "ready"
        child.index()
        child.wait_state("ready")
        time.sleep(0.2)
        assert counts["requests"] - before == requests
        process = child.close()
        assert process["idle_closed"], "stdio reopen claim requires actual idle close evidence"
        rows.append({"case": "wired_backfilling_ready_close_reopen", "status_while_http_wait_ms": status_ms, "pending": pending["semantic_pending"], "published": ready["dense_published"], "requests": requests, "reopen_extra_requests": 0, "process": process})
        root = base / "provider_failed"
        root.mkdir()
        counts["status"] = 500
        child = Child(semantic(), root)
        before = counts["requests"]
        for attempt in range(3):
            child.index()
            deadline = time.monotonic() + 10
            while counts["requests"] - before < attempt + 1 and time.monotonic() < deadline:
                time.sleep(0.03)
            assert counts["requests"] - before == attempt + 1
            if attempt < 2:
                time.sleep(31)
        failed = child.wait_state("failed")
        assert failed["semantic_failed"] > 0
        assert failed["dense_published"] == 0
        rows.append({"case": "provider_failed", "requests": counts["requests"] - before, "failed_tasks": failed["semantic_failed"], "published": 0, "process": child.close()})
    receipt = {"scope": "L3 exact-binary stdio lifecycle with synthetic local source and loopback HTTP; no live provider/quality claim", "binary": str(binary), "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(), "cases": rows, "passed": len(rows), "failed": 0}
    output.write_text(json.dumps(receipt, indent=2) + "\n")
    print(json.dumps(receipt))
finally:
    gate.set()
    if child is not None and child.process.poll() is None:
        child.close()
    server.shutdown()
