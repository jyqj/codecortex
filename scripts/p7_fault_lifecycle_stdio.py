#!/usr/bin/env python3
"""Replay P7-016 representative faults against a receipt-bound real product.

Only generated Rust fixtures and an owned loopback embedding endpoint are used.
No database, cache metadata, lease clock, or product timeout is modified. The
original independent lifecycle_stdio.py is deliberately left unchanged.
"""
import argparse
from contextlib import closing
import hashlib
import http.server
import json
import os
from pathlib import Path
import queue
import signal
import sqlite3
import subprocess
import threading
import time
import traceback


TOOLS = {"status", "index", "search", "context", "node", "explore", "trace",
         "relations", "impact", "architecture", "files", "graph_query",
         "ingest_traces", "adr"}
SEEDS = (197, 199, 211)


def sha256(path):
    digest = hashlib.sha256()
    with Path(path).open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def require(condition, message, evidence=None):
    if not condition:
        raise AssertionError(f"{message}: {evidence!r}")


def write_json(path, value):
    with Path(path).open("x") as output:
        json.dump(value, output, indent=2)
        output.write("\n")


class Evidence:
    def __init__(self, output):
        self.output = output
        self.lock = threading.Lock()
        self.sequence = 0

    def event(self, kind, **data):
        with self.lock:
            row = {"sequence": self.sequence, "kind": kind,
                   "unix_time": time.time(), "monotonic": time.monotonic(), **data}
            self.sequence += 1
            with (self.output / "events.jsonl").open("a") as stream:
                stream.write(json.dumps(row) + "\n")
        return row

    def progress(self, seed, phase, **data):
        row = self.event("phase", seed=seed, phase=phase, **data)
        print(json.dumps(row), flush=True)


class Provider:
    def __init__(self, seed, evidence):
        self.seed = seed
        self.evidence = evidence
        self.lock = threading.Lock()
        self.mode = "success"
        self.phase = "initial"
        self.rows = []
        self.entered = threading.Event()
        self.release = threading.Event()
        self.release.set()
        owner = self

        class Handler(http.server.BaseHTTPRequestHandler):
            def log_message(self, *_args):
                pass

            def do_POST(self):
                try:
                    owner.respond(self)
                except BaseException as error:
                    owner.evidence.event("provider_fixture_error", seed=seed,
                                         error=repr(error), traceback=traceback.format_exc())

        self.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.server.daemon_threads = True
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()

    @property
    def endpoint(self):
        return f"http://127.0.0.1:{self.server.server_port}/v1"

    def set_mode(self, mode, phase):
        with self.lock:
            self.mode, self.phase = mode, phase
            self.entered.clear()
            if mode == "hold":
                self.release.clear()
            else:
                self.release.set()

    def respond(self, handler):
        require(handler.client_address[0] == "127.0.0.1", "only owned loopback")
        require(handler.path == "/v1/embeddings", "exact embedding endpoint")
        require(handler.headers.get("Authorization") == "Bearer synthetic-p7-faults",
                "synthetic credential only")
        body = json.loads(handler.rfile.read(int(handler.headers["Content-Length"])))
        require(body["model"] == f"fake/p7-faults/{self.seed}", "fixed fake model")
        inputs = body["input"]
        require(inputs and all(isinstance(value, str) and
                (f"survivor_{self.seed}" in value or f"removed_{self.seed}" in value)
                for value in inputs), "only generated document inputs", inputs)
        with self.lock:
            mode, phase = self.mode, self.phase
            if mode == "fail_once":
                self.mode = "success"
            row = {"request_number": len(self.rows) + 1, "seed": self.seed,
                   "phase": phase, "mode": mode, "request": body,
                   "received_at": time.time(), "document_inputs": len(inputs),
                   "response_status": 500 if mode == "fail_once" else 200,
                   "response_write_completed": False,
                   "product_response_consumption": "unknown",
                   "actual_paid_cost": None}
            self.rows.append(row)
        self.evidence.event("http_request", **row)
        self.entered.set()
        require(self.release.wait(20), "owned provider hold exceeded 20 seconds")
        if row["response_status"] == 500:
            response = {"error": {"message": "synthetic fixed-seed retry failure",
                                   "type": "server_error"}}
        else:
            response = {"model": body["model"], "data": [
                {"index": index, "embedding": [1.0, 0.0]}
                for index in range(len(inputs))],
                "usage": {"prompt_tokens": len(inputs), "total_tokens": len(inputs)}}
        payload = json.dumps(response).encode()
        try:
            handler.send_response(row["response_status"])
            handler.send_header("Content-Type", "application/json")
            handler.send_header("Content-Length", str(len(payload)))
            handler.end_headers()
            handler.wfile.write(payload)
            handler.wfile.flush()
            row["response_write_completed"] = True
        except (BrokenPipeError, ConnectionResetError) as error:
            row["disconnect"] = type(error).__name__
        row["response_finished_at"] = time.time()
        row["response"] = response
        self.evidence.event("http_response", **row)

    def snapshot(self):
        with self.lock:
            return json.loads(json.dumps(self.rows))

    def count(self):
        with self.lock:
            return len(self.rows)

    def close(self):
        self.release.set()
        self.server.shutdown()
        self.server.server_close()


