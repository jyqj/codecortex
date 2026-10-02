#!/usr/bin/env python3
"""V12 fanout re-run bound to frozen source-v6 binary (P5-020 evidence).

Dual-arm same window (baseline binary for symmetry + frozen v6 candidate).
Criterion: issues=[] / zero failed requests (functional), not latency.
Locked argv from formal-v4 stage 14/15/25 (fanout-plan.json, compare_fanout.py).
"""
import datetime as dt
import hashlib
import json
import os
import subprocess
import time

REPO = "/Users/jin/Desktop/codecortex-rust"
DRIVER = f"{REPO}/artifacts/benchmarks/p5e-harness-20261001/final-v8/binaries/p5e-evidence-driver"
PLAN = f"{REPO}/artifacts/benchmarks/p5e-harness-20261001/final-v8/inputs/fanout-plan.json"
CMP = f"{REPO}/artifacts/benchmarks/p5e-harness-20261001/final-v8/inputs/compare_fanout.py"
CANDIDATE = f"{REPO}/artifacts/benchmarks/p5e-candidate-release-20261002-v6/binaries/codecortex"
BASELINE = f"{REPO}/artifacts/benchmarks/p5e-baseline-release-20261001/binaries/codecortex"
OUT = f"{REPO}/artifacts/benchmarks/p5e-formal-runs-20261002-fix1"


def sha256(p):
    return hashlib.sha256(open(p, "rb").read()).hexdigest()


def cov():
    la = os.getloadavg()
    tm = subprocess.run(["pgrep", "-f", "tm[-]r5bench"], capture_output=True, text=True).stdout.split()
    return {"utc": dt.datetime.now(dt.timezone.utc).isoformat(),
            "loadavg_1m_5m_15m": [round(la[0], 2), round(la[1], 2), round(la[2], 2)],
            "tm_r5bench_processes": len([x for x in tm if x.strip()])}


def arm(name, binary, outdir):
    assert not os.path.exists(outdir)
    env = dict(os.environ)
    env["CODECORTEX_BENCH_PROCESS_PROBE"] = "1"
    pre = cov()
    started = time.time()
    proc = subprocess.run([DRIVER, "graph", binary, PLAN, outdir], cwd=REPO,
                          capture_output=True, text=True, env=env)
    post = cov()
    return {"arm": name, "binary": binary, "binary_sha256": sha256(binary),
            "argv": [DRIVER, "graph", binary, PLAN, outdir],
            "return_code": proc.returncode, "elapsed_s": round(time.time() - started, 3),
            "stderr_tail": proc.stderr[-1000:] if proc.stderr else "",
            "covariates": {"pre": pre, "post": post}}


receipt = {"run_id": "p5e-formal-runs-20261002-fix1/fanout",
           "purpose": "V12 fanout double-side re-run bound to frozen source-v6 binary; criterion issues=[] zero failed requests (functional, not latency certification)",
           "input_sha256": {"driver": sha256(DRIVER), "plan": sha256(PLAN)},
           "arms": [], "paired_report": {}}
for name, binary, outdir in [
        ("candidate_v6", CANDIDATE, f"{OUT}/fanout-candidate-v6-r2"),
        ("baseline_prefix", BASELINE, f"{OUT}/fanout-baseline-prefix-r2")]:
    receipt["arms"].append(arm(name, binary, outdir))

for a in receipt["arms"]:
    s = json.load(open(f"{a['argv'][4]}/summary.json"))
    a["summary"] = {k: s.get(k) for k in ("status", "exit_code", "failed_checks", "queries") if k in s}
    a["criterion_issues_empty"] = (s.get("failed_checks") == 0 and s.get("exit_code") == 0)

cmp_ = subprocess.run(["python3", CMP,
                       "--baseline", f"{OUT}/fanout-baseline-prefix-r2",
                       "--candidate", f"{OUT}/fanout-candidate-v6-r2",
                       "--output", f"{OUT}/fanout-paired-report.json"],
                      capture_output=True, text=True)
receipt["paired_report"] = {"return_code": cmp_.returncode,
                            "path": f"{OUT}/fanout-paired-report.json",
                            "stderr_tail": cmp_.stderr[-500:]}
if cmp_.returncode == 0:
    receipt["paired_report"]["status"] = json.load(open(f"{OUT}/fanout-paired-report.json")).get("status")
receipt["finished_utc"] = dt.datetime.now(dt.timezone.utc).isoformat()
with open(f"{OUT}/FANOUT-V6-RECEIPT.json", "w") as f:
    json.dump(receipt, f, indent=2, ensure_ascii=False)
print(json.dumps({"arms": [{k: a[k] for k in ("arm", "return_code", "summary")} for a in receipt["arms"]],
                  "paired": receipt["paired_report"].get("status")}, ensure_ascii=False, indent=1))
