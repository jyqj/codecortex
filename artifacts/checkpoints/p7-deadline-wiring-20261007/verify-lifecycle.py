"""Re-read current raw stdio/HTTP against the unchanged original 13-state map."""
import hashlib
import json
from pathlib import Path

OUT = Path(__file__).resolve().parent
ROOT = Path("/workspace/scratch/71bc431c4e4f/codecortex-p7-012")


def read(path):
    return json.loads(path.read_text())


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


inputs = read(OUT / "lifecycle-inputs.json")
receipt = read(OUT / "wiring-lifecycle-receipt.json")
for relative, expected in inputs["original_inputs"].items():
    assert sha(ROOT / relative) == expected, relative
for relative, expected in receipt["raw_artifacts"].items():
    assert sha(OUT / relative) == expected["sha256"], relative
assert receipt["exit_code"] == 0 and receipt["source_unchanged"]
assert receipt["extra_literal_inputs_unchanged"]
raw = OUT / "raw/wiring-lifecycle/run"
summary = read(raw / "summary.json")
rpc = read(raw / "stdio-raw.json")
http = read(raw / "http-raw.json")
oracle = read(ROOT / "artifacts/checkpoints/p7-gate-lifecycle-independent-20261003/gate-review-map.json")
assert summary["passed"] is True
assert summary["binary_sha256"] == inputs["binary"]["sha256"]
assert summary["binary_sha256"] == receipt["explicit_lifecycle_binary"]["sha256"]
assert len(summary["cases"]) == len(oracle["lifecycle_rows"]) == 13
for expected, observed in zip(oracle["lifecycle_rows"], summary["cases"], strict=True):
    assert observed["case"] == expected["case"]
    assert observed["status"]["semantic_state"] == expected["expected_semantic_state"]
    assert observed["status"]["dense_state"] == expected["expected_dense_state"]
    assert observed["delta"]["document_posts"] == expected["expected_document_posts_delta"]
    assert observed["delta"]["query_posts"] == expected["expected_query_posts_delta"]

latest_status = {}
checkpoints = []
tool_lists = 0
positive_semantic_responses = 0
errors = []
for row in rpc:
    if row["kind"] == "rpc":
        request, response = row["request"], row["response"]
        if "error" in response:
            errors.append(response["error"])
            continue
        if request["method"] == "tools/list":
            assert len(response["result"]["tools"]) == 14
            tool_lists += 1
        if request["method"] == "tools/call":
            structured = response["result"]["structuredContent"]
            value = structured.get("result", structured)
            if request["params"]["name"] == "status":
                latest_status[row["session"]] = value["retrieval"]
            elif request["params"]["name"] in ("search", "context"):
                retrieval = value.get("evidence_summary", {}).get("retrieval", {})
                lanes = retrieval.get("lanes", retrieval.get("lane_receipts", []))
                semantic = next((lane for lane in lanes if lane["lane_id"] == "semantic"), None)
                if semantic and semantic["status"] == "complete" and semantic["candidate_count"] > 0:
                    hits = value["machine_pack"]["hits"]
                    assert hits and all(hit["file_path"] == "one.rs" for hit in hits)
                    assert all(hit["text"] and hit["text"] in "pub fn needle() -> u32 { 947 }\n" for hit in hits)
                    positive_semantic_responses += 1
    elif row["kind"] == "checkpoint":
        assert row["status"] == latest_status[row["session"]]
        checkpoints.append({key: value for key, value in row.items() if key not in ("sequence", "kind")})
assert checkpoints == summary["cases"]
assert len(errors) == 4
assert all(error["code"] == -32603 and error["message"] == "semantic recall is not configured" for error in errors)
assert len(http) == 8
for row in http:
    assert row["only_loopback"] and row["dummy_authorization_matched"]
    query = all(text.startswith("lifecycle_query_") for text in row["body"]["input"])
    assert row["query_posts"] == int(query) and row["document_posts"] == int(not query)
    assert row["query_inputs"] + row["document_inputs"] == len(row["body"]["input"])
costs = {key: sum(row[key] for row in http) for key in summary["costs"]}
assert costs == summary["costs"]
assert costs["document_posts"] == costs["query_posts"] == 4
assert sum(row["response_status"] == 500 for row in http) == 3
assert summary["paid_cost"] is None and summary["synthetic_reported_tokens_are_not_real_cost"]
report = {
    "schema_version": 1,
    "source": receipt["source_commit"],
    "verdict": "passed_current_raw_rederivation",
    "original_oracle_unchanged": True,
    "actual_lifecycle_runs": 1,
    "formal_state_checkpoints": len(checkpoints),
    "tools_list_responses_with_fourteen_tools": tool_lists,
    "positive_semantic_response_executions": positive_semantic_responses,
    "stdio_records": len(rpc),
    "http_records": len(http),
    "actual_cost_counts": costs,
    "paid_currency": None,
    "overlap_note": "State checkpoints, repeated public responses, HTTP attempts and Rust test functions are separate counts; none is a new quality-question total.",
    "binary_sha256_before_and_after": summary["binary_sha256"],
    "rust_or_product_rerun_by_this_verifier": False,
    "full_P7_014_completion": False,
}
(OUT / "lifecycle-raw-verification.json").write_text(json.dumps(report, indent=2) + "\n")
print(json.dumps(report))
