"""Validate opt-in, process-wide acquisition counters; never reconstruct missing waits."""
import json
from pathlib import Path
from p8_runtime_build import LOCK_OBSERVATION_PROFILE, require


ROLES = ("workload", "observer")
METRICS = ("writer_mutex_acquire", "read_pool_lock_acquire", "read_connection_checkout")
COUNTERS = ("attempts", "acquired", "poisoned", "failed", "would_block", "in_flight",
            "elapsed_ns_total", "elapsed_ns_max")
U64_MAX = (1 << 64) - 1


def unsigned(value, name):
    require(type(value) is int and 0 <= value <= U64_MAX, "invalid unsigned counter: " + name)
    return value


def validate_snapshot(value, drained=False):
    require(isinstance(value, dict) and value.get("schema_version") == 1
            and value.get("scope") == "process_lifetime_all_index_db_handles"
            and value.get("sqlite_busy_wait_observed") is False,
            "missing or wrong DB acquisition observation scope")
    for name in ("process_id", "db_instance_id", "created_instances", "update_sequence"):
        unsigned(value.get(name), name)
    require(value["process_id"] > 0 and 0 < value["db_instance_id"] <= value["created_instances"],
            "invalid observed process or database instance identity")
    require(type(value.get("coherent")) is bool and type(value.get("overflowed")) is bool,
            "missing counter completeness flags")
    require(value["coherent"] and not value["overflowed"],
            "DB acquisition snapshot is incoherent or overflowed")
    for role in ROLES:
        require(isinstance(value.get(role), dict) and set(value[role]) == set(METRICS),
                "missing acquisition role/metric")
        for name in METRICS:
            metric = value[role][name]
            require(isinstance(metric, dict) and set(metric) == set(COUNTERS),
                    "missing acquisition counters")
            for key in COUNTERS:
                unsigned(metric[key], role + "." + name + "." + key)
            require(metric["attempts"] == sum(metric[k] for k in
                    ("acquired", "poisoned", "failed", "would_block", "in_flight")),
                    "DB acquisition terminal denominator differs")
            require(metric["elapsed_ns_max"] <= metric["elapsed_ns_total"],
                    "DB acquisition maximum exceeds total")
            if drained:
                require(metric["in_flight"] == 0, "DB acquisitions were not drained at the boundary")
    return value


def summarize(before, after):
    """Exact integer differences of coherent, drained counters; no quantile estimates."""
    validate_snapshot(before, drained=True)
    validate_snapshot(after, drained=True)
    require(before["process_id"] == after["process_id"], "DB observation process changed")
    require(after["created_instances"] >= before["created_instances"]
            and after["update_sequence"] >= before["update_sequence"],
            "process-wide DB observation counters reset")
    result = {}
    for role in ROLES:
        result[role] = {}
        for name in METRICS:
            left, right = before[role][name], after[role][name]
            require(all(right[key] >= left[key] for key in COUNTERS),
                    "process-wide acquisition counter decreased")
            delta = {key: right[key] - left[key] for key in COUNTERS
                     if key not in ("in_flight", "elapsed_ns_max")}
            delta["in_flight_at_boundaries"] = [left["in_flight"], right["in_flight"]]
            # A lifetime maximum cannot be subtracted to obtain an interval max.
            delta["process_lifetime_max_ns_at_end"] = right["elapsed_ns_max"]
            delta["interval_max_ns"] = None
            result[role][name] = delta
    require(all(result["workload"][name]["attempts"] > 0 for name in METRICS),
            "mixed workload did not observe every declared acquisition seam")
    return dict(schema_version=1, status="complete_acquisition_observation",
                diagnostic_profile=LOCK_OBSERVATION_PROFILE,
                process_id=after["process_id"],
                current_instance_at_boundaries=[before["db_instance_id"], after["db_instance_id"]],
                created_instances_at_boundaries=[before["created_instances"], after["created_instances"]],
                counters=result, boundaries="after initial cold build, before offered work; after work and sampler drain, before endpoint comparison",
                scope="all IndexDb handles in the owned product process, including retired branch handles; no resets",
                observer_attribution="synchronous status handler acquisitions have a separate thread-local role; concurrent workload threads retain their role",
                writer_and_pool_timing="actual lock acquisition call elapsed, including uncontended call overhead; not pure blocked scheduler time",
                checkout_timing="read connection checkout wall time; may include validation or construction; not pure lock or SQLite busy wait",
                sqlite_busy_wait_observed=False, interval_maximum_observed=False,
                performance_comparison="instrumented feature build only; never merged into default-build latency samples",
                release_approval=False, task_complete=False)