class Product:
    def __init__(self, binary, root, cache, home, label, evidence):
        self.root, self.label, self.evidence = root, label, evidence
        self.log_path = evidence.output / f"{label}.stderr.log"
        self.log = self.log_path.open("xb")
        environment = {"PATH": os.environ["PATH"], "HOME": str(home),
                       "XDG_CONFIG_HOME": str(home / "config"),
                       "XDG_CACHE_HOME": str(home / "cache"),
                       "CODECORTEX_SEMANTIC_CACHE_ROOT": str(cache),
                       "CODECORTEX_PPID_POLL_MS": "0", "RUST_LOG": "info",
                       "P7_FAULTS_DUMMY": "synthetic-p7-faults"}
        command = [str(binary), "mcp", "--project-path", str(root)]
        self.process = subprocess.Popen(command, cwd=root, env=environment,
                                        stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                        stderr=self.log, text=True)
        self.responses = queue.Queue()
        self.reader_errors = []
        self.stdout_path = evidence.output / f"{label}.stdout.log"
        self.stdout_log = self.stdout_path.open("xb")
        self.next_id = 0
        self.exit_receipt = None
        evidence.event("spawn", session=label, command=command, pid=self.process.pid,
                       environment=environment)

        def read():
            try:
                for line in self.process.stdout.buffer:
                    # Preserve the exact bytes before JSON decoding, including
                    # malformed UTF-8/JSON and bytes following the last reply.
                    self.stdout_log.write(line)
                    self.stdout_log.flush()
                    try:
                        value = json.loads(line)
                        if not isinstance(value, dict):
                            raise ValueError("MCP stdout must contain JSON objects")
                        self.responses.put(value)
                    except (ValueError, UnicodeError) as error:
                        self.reader_errors.append(repr(error))
                        self.responses.put({"reader_error": repr(error)})
            except BaseException as error:
                self.reader_errors.append(repr(error))
                self.responses.put({"reader_error": repr(error)})
        self.reader = threading.Thread(target=read, name=f"{label}-stdout", daemon=True)
        self.reader.start()
        self.rpc("initialize", {"protocolVersion": "2024-11-05", "capabilities": {},
                 "clientInfo": {"name": "p7-fault-lifecycle", "version": "1"}})
        self.process.stdin.write(json.dumps({"jsonrpc": "2.0",
                                 "method": "notifications/initialized"}) + "\n")
        self.process.stdin.flush()
        listed = self.rpc("tools/list", {})
        require({tool["name"] for tool in listed["tools"]} == TOOLS,
                "original 14-tool surface", listed)

    def rpc(self, method, params):
        self.next_id += 1
        request = {"jsonrpc": "2.0", "id": self.next_id,
                   "method": method, "params": params}
        self.evidence.event("mcp_request", session=self.label, request=request)
        self.process.stdin.write(json.dumps(request) + "\n")
        self.process.stdin.flush()
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            response = self.responses.get(timeout=max(.01, deadline - time.monotonic()))
            self.evidence.event("mcp_response", session=self.label, response=response)
            require("reader_error" not in response, "MCP stdout must be JSON", response)
            if "id" in response:
                require(response["id"] == self.next_id,
                        "MCP response must match the only pending request", response)
            else:
                require(isinstance(response.get("method"), str)
                        and "result" not in response and "error" not in response,
                        "MCP notification must be a method without a response payload", response)
            if response.get("id") == self.next_id:
                require("error" not in response, "MCP request failed", response)
                return response["result"]
        raise TimeoutError(method)

    def tool(self, name, arguments):
        result = self.rpc("tools/call", {"name": name, "arguments": arguments})
        require(not result.get("isError"), "tool error", result)
        value = result["structuredContent"]
        return value.get("result", value)

    def index(self, full=False):
        result = self.tool("index", {"path": str(self.root), "full": full})
        require(not result.get("parse_errors"), "generated fixture parse errors", result)
        return result

    def status(self):
        return self.tool("status", {"aspect": "capabilities"})["retrieval"]

    def wait_state(self, state, published, timeout=15):
        deadline = time.monotonic() + timeout
        observed = None
        while time.monotonic() < deadline:
            observed = self.status()
            if (observed["semantic_state"] == state and observed["query_pins"] == 0
                    and observed.get("dense_published") == published):
                return observed
            time.sleep(.05)
        raise AssertionError(f"state {state}/{published} did not settle: {observed!r}")

    def stable_status(self):
        before = self.status()
        for _ in range(5):
            require(self.status()["generation"] == before["generation"],
                    "status must not advance generation")
        return before

    def finish(self, kill=False, cleanup=False):
        if self.exit_receipt is not None:
            return self.exit_receipt
        if self.process.poll() is None:
            if kill:
                self.process.kill()
            else:
                self.process.stdin.close()
        try:
            code = self.process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            if not cleanup:
                raise
            self.process.kill()
            code = self.process.wait(timeout=5)
        self.reader.join(timeout=5)
        reader_joined = not self.reader.is_alive()
        tail, errors = [], list(self.reader_errors)
        if reader_joined:
            while True:
                try:
                    response = self.responses.get_nowait()
                except queue.Empty:
                    break
                tail.append(response)
                self.evidence.event("mcp_stdout_tail", session=self.label, response=response)
                if ("reader_error" in response or "id" in response
                        or not isinstance(response.get("method"), str)
                        or "result" in response or "error" in response):
                    errors.append("unexpected terminal stdout: " + repr(response))
            self.stdout_log.close()
            self.process.stdout.close()
        self.log.close()
        self.exit_receipt = {"session": self.label, "exit_code": code,
                             "requested_sigkill": kill, "cleanup": cleanup,
                             "stderr_sha256": sha256(self.log_path),
                             "stdout_reader_joined": reader_joined,
                             "stdout_sha256": sha256(self.stdout_path) if reader_joined else None,
                             "stdout_errors": errors, "terminal_notifications": len(tail) if not errors else None,
                             "stdout_policy": "declared SIGKILL may interrupt stdout" if kill else "complete JSON and no unmatched response"}
        self.evidence.event("product_exit", **self.exit_receipt)
        if not cleanup:
            require(code == (-signal.SIGKILL if kill else 0),
                    "product exit must match the declared boundary", self.exit_receipt)
            require(reader_joined, "product stdout reader must terminate at EOF", self.exit_receipt)
            if not kill:
                require(not errors, "product stdout tail must be valid and fully drained", self.exit_receipt)
        return self.exit_receipt


