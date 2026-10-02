#!/usr/bin/env python3
"""Assemble docs/roadmap/code-index-v2/P5-GATE-draft.json from this round's receipts.

DRAFT ONLY: finalization (naming to P5-G5-GATE.json, 06-VALIDATION.md header
update, tasks.json backfill, independent audit) belongs to the closeout/audit
round per the P5-020 execution redlines.
"""
import datetime as dt
import json

REPO = "/Users/jin/Desktop/codecortex-rust"
EXEC = f"{REPO}/artifacts/checkpoints/p5-020-execution-20261002"
FREEZE = f"{REPO}/artifacts/benchmarks/p5e-g5-freeze-20261002"
FIX1 = f"{REPO}/artifacts/benchmarks/p5e-formal-runs-20261002-fix1"
V6 = f"{REPO}/artifacts/benchmarks/p5e-candidate-release-20261002-v6"

reg = json.load(open(f"{FREEZE}/regression/validation.json"))
cmds = {c["label"]: c for c in reg["commands"]}


def toolchain(prefix):
    return {
        "clippy": {"exit_code": cmds[f"{prefix}-clippy"]["exit_code"], "seconds": cmds[f"{prefix}-clippy"]["seconds"]},
        "workspace": {"passed": cmds[f"{prefix}-workspace"]["passed"], "failed": cmds[f"{prefix}-workspace"]["failed"],
                      "ignored": cmds[f"{prefix}-workspace"]["ignored"], "exit_code": cmds[f"{prefix}-workspace"]["exit_code"],
                      "seconds": cmds[f"{prefix}-workspace"]["seconds"]},
        "http": {"passed": cmds[f"{prefix}-http"]["passed"], "failed": cmds[f"{prefix}-http"]["failed"],
                 "ignored": cmds[f"{prefix}-http"]["ignored"], "exit_code": cmds[f"{prefix}-http"]["exit_code"],
                 "seconds": cmds[f"{prefix}-http"]["seconds"]},
    }


