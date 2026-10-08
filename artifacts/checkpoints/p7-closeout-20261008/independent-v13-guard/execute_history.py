"""Read-only invocation of the current frozen v12 proof against historical artifacts."""
import hashlib
import importlib
import json
from functools import lru_cache
from pathlib import Path
import subprocess
import sys
import time
import traceback

source = Path(sys.argv[1]).resolve()
history = Path(sys.argv[2]).resolve()
output = Path(sys.argv[3]).resolve()
sys.path.insert(0, str(source / "scripts"))
guard = importlib.import_module("verify_reviewed_source_v12")
started = time.time()


def digest(raw):
    return hashlib.sha256(raw).hexdigest()


def head(root):
    return subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip()


modules = {}


def visit(module):
    if module.__name__ in modules:
        return
    modules[module.__name__] = module
    for name in ("previous", "joint", "p7", "p8", "v1", "v2"):
        child = getattr(module, name, None)
        if getattr(child, "__name__", "").startswith("verify_"):
            visit(child)


visit(guard)
proofs = {name: module for name, module in modules.items() if hasattr(module, "approved_union")}
code_names = {module.approved_union.__code__: name for name, module in proofs.items()}
calls = {name: 0 for name in proofs}
returns = {name: 0 for name in proofs}
script_hashes = {name: digest(Path(module.__file__).read_bytes()) for name, module in modules.items()}
receipt = {
    "spec": "p7-closeout-independent-historical-execution-v1",
    "source_checkout_commit": head(source),
    "frozen_guard_version": guard.VERSION,
    "frozen_guard_sha256": script_hashes[guard.__name__],
    "historical_artifact_checkout_commit": head(history),
    "historical_artifact_root": str(history),
    "artifact_checkout_used_read_only": True,
    "module_sha256": script_hashes,
    "registry_sha256": digest(guard.REGISTRY.read_bytes()),
    "invocation": list(sys.argv),
    "cache_scope": "immutable Git blobs only; actual proof functions and filesystem checks execute",
    "scope": "historical v12 and both full prior proof chains; no current runtime or new v13 acceptance",
    "status": "running",
    "started_unix": started,
}
output.write_text(json.dumps(receipt, indent=2) + "\n")


def profile(frame, event, arg):
    name = code_names.get(frame.f_code)
    if name is None or event not in ("call", "return"):
        return
    if event == "call":
        calls[name] += 1
    else:
        returns[name] += int(isinstance(arg, dict))
    print(json.dumps({"event": event, "proof": name,
                      "completed_input_count": len(arg) if isinstance(arg, dict) else None,
                      "elapsed_s": round(time.time() - started, 3)}), flush=True)


guard.git.blob = lru_cache(maxsize=4096)(guard.git.blob)
exit_code = 0
try:
    sys.setprofile(profile)
    result = guard.approved_union(guard.load_registry(), root=history)
    sys.setprofile(None)
    assert all(calls.values()), "a reachable old proof did not execute"
    assert all(returns.values()), "a reachable old proof did not finish with an approved inventory"
    assert script_hashes == {name: digest(Path(module.__file__).read_bytes())
                             for name, module in modules.items()}, "proof script bytes changed during run"
    expected = {name: digest(raw) for name, raw in result.items()}
    receipt.update(status="passed", complete_inputs=len(result),
                   complete_input_manifest_sha256=digest(json.dumps(expected, sort_keys=True).encode()))
except BaseException as exc:
    sys.setprofile(None)
    exit_code = 1
    receipt.update(status="failed", error_type=type(exc).__name__, error=str(exc))
    traceback.print_exc()
finally:
    receipt.update(proof_calls=calls, proof_successful_returns=returns, exit_code=exit_code,
                   finished_unix=time.time(), elapsed_s=round(time.time() - started, 3))
    output.write_text(json.dumps(receipt, indent=2) + "\n")
    print(json.dumps(receipt, sort_keys=True), flush=True)
raise SystemExit(exit_code)