def database_snapshot(root, evidence, seed, phase):
    path = root / ".codecortex" / "index.sqlite3"
    # Read-only observer: never make desired rows, clear retry times, change
    # attempts, or advance the lease clock to make a scenario pass.
    with closing(sqlite3.connect(path.as_uri() + "?mode=ro", uri=True, timeout=5)) as database:
        database.row_factory = sqlite3.Row
        result = {"outbox": [dict(row) for row in database.execute(
                      "SELECT * FROM semantic_outbox ORDER BY task_id")],
                  "manifest": [dict(row) for row in database.execute(
                      "SELECT * FROM semantic_manifest ORDER BY doc_key")]}
    evidence.event("database_read_only", seed=seed, phase=phase, snapshot=result)
    return result


def cache_snapshot(cache):
    return {str(path.relative_to(cache)): {"sha256": sha256(path),
            "bytes": path.stat().st_size}
            for path in sorted(cache.rglob("*.bin"))}


def verify_local(product, seed, source):
    for tool, key in (("search", "query"), ("context", "task")):
        result = product.tool(tool, {key: f"survivor_{seed}", "retrieval_strategy": "local"})
        hits = result["machine_pack"]["hits"]
        require(hits, "local source must remain available during the held provider", result)
        for hit in hits:
            require(hit["file_path"] == "keep.rs" and hit["text"] and
                    hit["text"] in source, "local answer must bind current source", hit)
        require(all(span["file_path"] == "keep.rs" for span in result["spans"]),
                "deleted source must not be hydrated", result["spans"])
    removed = product.tool("search", {"query": f"removed_{seed}", "mode": "symbol"})
    require(removed == [], "deleted symbol must never revive", removed)


