#!/usr/bin/env python3
"""V18 contract probe: MCP stdio JSON-RPC against a codecortex binary.

Sequence: initialize -> notifications/initialized -> tools/list ->
tools/capabilities (capability_status snapshot) -> protocol smoke
(unknown-tool error + valid search call). No network, no API key.
Writes one JSON receipt per binary.
"""
import json
import os
import subprocess
import sys
import tempfile
import time


def rpc(proc, id_, method, params=None):
    req = {"jsonrpc": "2.0", "id": id_, "method": method}
    if params is not None:
        req["params"] = params
    proc.stdin.write((json.dumps(req) + "\n").encode())
    proc.stdin.flush()
    deadline = time.time() + 60
    while time.time() < deadline:
        line = proc.stdout.readline()
        if not line:
            raise RuntimeError(f"stdout closed while waiting for id={id_}")
        line = line.strip()
        if not line:
            continue
        msg = json.loads(line)
        if msg.get("id") == id_:
            return msg
    raise RuntimeError(f"timeout waiting for id={id_}")


def notify(proc, method, params=None):
    req = {"jsonrpc": "2.0", "method": method}
    if params is not None:
        req["params"] = params
    proc.stdin.write((json.dumps(req) + "\n").encode())
    proc.stdin.flush()


def probe(binary, receipt_path, project):
    env = dict(os.environ)
    env["CODECORTEX_PPID_POLL_MS"] = "0"
    proc = subprocess.Popen(
        [binary, "mcp", "--project-path", project],
        stdin=subprocess.PIPE, stdout=subprocess.PIPE,
        stderr=subprocess.PIPE, env=env)
    receipt = {"binary": binary, "steps": {}}
    try:
        init = rpc(proc, 1, "initialize", {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "p5-020-v18-probe", "version": "1"},
        })
        receipt["steps"]["initialize"] = {
            "ok": "error" not in init,
            "server_info": (init.get("result", {}).get("serverInfo")),
        }
        notify(proc, "notifications/initialized")

        tl = rpc(proc, 2, "tools/list", {})
        tools = tl.get("result", {}).get("tools", [])
        receipt["steps"]["tools/list"] = {
            "count": len(tools),
            "names": sorted(t["name"] for t in tools),
            "contracts": {t["name"]: {
                "required": sorted(t.get("inputSchema", {}).get("required", [])),
                "properties": sorted(t.get("inputSchema", {}).get("properties", {}).keys()),
            } for t in tools},
        }

        cap = rpc(proc, 3, "tools/call", {
            "name": "status", "arguments": {"aspect": "capabilities"}})
        content = cap.get("result", {})
        sc = content.get("structuredContent") or {}
        retr = (sc.get("result", {}).get("retrieval")
                if isinstance(sc.get("result"), dict) else None)
        receipt["steps"]["capabilities"] = {
            "is_error": content.get("isError"),
            "semantic_state": retr and retr.get("semantic_state"),
            "dense_state": retr and retr.get("dense_state"),
            "dense_reason": retr and retr.get("dense_reason"),
            "structured_head": json.dumps(sc)[:400],
        }

        err = rpc(proc, 4, "tools/call", {"name": "no_such_tool", "arguments": {}})
        receipt["steps"]["protocol_error_unknown_tool"] = {
            "has_error": "error" in err,
            "code": err.get("error", {}).get("code"),
        }

        ok = rpc(proc, 5, "tools/call", {"name": "status", "arguments": {}})
        r5 = ok.get("result", {})
        receipt["steps"]["status_call"] = {
            "is_error": r5.get("isError"),
            "has_structured": bool(r5.get("structuredContent")),
        }

        srch = rpc(proc, 6, "tools/call", {
            "name": "search", "arguments": {"query": "needle", "top_k": 5}})
        r6 = srch.get("result", {})
        receipt["steps"]["search_call"] = {
            "is_error": r6.get("isError"),
            "has_structured": bool(r6.get("structuredContent")),
            "error_content_head": json.dumps(r6.get("content", ""))[:300],
        }
    finally:
        try:
            proc.stdin.close()
        except Exception:
            pass
        try:
            proc.terminate()
            proc.wait(timeout=10)
        except Exception:
            proc.kill()
        receipt["server_stderr_tail"] = proc.stderr.read().decode(errors="replace")[-2000:]
    with open(receipt_path, "w") as f:
        json.dump(receipt, f, indent=2, ensure_ascii=False)
    return receipt


if __name__ == "__main__":
    binary, receipt_path = sys.argv[1], sys.argv[2]
    project = sys.argv[3]
    receipt = probe(binary, receipt_path, project)
    print(json.dumps({k: v for k, v in receipt.items() if k != "steps"}, ensure_ascii=False))
    for name, st in receipt["steps"].items():
        print(name, "->", json.dumps(st, ensure_ascii=False)[:400])
