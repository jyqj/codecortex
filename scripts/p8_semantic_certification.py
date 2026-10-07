#!/usr/bin/env python3
"""Check bounded local evidence for a semantic release; never issue a certificate.

File integrity and cross-record consistency are machine-checkable here. A
supplied JSON declaration cannot authenticate provider execution, independent
holdout custody, approval authority, or gold scoring. Those external trust gates
remain blocked even for a structurally complete packet. No model is called.
"""

import argparse
from datetime import datetime, timezone
from decimal import Decimal, InvalidOperation
import ipaddress
import json
import math
from pathlib import Path
import re
import sys
from urllib.parse import urlsplit

import p8_judge_evidence as common
import p8_release_evidence as support


ROLES = {"input_lock", "custody", "execution", "attempts", "costs", "budget", "quality"}
BINDING = {"source_commit", "binary_sha256", "config_sha256", "scoring_sha256",
           "input_lock_sha256", "corpus_sha256", "query_sha256", "gold_sha256",
           "case_set_sha256"}
STATUSES = {"success", "no_match", "partial", "timeout", "tool_error", "protocol_error", "cancelled"}
SUCCESS = {"success", "no_match"}
require = common.require
fields = common.fields
text = common.text
integer = common.integer


def finite(value, low, high, label):
    require(type(value) in (int, float) and math.isfinite(value) and low <= value <= high,
            "invalid " + label)
    return value


def money(value):
    require(isinstance(value, str) and len(value) <= 32
            and re.fullmatch(r"[0-9]+(?:\.[0-9]{1,9})?", value), "decimal money string required")
    try:
        result = Decimal(value)
    except InvalidOperation as exc:
        raise support.Invalid("invalid money") from exc
    require(result <= Decimal("1000000000"), "money amount limit")
    return result


def timestamp(value):
    text(value, "timestamp", 64)
    try:
        result = datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError as exc:
        raise support.Invalid("ISO timestamp required") from exc
    require(result.utcoffset() is not None, "timezone required")
    return result.astimezone(timezone.utc)


def public_https(endpoint):
    try:
        endpoint = text(endpoint, "provider endpoint", 2048)
        parsed = urlsplit(endpoint)
        host = parsed.hostname
        if (parsed.scheme != "https" or not host or parsed.username or parsed.password
                or parsed.query or parsed.fragment or parsed.port not in (None, 443)
                or host.lower() == "localhost" or "." not in host
                or host.lower().endswith((".localhost", ".invalid", ".test", ".example"))):
            return False
        try:
            return ipaddress.ip_address(host).is_global
        except ValueError:
            return bool(re.fullmatch(r"[a-zA-Z0-9.-]+", host))
    except (ValueError, support.Invalid):
        return False


def binding(value):
    fields(value, BINDING)
    require(isinstance(value["source_commit"], str)
            and re.fullmatch(r"[0-9a-f]{40}", value["source_commit"]), "fixed source commit required")
    for key in BINDING - {"source_commit"}:
        common.hash_value(value[key])
    return value