def wait_real_deadline(unix_deadline, evidence, seed, phase, check=None):
    started = time.time()
    evidence.progress(seed, phase, started_at=started, deadline=unix_deadline)
    next_check = time.monotonic()
    while time.time() <= unix_deadline:
        if check is not None and time.monotonic() >= next_check:
            check()
            next_check = time.monotonic() + 5
        time.sleep(min(.2, max(.01, unix_deadline - time.time())))
    evidence.event("real_deadline_elapsed", seed=seed, phase=phase,
                   started_at=started, finished_at=time.time(), deadline=unix_deadline)


def run_seed(binary, output, evidence, seed):
    case_root = output / f"seed-{seed}"
    root, cache, home = case_root / "project", case_root / "semantic-cache", case_root / "home"
    root.mkdir(parents=True)
    home.mkdir()
    initial_source = f"pub fn survivor_{seed}() -> u32 {{ {seed} }}\n"
    current_source = f"pub fn survivor_{seed}() -> u32 {{ {seed + 1000} }}\n"
    (root / "keep.rs").write_text(initial_source)
    (root / "remove.rs").write_text(f"pub fn removed_{seed}() -> u32 {{ {seed + 1} }}\n")
    provider = Provider(seed, evidence)
    config = {"auto_index": {"enabled": False}, "query": {"strategy": "local"},
              "semantic": {"enabled": True, "network_opt_in": True,
                           "allow_query_network": False, "allow_http": True,
                           "endpoint": provider.endpoint,
                           "model_id": f"fake/p7-faults/{seed}", "dimensions": 2,
                           "max_input_tokens": 8192, "max_batch_items": 16,
                           "api_key_ref": "env:P7_FAULTS_DUMMY",
                           "breaker_failure_threshold": 100, "retry_max_attempts": 1,
                           "worker_lease_secs": 2, "gc_min_retention_secs": 1}}
    (root / ".codecortex.json").write_text(json.dumps(config))
    children, checkpoints = [], []
    result = {"seed": seed, "passed": False, "configuration": config}

    def spawn(label):
        product = Product(binary, root, cache, home, f"seed-{seed}-{label}", evidence)
        children.append(product)
        return product

    def checkpoint(phase, product, **data):
        row = {"phase": phase, "status": product.status(),
               "observed_document_posts": provider.count(), **data}
        checkpoints.append(row)
        evidence.progress(seed, phase, **{key: value for key, value in row.items()
                                         if key != "phase"})

    try:
        first = spawn("initial")
        first.index()
        first.wait_state("ready", 2)
        first.stable_status()
        initial_cache = cache_snapshot(cache)
        require(len(initial_cache) == 2, "two genuine initial cache objects", initial_cache)
        require(sum(row["document_inputs"] for row in provider.snapshot()) == 2,
                "initial provider inputs correspond to both fixture documents")
        checkpoint("initial_ready", first, cache=initial_cache)
        first.finish()

        before_restart = provider.count()
        child = spawn("warm-restart")
        child.index()
        ready = child.wait_state("ready", 2)
        require(provider.count() == before_restart, "clean restart must reuse document cache")
        checkpoint("clean_restart_zero_document_calls", child, document_posts_delta=0)
        # The public GC retention knob is one second. Age actual product
        # objects with real time; never forge cache metadata or timestamps.
        wait_real_deadline(time.time() + 1.2, evidence, seed, "age_real_cache")
        (root / "keep.rs").write_text(current_source)
        (root / "remove.rs").unlink()
        provider.set_mode("fail_once", "rebuilt-provider-500")
        before_failure = provider.count()
        child.index(full=True)
        require(provider.entered.wait(5), "rebuild must reach the actual HTTP provider")
        pending = child.wait_state("backfilling", 0)
        require(provider.count() == before_failure + 1, "exactly one initial HTTP 500 attempt")
        require(pending["generation"]["incarnation"] != ready["generation"]["incarnation"],
                "full rebuild must replace the database incarnation")
        require(pending["semantic_pending"] == 1 and pending["semantic_failed"] == 0,
                "HTTP 500 remains pending within the original attempt budget", pending)
        snapshot = database_snapshot(root, evidence, seed, "http500_pending")
        tasks = [row for row in snapshot["outbox"] if row["state"] == "pending"]
        require(len(tasks) == 1 and tasks[0]["attempt_count"] == 1,
                "durable retry retains the exact original task", tasks)
        retry_at = tasks[0]["available_at"]
        response = provider.snapshot()[-1]
        require(retry_at - response["received_at"] >= 29,
                "production durable backoff must remain approximately 30 seconds", tasks)
        after_gc = cache_snapshot(cache)
        require(not set(initial_cache).intersection(after_gc),
                "automatic post-index GC must delete both aged unreferenced objects", after_gc)
        verify_local(child, seed, current_source)
        checkpoint("rebuild_delete_http500_pending_and_automatic_gc", child,
                   outbox=snapshot["outbox"], original_cache_removed=list(initial_cache),
                   cache_after_gc=after_gc, retry_at=retry_at)

        def stable_pending():
            status = child.stable_status()
            require(status["semantic_pending"] == 1 and provider.count() == before_failure + 1,
                    "status reads and elapsed time must not self-schedule retries", status)
        wait_real_deadline(retry_at + .05, evidence, seed, "unchanged_30_second_backoff",
                           stable_pending)
        provider.set_mode("hold", "held_retry_before_sigkill")
        child.index()
        require(provider.entered.wait(5), "explicit next index must reach retry HTTP")
        held = child.status()
        require(held["semantic_state"] == "backfilling" and held["query_pins"] >= 1,
                "held provider must retain its physical worker pin", held)
        verify_local(child, seed, current_source)
        claimed = database_snapshot(root, evidence, seed, "held_before_sigkill")
        live = [row for row in claimed["outbox"] if row["state"] == "claimed"]
        require(len(live) == 1 and live[0]["attempt_count"] == 2,
                "retry claims the same task without resetting its budget", live)
        lease_deadline = live[0]["lease_expires_at"]
        kill_time = time.time()
        require(kill_time < lease_deadline, "kill must precede the real two-second lease expiry",
                {"kill_time": kill_time, "lease_deadline": lease_deadline})
        child.finish(kill=True)
        provider.set_mode("success", "recovered_after_sigkill")
        wait_real_deadline(lease_deadline + .05, evidence, seed, "real_lease_expiry_after_sigkill")
        before_recover = provider.count()
        recovered = spawn("crash-recovery")
        recovered.index()
        recovered.wait_state("ready", 1)
        verify_local(recovered, seed, current_source)
        recovered.stable_status()
        require(provider.count() == before_recover + 1,
                "killed-before-response work needs one observed recovery HTTP attempt")
        after = database_snapshot(root, evidence, seed, "recovered_ready")
        require(not any(row["state"] in ("pending", "claimed", "failed")
                        for row in after["outbox"]), "recovery must leave no live or dead task", after)
        require(any(row["attempt_count"] == 3 and row["state"] == "done"
                    for row in after["outbox"]), "crash recovery preserves the three claims", after)
        checkpoint("sigkill_lease_reclaimed_ready", recovered, kill_time=kill_time,
                   lease_deadline=lease_deadline, document_posts_delta=1,
                   crash_attempt_billing="unknown; response had not been consumed", outbox=after)
        recovered.finish()

        before_warm = provider.count()
        warm = spawn("final-cache-reuse")
        warm.index()
        warm.wait_state("ready", 1)
        verify_local(warm, seed, current_source)
        warm.stable_status()
        require(provider.count() == before_warm, "settled restart must make zero new document calls")
        final_cache = cache_snapshot(cache)
        require(len(final_cache) == 1, "only the current published object remains", final_cache)
        checkpoint("final_restart_cache_reuse_and_read_only_status", warm,
                   document_posts_delta=0, cache=final_cache)
        warm.finish()
        logs = "\n".join(child.log_path.read_text(errors="replace") for child in children)
        require("semantic worker GC page completed" in logs,
                "actual runtime automatic GC execution must appear in product logs")
        result.update(passed=True, checkpoints=checkpoints, cache_initial=initial_cache,
                      cache_final=final_cache, observed_document_posts=provider.count(),
                      observed_document_inputs=sum(row["document_inputs"] for row in provider.snapshot()),
                      observed_sigkill_count=1, paid_cost=None, billing_unknown_after_crash=True,
                      manual_database_or_clock_changes=False,
                      scope="representative combined L3 recovery, synthetic vectors only")
    except BaseException as error:
        result.update(error=repr(error), traceback=traceback.format_exc(), checkpoints=checkpoints)
        raise
    finally:
        provider.release.set()
        for child in children:
            try:
                child.finish(cleanup=True)
            except BaseException as error:
                evidence.event("cleanup_error", seed=seed, session=child.label, error=repr(error))
        provider.close()
        result["http"] = provider.snapshot()
        result["exits"] = [child.exit_receipt for child in children]
        write_json(case_root / "result.json", result)
    return result


