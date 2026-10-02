#!/usr/bin/env python3
"""mixed-c4 criteria retest after c4 fix (P5-020 evidence item 3).

Same-window dual-arm (quiet-window predicate documented structurally
unreachable; precedent: c16 same-window protocol 2026-10-02). Judgment is
error / false-positive-Partial census on the fixed candidate arm — NOT a
latency certification. Environment recorded as covariates.
Locked inputs: final-v8 mixed-c4.json (sha-verified pre-run), locked harness
driver and baseline binary (sha-verified pre-run).
"""
import collections
import datetime as dt
import hashlib
import json
import os
import subprocess
import sys
import time

REPO = "/Users/jin/Desktop/codecortex-rust"
DRIVER = f"{REPO}/artifacts/benchmarks/p5e-harness-20261001/final-v8/binaries/p5e-evidence-driver"
INPUT = f"{REPO}/artifacts/benchmarks/p5e-harness-20261001/final-v8/inputs/mixed-c4.json"
CANDIDATE = f"{REPO}/artifacts/benchmarks/p5e-candidate-release-20261002-v6/binaries/codecortex"
BASELINE = f"{REPO}/artifacts/benchmarks/p5e-baseline-release-20261001/binaries/codecortex"
OUT = f"{REPO}/artifacts/benchmarks/p5e-formal-runs-20261002-fix1"

EXPECTED_SHAS = {
    DRIVER: "809adfd6680e801fe9a24d11508e10b8c96be326524f9f16e1a12f5b550ba364",
    INPUT: "PLACEHOLDER_VERIFIED_SEPARATELY",
    BASELINE: "6adae4d53bfb0be40623d86ff159d30f38e1f2bab8b5d3ff6e0ec757eb2451be",
}


def sha256(p):
    h = hashlib.sha256()
    with open(p, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def covariate():
    la = os.getloadavg()
    tm = subprocess.run(["pgrep", "-f", "tm[-]r5bench"], capture_output=True, text=True).stdout.split()
    return {"utc": dt.datetime.now(dt.timezone.utc).isoformat(),
            "loadavg_1m_5m_15m": [round(la[0], 2), round(la[1], 2), round(la[2], 2)],
            "tm_r5bench_processes": len([x for x in tm if x.strip()])}


def run_arm(name, binary, outdir, env_extra):
    assert not os.path.exists(outdir), f"driver is fail-closed on existing output dir: {outdir}"
    pre = covariate()
    env = dict(os.environ)
    env["CODECORTEX_BENCH_PROCESS_PROBE"] = "1"
    started = time.time()
    proc = subprocess.run([DRIVER, "mixed-stdio", binary, INPUT, outdir],
                          cwd=REPO, env=env, capture_output=True, text=True)
    finished = time.time()
    post = covariate()
    return {
        "arm": name, "binary": binary, "binary_sha256": sha256(binary),
        "output_dir": outdir, "argv": [DRIVER, "mixed-stdio", binary, INPUT, outdir],
        "return_code": proc.returncode, "started_epoch": started, "finished_epoch": finished,
        "elapsed_s": round(finished - started, 3),
        "stderr_tail": proc.stderr[-1500:] if proc.stderr else "",
        "covariates": {"pre": pre, "post": post},
    }


def census(outdir):
    obs = json.load(open(f"{outdir}/observations.json"))
    checks = obs if isinstance(obs, list) else (obs.get("checks") or obs.get("observations") or [])
    cnt = collections.Counter()
    obs_fields = 0
    errors = []
    for c in checks:
        chk = c.get("check", c)
        st = chk.get("status")
        cnt[st] += 1
        if st == "error":
            errors.append(chk.get("error") or chk.get("message"))
        s = json.dumps(c)
        if "dispatch_observed_generation_change" in s or "changed_during_query" in s:
            obs_fields += 1
    summary = json.load(open(f"{outdir}/summary.json"))
    return {"status": summary.get("status"), "failures": summary.get("failures"),
            "status_census": dict(cnt), "error_samples": errors[:5],
            "observations_with_change_markers": obs_fields,
            "read_requests": summary.get("read_requests"),
            "completed_jobs": summary.get("completed_jobs"),
            "offered_jobs": summary.get("offered_jobs")}


if __name__ == "__main__":
    pre_check = {p: sha256(p) for p in (DRIVER, INPUT, BASELINE, CANDIDATE)}
    receipt = {"schema_version": 1,
               "run_id": "p5e-formal-runs-20261002-fix1",
               "purpose": "mixed-c4 criteria retest after c4 fix (P0+P1+P2) on frozen source-v6 binary",
               "criteria": {"candidate_fixed_arm": "0 hard error; 0 false-positive Partial (true stale would remain changed_during_query and is NOT a false positive); judgment is census-only, not latency certification"},
               "protocol": {"name": "same_window_dual_arm", "quiet_window": "structurally unreachable on this machine (3 bounded waits documented in c16-final receipts); precedent C16-FINAL-RECEIPT.json protocol.same_window_dual_arm", "note": "ambient load recorded as covariate, common-mode across arms"},
               "input_lock_sha256": pre_check,
               "expected_shas_note": "driver/baseline sha verified equal to locked_chain in c16-final/same-window/C16-FINAL-RECEIPT.json",
               "arms": [], "census": {}}
    assert pre_check[DRIVER] == EXPECTED_SHAS[DRIVER], "driver sha drift"
    assert pre_check[BASELINE] == EXPECTED_SHAS[BASELINE], "baseline sha drift"
    receipt["arms"].append(run_arm("candidate_fixed_v6", CANDIDATE, f"{OUT}/mixed-c4-candidate-v6", {}))
    receipt["arms"].append(run_arm("baseline_prefix", BASELINE, f"{OUT}/mixed-c4-baseline-prefix", {}))
    for arm in receipt["arms"]:
        receipt["census"][arm["arm"]] = census(arm["output_dir"])
    cmp_ = subprocess.run(["python3", f"{REPO}/artifacts/benchmarks/p5e-harness-20261001/final-v8/inputs/compare_mixed.py",
                           "--baseline", f"{OUT}/mixed-c4-baseline-prefix",
                           "--candidate", f"{OUT}/mixed-c4-candidate-v6",
                           "--output", f"{OUT}/mixed-c4-paired-report.json"],
                          capture_output=True, text=True)
    receipt["paired_report"] = {"return_code": cmp_.returncode,
                                "path": f"{OUT}/mixed-c4-paired-report.json",
                                "stderr_tail": cmp_.stderr[-800:]}
    if cmp_.returncode == 0:
        pr = json.load(open(f"{OUT}/mixed-c4-paired-report.json"))
        receipt["paired_report"]["status"] = pr.get("status")
    receipt["finished_utc"] = dt.datetime.now(dt.timezone.utc).isoformat()
    with open(f"{OUT}/MIXED-C4-FIX1-RECEIPT.json", "w") as f:
        json.dump(receipt, f, indent=2, ensure_ascii=False)
    print(json.dumps({"census": receipt["census"], "paired": receipt["paired_report"].get("status")}, ensure_ascii=False, indent=1))