ws_s = cmds["stable-workspace"]
ws_1 = cmds["1950-workspace"]
gate = {
    "draft": True,
    "draft_note": "DRAFT produced by P5-020 execution round 2026-10-02; finalization (rename to P5-G5-GATE.json, 06-VALIDATION.md freeze header update, tasks.json evidence backfill, independent audit per PLAYBOOK 4.3) belongs to the closeout/audit round. Do not treat as final gate verdict.",
    "batch": "P5-E",
    "gate": "G5",
    "accepted_tasks": ["P5-019", "P5-020"],
    "status": "draft_passed_declared_local_scope",
    "G5": "draft_passed_declared_local_scope",
    "M2": "draft_passed_local_scope",
    "target_sha": "0de7c890fcb2a152b4b21eafc4d8ad1a2c3885a6",
    "covered_source_digest_sha256": "d1f5a7af5ff9b0d025b6dd8dc475c110f178735d757d22e3546ba8470b45a167",
    "covered_files": 629,
    "config_frozen_head": "0de7c890fcb2a152b4b21eafc4d8ad1a2c3885a6 (CONFIGURATION.md / MCP_TOOLS.md / crates/cc-model/src/config.rs inside closure; default semantic.enabled=false, tiny read pool; zero network zero key)",
    "input_contract": "14 tools and existing properties preserved; additive fields: retrieval_strategy x2 (already in baseline binary, verified unchanged); live tools/list diff frozen-v6 vs baseline release binary = zero schema drift; freshness observation fields (dispatch_observed_generation_change) are response-payload additive only, not tool-schema changes",
    "semantic_delivery": "no embedding required; capability_status shows semantic_state=not_configured and dense_state=disabled (provider_and_vector_publication_not_implemented); no ready impersonation; probed on frozen binary over stdio, zero network zero key",
    "freeze_anchor": "F0",
    "c4_disposition": "fixed_verified",
    "toolchain_tests": {
        "stable": {**toolchain("stable"),
                   "release-cost": {"exit_code": cmds["release-cost"]["exit_code"], "seconds": cmds["release-cost"]["seconds"],
                                    "passed": cmds["release-cost"]["passed"], "failed": cmds["release-cost"]["failed"],
                                    "ignored": cmds["release-cost"]["ignored"]}},
        "1.95.0": toolchain("1950"),
    },
    "validations": {
        "V11": {
            "status": "passed_local_scope_with_c4_fix_verified",
            "evidence": [
                "artifacts/benchmarks/p5e-formal-runs-20261002-fix1/MIXED-C4-FIX1-RECEIPT.json (candidate fixed arm census {Success:300, build:30}, 0 hard error, 0 Partial, 330/330 count lock; same-window dual-arm, covariates recorded; census-only criteria)",
                "artifacts/checkpoints/p5-020-execution-20261002/E2-c4-redgreen-RECEIPT.json (fence backoff bounded + commit-storm recovery, freshness refinement guards: true stale stays changed_during_query; cc-search 281/0, cc-server 249+41+9/0, p1d_concurrency 4/0/1)",
                "artifacts/benchmarks/p5e-formal-runs-20261001-v4/mixed-c1-paired-report.json + mixed-c8-paired-report.json (formal-v4 mechanism-scope reference, v5 closure)",
                "artifacts/benchmarks/p5e-formal-runs-20261001-v4-rerun-r1/c16-final/same-window/C16-FINAL-RECEIPT.json (c16 terminal receipt, both final arms 0 hard error)",
            ],
        },
        "V12": {
            "status": "passed_mechanism_scope_reference_plus_v6_fanout",
            "evidence": [
                "artifacts/benchmarks/p5e-formal-runs-20261002-fix1/FANOUT-V6-RECEIPT.json (fanout double-side bound to frozen v6 binary; criterion issues=[] zero failed requests)",
                "artifacts/benchmarks/p5e-formal-runs-20261001-v4/facets/ (facet cell_100-111 zero-fail + original51 strict budget; formal-v4 reference, v5 closure 78f83f0f...)",
                "artifacts/checkpoints/p5-020-execution-20261002/V12-facet-disposition.json (facet 8-cell v6 rebinding NO-GO on cost with zero delta-intersection analysis; audit round may escalate)",
            ],
        },
        "V18": {
            "status": "passed_local_scope",
            "evidence": [
                "artifacts/checkpoints/p5-020-execution-20261002/additive-contract.json (live tools/list: 14 tools identical names and schemas vs baseline release binary; zero drift)",
                "artifacts/checkpoints/p5-020-execution-20261002/v18-probe-frozen-v6.json (initialize/tools-list/capabilities/protocol-error/valid-search on frozen binary, stdio, zero network zero key)",
                "artifacts/checkpoints/p5-020-execution-20261002/v18-probe-baseline.json (same probe on baseline binary for parity)",
                "workspace protocol/real-mcp test groups inside regression logs (artifacts/benchmarks/p5e-g5-freeze-20261002/regression/)",
            ],
        },
        "V19": {
            "status": "passed_mechanism_scope_with_frozen_source_reference",
            "evidence": [
                "artifacts/benchmarks/p5e-formal-runs-20261001-v4/ (48 edges + 1224-request 8-cell matrix + 9 witness + original51 baseline strict red retained; 31/31 stage receipts)",
                "artifacts/benchmarks/p5e-formal-runs-20261001-v4-replay/REPLAY-RECEIPT.json (R2 25/25 PASS)",
                "artifacts/checkpoints/todolist-completion-audit/round07/formal-v4-quality-acceptance.json",
                "artifacts/checkpoints/p5-020-execution-20261002/E2-c4-redgreen-RECEIPT.json + artifacts/benchmarks/p5e-formal-runs-20261002-fix1/MIXED-C4-FIX1-RECEIPT.json (c4-fix criteria retest strengthens the concurrency surface; V19 main body remains pre-fix source mechanism-scope evidence)",
            ],
        },
        "V20": {
            "status": "passed_mechanism_scope_plus_frozen_source_concurrency_observations",
            "evidence": [
                "artifacts/benchmarks/p5e-formal-runs-20261001-v4/mixed-c1-paired-report.json + mixed-c8-paired-report.json (per-lane latency + native resource proof; formal-v4 reference)",
                "artifacts/benchmarks/p5e-formal-runs-20261001-v4-rerun-r1/c16-final/same-window/mixed-c16-paired-report.json (same-window c16; final arms 0 hard error; recorded as-is with invalid_workload_comparison status note)",
                "artifacts/benchmarks/p5e-formal-runs-20261002-fix1/MIXED-C4-FIX1-RECEIPT.json (c4 concurrency point observed on frozen source; census-only, latency not certified)",
                "artifacts/benchmarks/p5d-20260930-resume/final-v3/lifecycle-cost-summary.json (lifecycle-cost reference)",
                "artifacts/benchmarks/p5e-g5-freeze-20261002/regression/validation.json (release-cost command, frozen source)",
            ],
        },
    },
    "acceptance": {
        "semantic_disabled_only": {
            "status": "passed",
            "evidence": "artifacts/checkpoints/p5-020-execution-20261002/additive-contract.json probes: semantic_state=not_configured, dense_state=disabled, reason=provider_and_vector_publication_not_implemented; default config stdio, no cache files, no endpoint probing, no worker",
        },
        "old_functionality_regression": {
            "status": "passed_dual_toolchain_with_documented_environment_flake" if (ws_s["failed"] or ws_1["failed"]) else "passed_dual_toolchain",
            "numbers": {"stable": {"workspace": ws_s, "http": cmds["stable-http"]},
                        "1.95.0": {"workspace": ws_1, "http": cmds["1950-http"]},
                        "clippy_both": "0 warnings (-D warnings) rc=0",
                        "release-cost": cmds["release-cost"]},
            "single_failure_disposition": "the only failing test on both toolchains is cc-eval lib::tests::benchmark_fixture (perf-threshold, index warm p95 549-665ms vs 500ms limit under ambient load 7.6-12); causal control on detached-HEAD 0de7c890 (no round delta) reproduced the same failure and then passed minutes later -> environment-sensitive flake, not round-caused; threshold untouched; see artifacts/checkpoints/p5-020-execution-20261002/E5-regression-disposition.json; final verdict delegated to independent audit round",
            "known_red_retained": "original source/intent strict Partial and S11 known failures retained as-is, not claimed green; ignored counts listed explicitly and not counted as passes",
        },
    },
    "full_retrieval_gate": "not_passed_known_failures_retained",
    "artifacts": [
        "artifacts/benchmarks/p5e-candidate-release-20261002-v6/source-manifest.json",
        "artifacts/benchmarks/p5e-candidate-release-20261002-v6/BUILD-RECEIPT.json",
        "artifacts/benchmarks/p5e-candidate-release-20261002-v6/binaries/codecortex",
        "artifacts/benchmarks/p5e-g5-freeze-20261002/F0-FREEZE-RECEIPT.json",
        "artifacts/benchmarks/p5e-g5-freeze-20261002/regression/validation.json",
        "artifacts/benchmarks/p5e-formal-runs-20261002-fix1/MIXED-C4-FIX1-RECEIPT.json",
        "artifacts/benchmarks/p5e-formal-runs-20261002-fix1/FANOUT-V6-RECEIPT.json",
        "artifacts/checkpoints/p5-020-execution-20261002/E1-c16-receipt-verification.json",
        "artifacts/checkpoints/p5-020-execution-20261002/E2-c4-redgreen-RECEIPT.json",
        "artifacts/checkpoints/p5-020-execution-20261002/additive-contract.json",
        "artifacts/checkpoints/p5-020-execution-20261002/v18-probe-frozen-v6.json",
        "artifacts/checkpoints/p5-020-execution-20261002/v18-probe-baseline.json",
        "artifacts/checkpoints/p5-020-execution-20261002/V12-facet-disposition.json",
        "artifacts/checkpoints/p5-020-execution-20261002/EXECUTION-PROGRESS.md",
        "artifacts/checkpoints/c4-race-analysis-20261002/IMPLEMENTATION-20261002.md",
        "artifacts/benchmarks/p5e-formal-runs-20261001-v4/STAGE-RECEIPTS.json",
        "artifacts/benchmarks/p5e-formal-runs-20261001-v4-rerun-r1/c16-final/same-window/C16-FINAL-RECEIPT.json",
        "artifacts/checkpoints/todolist-completion-audit/round07/formal-v4-quality-acceptance.json",
    ],
    "limitations": [
        "Original source/intent strict Partial and S11 known failures retained as-is; this is NOT a full retrieval quality certification.",
        "formal-v4 main-body evidence (V19 facets/edges/witness, V20 c1/c8 latency) measured on v5 closure 78f83f0faac3ca7c2816a74847f34947f8be8cd504316df1557390cdb63d1c10; frozen v6 delta = c4 fix + ABI projection, with c4 criteria retest (E3) and fanout rebinding (V12) as the strengthened frozen-source observations; facet 8-cell not rebuilt on v6 (zero delta-intersection, see V12-facet-disposition.json).",
        "No real provider / holdout / 100k / cross-platform / release certification; G7/G8 out of scope; semantic only shown disabled (no fake availability).",
        "mixed-c4 retest and fanout re-run executed outside a quiet window (structurally unreachable on this machine; same-window protocol precedent); criteria were census/functional-only and latency is not certified.",
        "Storm fence test has a known timing-sensitive flake window (assertion-failure direction); one parallel-mode flake observed and recorded in E2; sequential-mode (regression mode) stable.",
    ],
    "independent_audit": "pending (audit round; expected under artifacts/checkpoints/ per PLAYBOOK 4.3)",
    "next_phase": "P6 batch 1 (after F0: crates/ zero-change straight to batch-1 first commit; IMPLEMENTATION-ORDER.md:30)",
}
with open(f"{REPO}/docs/roadmap/code-index-v2/P5-GATE-draft.json", "w") as f:
    json.dump(gate, f, indent=2, ensure_ascii=False)
print("GATE draft written")
print(json.dumps(gate["toolchain_tests"], indent=1)[:800])
print("acceptance.regression:", gate["acceptance"]["old_functionality_regression"]["status"])