def verify_identity(binary, receipt):
    require(receipt.get("build_exit_code") == 0, "successful actual product build receipt")
    require(Path(receipt["binary_path"]).resolve() == binary, "receipt names actual product")
    require(receipt["binary_sha256"] == sha256(binary), "product digest matches build receipt")
    artifact = receipt["cargo_artifact"]
    require(artifact["reason"] == "compiler-artifact" and
            artifact["target"]["name"] == "codecortex" and
            artifact["target"]["kind"] == ["bin"], "Cargo artifact is the real product", artifact)
    require(set(artifact["features"]) == {"semantic", "semantic-http"},
            "exact semantic-http product features", artifact["features"])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--build-receipt", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--seed", action="append", type=int,
                        help="run selected declared seeds; formal matrix uses all three")
    args = parser.parse_args()
    binary, output = args.binary.resolve(), args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    evidence = Evidence(output)
    results = []
    summary = {"schema_version": 1, "passed": False,
               "script_sha256": sha256(__file__), "binary_sha256": sha256(binary),
               "declared_seeds": list(SEEDS), "executed_seeds": args.seed or list(SEEDS),
               "original_lifecycle_script_modified": False,
               "internal_put_cas_gc_wal_windows": "separately bound production fault tests required",
               "live_provider_quality": "not_run; synthetic loopback only"}
    try:
        receipt = json.loads(args.build_receipt.read_text())
        verify_identity(binary, receipt)
        write_json(output / "build-receipt.json", receipt)
        for seed in summary["executed_seeds"]:
            require(seed in SEEDS, "only declared replay seeds", seed)
            results.append(run_seed(binary, output, evidence, seed))
        verify_identity(binary, receipt)
        summary.update(passed=True, results=results,
                       complete_declared_matrix=summary["executed_seeds"] == list(SEEDS))
        return 0
    except BaseException as error:
        summary.update(error=repr(error), traceback=traceback.format_exc(), results=results)
        return 1
    finally:
        write_json(output / "summary.json", summary)
        print(json.dumps({"passed": summary["passed"], "output": str(output),
                          "error": summary.get("error")}), flush=True)


if __name__ == "__main__":
    raise SystemExit(main())