def capture(product, raw, phase, elapsed):
    started = elapsed()
    value = product.tool("status", {"aspect": "lock_observation"}, timeout=30)
    # Retain the original response before validation, including any failure.
    raw.emit("db_lock_observation", phase=phase, started_ns=started,
             finished_ns=elapsed(), owned_product_pid=product.process.pid, response=value)
    validate_snapshot(value, drained=True)
    require(value["process_id"] == product.process.pid,
            "DB acquisition response belongs to another process")
    return value


def verify_retained(output, plan, report):
    """Replay only the retained counter boundaries, not any native workload."""
    require(plan.get("diagnostic_profile") == report.get("diagnostic_profile") == LOCK_OBSERVATION_PROFILE,
            "diagnostic report and plan profile disagree")
    require(plan.get("profile") == "mixed" and plan.get("operations") == 900
            and plan.get("files") == 1000 and plan.get("offer_interval_ms") == 500
            and plan.get("concurrency") in (1, 4, 8, 16),
            "diagnostic archive changed the original mixed workload")
    output = Path(output)
    build = json.loads((output / "build-evidence/build-receipt.json").read_text())
    require(build.get("diagnostic_profile") == LOCK_OBSERVATION_PROFILE,
            "diagnostic archive is not bound to the feature build")
    if report.get("status") != "passed_observation":
        # An incomplete observation stays an archived failure. It cannot be
        # certified by a successful inventory verification or missing rows.
        require(report.get("exit_code") in (1, 2), "failed diagnostic archive claims success exit")
        return dict(status="retained_failed_observation", observation_exit_code=report["exit_code"])
    rows, operations = [], []
    with (output / "raw.jsonl").open() as stream:
        for line in stream:
            value = json.loads(line)
            if value.get("kind") == "db_lock_observation":
                rows.append(value)
            elif value.get("kind") == "operation":
                operations.append(value)
    require([row.get("phase") for row in rows] == ["before_work", "after_work_and_sampler_drain"],
            "missing, repeated or reordered original counter boundaries")
    require(all(type(row.get(key)) is int and row[key] >= 0 for row in rows
                for key in ("started_ns", "finished_ns"))
            and rows[0]["started_ns"] <= rows[0]["finished_ns"] <= rows[1]["started_ns"] <= rows[1]["finished_ns"],
            "invalid original counter-boundary times")
    require(len(operations) == plan["operations"]
            and all(type(row.get(key)) is int and row[key] >= 0 for row in operations
                    for key in ("offered_ns", "finished_ns"))
            and rows[0]["finished_ns"] <= min(row["offered_ns"] for row in operations)
            and rows[1]["started_ns"] >= max(row["finished_ns"] for row in operations),
            "counter boundaries do not enclose every original offered operation")
    expected = summarize(rows[0]["response"], rows[1]["response"])
    product_receipt = json.loads((output / "product/process.json").read_text())
    require(type(product_receipt.get("pid")) is int and product_receipt["pid"] > 0
            and all(row.get("owned_product_pid") == row["response"]["process_id"]
                    == product_receipt["pid"] for row in rows),
            "retained acquisition response is not bound to the original owned product PID")
    require(expected == report.get("db_lock_observation")
            == json.loads((output / "db-lock-observation.json").read_text()),
            "retained acquisition summary differs from its original counters")
    require(report.get("exit_code") == 0 and report.get("owned_cleanup", {}).get("unfinished_work") == 0
            and report["owned_cleanup"].get("sampler_stopped") is True
            and report["owned_cleanup"].get("product_stopped") is True
            and report.get("artifact_seal_status") == "sealed",
            "successful acquisition report lacks owned drain and seal")
    return dict(status="original_counter_boundaries_verified", native_workload_rerun=False)