def _evaluate(manifest_path, output):
    manifest, manifest_raw = common.read_object(manifest_path)
    fields(manifest, {"schema_version", "run_id", "binding", "model", "thresholds", "records"})
    require(type(manifest["schema_version"]) is int and manifest["schema_version"] == 1,
            "unsupported semantic evidence schema")
    run_id = text(manifest["run_id"], "run ID", 128)
    pinned = binding(manifest["binding"])
    selected_model = common.model(manifest["model"])
    thresholds = manifest["thresholds"]
    fields(thresholds, {"min_cases", "max_error_rate", "min_mean_gain"})
    integer(thresholds["min_cases"], 1, 10000, "minimum cases")
    finite(thresholds["max_error_rate"], 0, 1, "maximum error rate")
    finite(thresholds["min_mean_gain"], -1, 1, "minimum observed mean gain")
    require(isinstance(manifest["records"], dict) and set(manifest["records"]) <= ROLES,
            "unknown evidence role")
    checks, records, observed = [], {}, {}
    def check(name, passed, detail=None):
        checks.append({"name": name, "passed": bool(passed), "detail": detail})
    result = {"schema_version": 1, "kind": "p8_semantic_evidence_admission",
        "run_id": run_id, "manifest_sha256": common.sha(manifest_raw),
        "binding": pinned, "model": selected_model, "status": "blocked", "exit_code": 1,
        "release_certified": False, "M4_semantic": "blocked", "model_calls": 0, "network_calls": 0,
        "local_profile": {"status": "not_assessed", "modified": False},
        "external_authenticity": "not_verified",
        "statistical_acceptance": "not_run", "checks": checks, "observed_files": observed}
    metadata = output / "metadata"
    metadata.mkdir(mode=0o700)
    common.private_write(metadata / "manifest.json", manifest_raw)
    for role in sorted(ROLES):
        ref = manifest["records"].get(role)
        check("present_" + role, ref is not None)
        if ref is None:
            continue
        fields(ref, {"path", "sha256"})
        original = common.relative(Path(manifest_path).parent, ref["path"])
        support.disjoint(output, [original])
        value, raw = common.read_object(original)
        require(common.sha(raw) == common.hash_value(ref["sha256"]), "evidence file digest mismatch: " + role)
        common.private_write(metadata / (role + ".json"), raw)
        records[role] = value
        observed[role] = {"sha256": common.sha(raw), "bytes": len(raw)}
    # Self-asserted signatures, trust booleans, and operator declarations are
    # never promoted to authentic execution/custody/approval by this tool.
    external_blockers = ["authenticated_provider_execution_required",
        "independent_clean_custody_verification_required", "approval_authority_verification_required",
        "deterministic_scoring_and_statistical_gate_verification_required"]
    result["external_blockers"] = external_blockers
    if set(records) != ROLES:
        result["structural_status"] = "incomplete"
        result["blockers"] = [c["name"] for c in checks if not c["passed"]] + external_blockers
        return result
    if records["input_lock"].get("kind") == "p8_local_candidate":
        check("semantic_input_lock_kind", False, "local candidate remains local; no semantic promotion")
        result["structural_status"] = "blocked_local_candidate"
        result["blockers"] = ["semantic_input_lock_kind", *external_blockers]
        return result
    for role, value in records.items():
        require(type(value.get("schema_version")) is int and value["schema_version"] == 1,
                "unsupported record schema: " + role)
        expected = {k: v for k, v in pinned.items() if k != "input_lock_sha256"} if role == "input_lock" else pinned
        check("binding_" + role, value.get("binding") == expected)

    lock = records["input_lock"]
    fields(lock, {"schema_version", "kind", "binding", "model", "files"})
    check("semantic_input_lock_kind", lock["kind"] == "p8_semantic_input_lock")
    check("input_lock_digest", observed["input_lock"]["sha256"] == pinned["input_lock_sha256"])
    check("input_lock_model", lock["model"] == selected_model)
    fields(lock["files"], {"binary", "config", "scoring"})
    lock_base = common.relative(Path(manifest_path).parent, manifest["records"]["input_lock"]["path"]).parent
    for role, ref in lock["files"].items():
        fields(ref, {"path", "sha256"})
        original = common.relative(lock_base, ref["path"])
        support.disjoint(output, [original])
        actual = support.file_record(original, limit=64 * 1024 * 1024)
        check("actual_" + role + "_bytes", actual["sha256"] == common.hash_value(ref["sha256"])
              == pinned[role + "_sha256"])
        observed["candidate_" + role] = actual
        if role == "config":
            support.no_secrets(common.read_object(original)[0])

    custody = records["custody"]
    fields(custody, {"schema_version", "binding", "source_kind", "split", "status",
                    "development_overlap", "case_ids", "audit_sha256", "custodian"})
    require(isinstance(custody["case_ids"], list) and 0 < len(custody["case_ids"]) <= 10000,
            "bounded nonempty case IDs required")
    case_ids = [text(cid, "case ID", 128) for cid in custody["case_ids"]]
    require(len(case_ids) == len(set(case_ids)), "duplicate custody case ID")
    case_set = set(case_ids)
    integer(custody["development_overlap"], 0, 10000, "development overlap")
    text(custody["custodian"], "custodian", 256)
    common.hash_value(custody["audit_sha256"])
    check("clean_heldout_declaration", custody["source_kind"] == "independent_custody_export"
          and custody["split"] == "heldout" and custody["status"] == "clean"
          and custody["development_overlap"] == 0,
          "metadata consistency only; custody authenticity is an external blocker")
    check("case_inventory_digest", support.digest(sorted(case_ids)) == pinned["case_set_sha256"])
    check("minimum_cases", len(case_ids) >= thresholds["min_cases"])

    execution = records["execution"]
    fields(execution, {"schema_version", "binding", "model", "run_id", "source_kind",
                      "endpoint", "started_at", "ended_at", "requests"})
    check("live_provider_origin_declaration", execution["source_kind"] == "live_provider_export"
          and public_https(execution["endpoint"]), "HTTPS metadata is not proof that a request occurred")
    check("execution_run_and_model", execution["run_id"] == run_id and execution["model"] == selected_model)
    started, ended = timestamp(execution["started_at"]), timestamp(execution["ended_at"])
    require(started <= ended, "execution time order invalid")
    require(isinstance(execution["requests"], list) and 0 < len(execution["requests"]) <= 10000,
            "bounded nonempty provider request records required")
    requests, query_requests = {}, set()
    for request in execution["requests"]:
        fields(request, {"request_id", "stage", "model_revision", "request_sha256", "response_sha256"})
        rid = text(request["request_id"], "request ID", 256)
        require(rid not in requests, "duplicate provider request ID")
        require(isinstance(request["stage"], str) and request["stage"] in {"index", "query", "retry"},
                "unknown provider request stage")
        common.hash_value(request["request_sha256"])
        if request["response_sha256"] is not None:
            common.hash_value(request["response_sha256"])
        check("observed_revision_" + rid, request["model_revision"] == selected_model["revision"])
        requests[rid] = request
        if request["stage"] != "index":
            query_requests.add(rid)

    attempts = records["attempts"]
    fields(attempts, {"schema_version", "binding", "run_id", "rows"})
    check("attempt_run", attempts["run_id"] == run_id)
    require(isinstance(attempts["rows"], list) and 0 < len(attempts["rows"]) <= 20000,
            "bounded nonempty attempts required")
    seen, paired, associated, distributions = set(), {}, set(), {"baseline": {}, "semantic": {}}
    for row in attempts["rows"]:
        fields(row, {"attempt_id", "case_id", "profile", "status", "elapsed_us", "provider_request_ids"})
        aid = text(row["attempt_id"], "attempt ID", 128)
        cid = text(row["case_id"], "attempt case", 128)
        profile = text(row["profile"], "profile", 32)
        status = text(row["status"], "attempt status", 32)
        require(aid not in seen and cid in case_set and profile in distributions and status in STATUSES,
                "duplicate or unknown attempt identity/status")
        require((profile, cid) not in paired, "one paired attempt per case/profile required")
        seen.add(aid)
        paired[(profile, cid)] = status
        if row["elapsed_us"] is not None:
            integer(row["elapsed_us"], 0, 2**64 - 1, "attempt timing")
        check("measured_attempt_" + aid, row["elapsed_us"] is not None)
        require(isinstance(row["provider_request_ids"], list) and len(row["provider_request_ids"]) <= 256,
                "invalid attempt provider request IDs")
        refs = [text(rid, "attempt provider ID", 256) for rid in row["provider_request_ids"]]
        require(len(refs) == len(set(refs)) and set(refs) <= query_requests,
                "duplicate or unknown attempt provider request")
        require(profile != "baseline" or not refs, "baseline must not claim semantic provider calls")
        if profile == "semantic" and status in SUCCESS and refs:
            check("successful_provider_response_" + aid,
                  any(requests[rid]["response_sha256"] is not None for rid in refs))
        associated.update(refs)
        bucket = distributions[profile]
        bucket[status] = bucket.get(status, 0) + 1
    check("complete_paired_attempt_inventory", set(paired) == {
        (profile, cid) for profile in distributions for cid in case_ids})
    check("query_request_inventory", associated == query_requests)
    result["attempt_distributions"] = {}
    for profile, counts in distributions.items():
        total = sum(counts.values())
        errors = sum(count for status, count in counts.items() if status not in SUCCESS)
        rate = errors / total if total else None
        result["attempt_distributions"][profile] = {"all_attempts": total, "status_counts": counts,
            "unsuccessful_attempts": errors, "error_rate": rate}
        check("error_rate_" + profile, rate is not None and rate <= thresholds["max_error_rate"])

    costs = records["costs"]
    fields(costs, {"schema_version", "binding", "currency", "items"})
    require(isinstance(costs["currency"], str) and re.fullmatch(r"[A-Z]{3}", costs["currency"]),
            "currency code required")
    require(isinstance(costs["items"], list) and len(costs["items"]) <= 10000, "bounded cost rows required")
    cost_ids, totals, unknown = set(), {"reported": Decimal(0), "estimated": Decimal(0)}, 0
    for item in costs["items"]:
        fields(item, {"request_id", "basis", "amount", "receipt_sha256"})
        rid = text(item["request_id"], "cost request ID", 256)
        require(rid in requests and rid not in cost_ids, "duplicate/unknown cost request")
        cost_ids.add(rid)
        require(isinstance(item["basis"], str) and item["basis"] in {"reported", "estimated", "unknown"},
                "unknown cost basis")
        if item["basis"] == "unknown":
            require(item["amount"] is None and item["receipt_sha256"] is None,
                    "unknown cost cannot invent amount or receipt")
            unknown += 1
        else:
            totals[item["basis"]] += money(item["amount"])
            if item["basis"] == "reported":
                common.hash_value(item["receipt_sha256"])
            else:
                require(item["receipt_sha256"] is None, "estimate cannot masquerade as a provider receipt")
    check("all_request_costs_accounted", cost_ids == set(requests))
    fully_reported = cost_ids == set(requests) and all(item["basis"] == "reported" for item in costs["items"])
    check("actual_reported_costs_required", fully_reported)
    result["cost_summary"] = {"currency": costs["currency"],
        "reported_total": str(totals["reported"]), "estimated_total": str(totals["estimated"]),
        "unknown_requests": unknown, "missing_requests": len(set(requests) - cost_ids),
        "fully_reported": fully_reported, "receipt_authenticity": "not_verified"}

    budget = records["budget"]
    fields(budget, {"schema_version", "binding", "run_id", "model", "approved", "approval_id",
                   "approval_record_sha256", "currency", "max_cost", "max_requests", "approved_at", "expires_at"})
    require(type(budget["approved"]) is bool, "explicit approval boolean required")
    text(budget["approval_id"], "approval ID", 256)
    common.hash_value(budget["approval_record_sha256"])
    integer(budget["max_requests"], 1, 10000000, "approved request cap")
    cap = money(budget["max_cost"])
    check("approved_budget_scope", budget["approved"] and budget["run_id"] == run_id
          and budget["model"] == selected_model and budget["currency"] == costs["currency"])
    check("approved_execution_window", timestamp(budget["approved_at"]) <= started <= ended <= timestamp(budget["expires_at"]))
    check("approved_request_limit", len(requests) <= budget["max_requests"])
    check("approved_cost_limit", fully_reported and totals["reported"] <= cap)
    result["budget_comparison"] = {"max_cost": str(cap), "max_requests": budget["max_requests"],
                                   "observed_request_records": len(requests), "authority": "not_verified"}

    quality = records["quality"]
    fields(quality, {"schema_version", "binding", "method", "scoring_sha256", "rows"})
    check("deterministic_gold_score_declaration", quality["method"] == "deterministic_gold_scores"
          and quality["scoring_sha256"] == pinned["scoring_sha256"])
    require(isinstance(quality["rows"], list) and 0 < len(quality["rows"]) <= 10000,
            "bounded paired quality rows required")
    quality_ids, deltas = set(), []
    for row in quality["rows"]:
        fields(row, {"case_id", "baseline", "semantic"})
        cid = text(row["case_id"], "quality case ID", 128)
        require(cid not in quality_ids and cid in case_set, "duplicate/unknown quality case")
        quality_ids.add(cid)
        baseline_score = finite(row["baseline"], 0, 1, "baseline quality score")
        semantic_score = finite(row["semantic"], 0, 1, "semantic quality score")
        deltas.append(semantic_score - baseline_score)
    check("paired_quality_inventory", quality_ids == set(case_ids))
    mean = math.fsum(deltas) / len(deltas)
    check("declared_mean_gain_threshold", mean >= thresholds["min_mean_gain"])
    result["quality_observation"] = {"paired_rows": len(deltas), "mean_delta": mean,
        "source": "recomputed_from_provided_score_rows", "scorer_execution_authenticated": False,
        "model_benefit_certified": False, "statistical_acceptance": "not_run"}
    failed = [c["name"] for c in checks if not c["passed"]]
    result["structural_status"] = "passed" if not failed else "failed"
    result["blockers"] = failed + external_blockers
    return result


def evaluate(manifest_path, output):
    return common.local_run(output, [manifest_path], lambda out: _evaluate(support.path(manifest_path), out))


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", required=True)
    parser.add_argument("--output", required=True)
    args = parser.parse_args(argv)
    try:
        result = evaluate(args.manifest, args.output)
    except (support.Invalid, OSError) as exc:
        result = {"status": "invalid_input", "exit_code": 2, "error": str(exc)[:2048],
                  "release_certified": False, "local_profile": {"status": "not_assessed", "modified": False}}
    print(json.dumps(result, sort_keys=True, allow_nan=False))
    return result["exit_code"]


if __name__ == "__main__":
    sys.exit(main())
