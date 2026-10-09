"""Explicit protocol controls only; these fakes are not product cache evidence."""
import copy
import json
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import p8_runtime as runtime

ACTIONS = ("bounded_symbol_churn", "add", "rename", "delete", "real_git_branch_switch", "restore_api")

# Observed wire shape, projected without moving fields. Explicit protocol fixtures.
FIXTURE_ORIGIN = {'scope': 'Minimal field-preserving projections from retained actualG wire bodies; schema fixtures only, not a new product execution. ProtocolProduct maps only owned entity/source coordinates and live synthetic counters.', 'source_commit': 'bb9a96d71622458c39a143055360cc97f0d11d78', 'G2_product_Rust_same': 'd2d492b329a5630bf4a5fa6bc4b6eac30d07b13c', 'status': {'artifact': 11576468006, 'member': 'p8-runtime/product/rpc.jsonl', 'member_sha256': 'd387c314670769cbde5f0aaf414f53c73b03cafd0d18b99ca395dfb1e9e847ad', 'response_id': 4, 'response_canonical_sha256': 'bb14dfd0e05af850d6a7d6537683f2b7dfdada9ffe433d47240de8afce547eb7'}, 'hybrid': {'member': 'measurement/replay-input.json', 'member_sha256': '9c03093974eacb7f34f69c6f9789812be3e7d3ab976c39e2116ba52957a44302', 'sample_index': 0, 'response_canonical_sha256': 'b2f24f5f3a15f39d4af1d74ce17368cae9c6571bd2cbc21cfa1ee82821626242'}}
G_STATUS_SHAPE = {'resolution_freshness': {'basis_epoch': None, 'complete': True, 'completed_files': 0, 'index_epoch': 5, 'reason': None, 'retry': None, 'root_count': 0, 'scope': 'observed_resolution_invalidations', 'status': 'ready'}, 'diagnostics': {'process_resources': {'pid': 6643}, 'retrieval': {'generation': {'evidence_epoch': 1, 'incarnation': [5, 146, 226, 99, 157, 174, 103, 161, 85, 169, 197, 41, 192, 113, 244, 171], 'index_epoch': 5, 'semantic_epoch': None}, 'resolution_freshness': {'basis_epoch': None, 'complete': True, 'completed_files': 0, 'index_epoch': 5, 'reason': None, 'retry': None, 'root_count': 0, 'scope': 'observed_resolution_invalidations', 'status': 'ready'}}, 'search_cache': {'graph_hit_rate': 0.0, 'graph_hits': 0, 'graph_misses': 0, 'result_hit_rate': 0.0, 'result_hits': 0, 'result_misses': 0}, 'query_execution': {'async_admitted': 0, 'async_in_flight': 0, 'async_limit': 8, 'completed': 0, 'cpu_admitted': 0, 'cpu_in_flight': 0, 'cpu_limit': 4, 'queue_limit': 32, 'rejected': 0}}}
G_HYBRID_SHAPE = {'machine_pack': {'hits': [{'file_path': 'source_000.py', 'symbol_name': 'p8_lifecycle_000000', 'start_line': 1, 'end_line': 2, 'text': 'def p8_lifecycle_000000(value):\n    return value + 17', 'metadata': {'source_evidence': {'boundary': 'symbol', 'owner': {'end': 53, 'start': 0}, 'signature': {'end': 36, 'start': 0}, 'slice_digest': 'cb81271fb4d222e6255c3fb4d980f3219318a91a8e8a215c70d7d13f26066cfa', 'source': {'byte_len': 715, 'content_digest': '170165262feb9e145f3e4b25eeaa5b43af61519401a3d5972f4ed4cd135a5fc4', 'encoding': 'utf8', 'snapshot_id': '8251b1dd9c800dede9de37bdc90b2badf9c43e6ed1f7eb07ecc2df76cf6f71d2'}, 'span': {'end': 53, 'start': 0}}, 'source_freshness': {'disk_checked': True, 'hydrator': 'current-document-scope-source-generation-v1', 'status': 'current_verified'}}}]}, 'evidence_summary': {'packing': {'compacted': False, 'configured_max_bytes': 18000, 'continuation': 'Use files region/expand on retained references; recheck source version.', 'limit_bytes': 16000, 'omitted_hits': 0, 'omitted_nodes': 0, 'original_hits': 1, 'original_nodes': 1, 'partial': False, 'spec': 'whole-json-intent-facets-source-support-before-incidental-v5', 'token_estimate_method': 'ceil(serialized_utf8_bytes/4); not a tokenizer', 'used_bytes': 14120}}}


class ProtocolProduct:
    """Known state machine with explicit mutation/timeout/corruption injection."""
    def __init__(self, project):
        self.project = project
        self.epoch = 5
        self.pid = 1001
        self.process = SimpleNamespace(pid=self.pid)
        self.completed = 0
        self.rejected = 0
        self.counts = dict.fromkeys(runtime.CACHE_COUNTERS, 0)
        self.filled = set()
        self.calls = []
        self.fail_role = None
        self.bad_hit = None
        self.force_lookup = None

    def status(self):
        state = copy.deepcopy(G_STATUS_SHAPE)
        diagnostics = state["diagnostics"]
        diagnostics["process_resources"]["pid"] = self.pid
        diagnostics["retrieval"]["generation"]["index_epoch"] = self.epoch
        state["resolution_freshness"]["index_epoch"] = self.epoch
        diagnostics["retrieval"]["resolution_freshness"]["index_epoch"] = self.epoch
        diagnostics["search_cache"].update(self.counts)
        diagnostics["query_execution"].update(completed=self.completed, rejected=self.rejected)
        return state

    def hit(self):
        # Explicit test-only mapping from an observed lifecycle entity to this
        # owned tiny fixture; actual wire paths and metadata layout stay intact.
        response = copy.deepcopy(G_HYBRID_SHAPE)
        hit = response["machine_pack"]["hits"][0]
        source = (self.project/'stable.py').read_bytes()
        hit.update(file_path="stable.py", symbol_name=runtime.QUERY, start_line=1, end_line=2,
                   text=source[:-1].decode())
        proof = hit["metadata"]["source_evidence"]
        proof["span"] = dict(start=0, end=len(source)-1)
        proof["source"]["byte_len"] = len(source)
        if self.bad_hit: self.bad_hit(hit)
        return response

    def tool(self, name, arguments, timeout=45):
        if name == "search" and arguments["mode"] == "hybrid":
            # SearchParams denies unknown fields; the actual public field is
            # retrieval_strategy, not the informal plan description "strategy".
            assert set(arguments) == {"query", "mode", "top_k", "retrieval_strategy"}
            assert arguments["retrieval_strategy"] == "local"
        role = ("before_status" if len(self.calls) % 4 == 0 else "after_status") if name == "status" else arguments["mode"]
        self.calls.append(dict(role=role, name=name, arguments=copy.deepcopy(arguments), timeout=timeout))
        if role == self.fail_role: raise TimeoutError("explicit protocol timeout " + role)
        if name == "status": return self.status()
        self.completed += 1
        if arguments["mode"] == "symbol": return [dict(name=runtime.QUERY, file_path="stable.py")]
        lookup = self.force_lookup or ("hit" if self.epoch in self.filled else "miss")
        self.counts["graph_hits" if lookup == "hit" else "graph_misses"] += 1
        self.filled.add(self.epoch)
        return self.hit()


class SoakCacheProtocolTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="cache-protocol-only-")
        self.addCleanup(self.temp.cleanup)
        self.project = Path(self.temp.name)
        (self.project/'.p8-owned').write_text(runtime.MARKER)
        (self.project/'stable.py').write_text(f"def {runtime.QUERY}():\n    return 7\n")
        self.product = ProtocolProduct(self.project)

    def read(self, previous=None, number=1, mutation=None):
        at = number*1_000_000_000
        row = dict(id=number, operation="read", status="error", offered_ns=at,
                   scheduled_ns=at, started_ns=at, call_started_ns=at+1)
        clock = iter(range(at+2, at+200)).__next__
        try:
            result = runtime.soak_read(self.product, self.project, row, previous, mutation, clock)
            row["status"] = "success"
        except Exception as error:
            row["error"] = str(error)
            result = None
        row["finished_ns"] = clock()
        return row, result

    def successful_probe(self):
        row, observed = self.read()
        self.assertIsNotNone(observed, row)
        return row["cache_probe"]

    def test_observed_wire_status_has_outer_and_nested_matching_freshness(self):
        # Direct retained-body projection; no invented diagnostics-root field.
        self.assertNotIn("resolution_freshness", G_STATUS_SHAPE["diagnostics"])
        identity = runtime.cache_identity(copy.deepcopy(G_STATUS_SHAPE))
        self.assertEqual(identity["pid"], G_STATUS_SHAPE["diagnostics"]["process_resources"]["pid"])
        self.assertEqual(identity["generation"], G_STATUS_SHAPE["diagnostics"]["retrieval"]["generation"])
        for change in (lambda s:s.pop("resolution_freshness"),
                       lambda s:s["diagnostics"]["retrieval"].pop("resolution_freshness"),
                       lambda s:s["resolution_freshness"].update(index_epoch=999),
                       lambda s:s["diagnostics"]["retrieval"]["resolution_freshness"].update(complete=False)):
            state = copy.deepcopy(G_STATUS_SHAPE);change(state)
            with self.assertRaises(ValueError): runtime.cache_identity(state)

    def test_real_generation_rule_includes_noop_hit_and_refill_then_hit(self):
        first, a = self.read(); self.assertEqual(a["lookup"]["state"], "miss")
        second, b = self.read(a, 2); self.assertEqual(b["lookup"]["state"], "hit")
        self.product.epoch += 1
        third, c = self.read(b, 4); self.assertEqual(c["lookup"]["state"], "miss")
        self.assertTrue(c["invalidated"])
        _, d = self.read(c, 5); self.assertEqual(d["lookup"]["state"], "hit")
        _, e = self.read(d, 7); self.assertEqual(e["lookup"]["state"], "hit")
        self.assertFalse(e["invalidated"])
        self.assertEqual([x["role"] for x in first["cache_probe"]["requests"]], list(runtime.SOAK_READ_ROLES))
        self.assertEqual(first["response"], first["cache_probe"]["requests"][1]["response"])
        self.assertEqual(len(self.product.calls), 20)
        self.assertGreater(first["finished_ns"], first["cache_probe"]["requests"][-1]["finished_ns"])

    def test_zero_ambiguous_reset_missing_and_boolean_counters_rejected(self):
        before = self.product.status()
        variations = [dict(graph_hits=0, graph_misses=0), dict(graph_hits=1, graph_misses=1),
                      dict(graph_hits=2, graph_misses=0), dict(graph_hits=True),
                      dict(graph_hits=-1), dict(result_misses=1, graph_hits=1)]
        for changes in variations:
            with self.subTest(changes=changes):
                after = copy.deepcopy(before);after["diagnostics"]["search_cache"].update(changes)
                with self.assertRaises(ValueError): runtime.cache_lookup(before, after)
        after = copy.deepcopy(before);del after["diagnostics"]["search_cache"]["graph_hits"]
        with self.assertRaises(ValueError): runtime.cache_lookup(before, after)

    def test_cross_generation_or_server_cannot_be_cache_receipt(self):
        original = self.successful_probe()
        for key in ("pid", "epoch", "incarnation", "incomplete"):
            with self.subTest(key=key):
                probe = copy.deepcopy(original);after = probe["requests"][-1]["response"]["diagnostics"]
                if key == "pid": after["process_resources"]["pid"] += 1
                elif key == "epoch":
                    after["retrieval"]["generation"]["index_epoch"] += 1
                    after["retrieval"]["resolution_freshness"]["index_epoch"] += 1
                elif key == "incarnation": after["retrieval"]["generation"]["incarnation"][0] += 1
                else: after["retrieval"]["resolution_freshness"]["complete"] = False
                with self.assertRaises(ValueError): runtime.validate_cache_probe(probe, None, self.project)

    def test_wrong_hit_after_invalidation_and_wrong_miss_same_generation_fail(self):
        _, previous = self.read()
        self.product.force_lookup = "miss"
        row, bad = self.read(previous, 2)
        self.assertIsNone(bad);self.assertEqual(row["status"], "error")
        self.product.epoch += 1;self.product.force_lookup = "hit"
        row, bad = self.read(previous, 4)
        self.assertIsNone(bad);self.assertIn("previous accepted generation", row["error"])

    def test_cache_hit_cannot_hide_stale_source_or_wrong_entity(self):
        bad_hits = [lambda h:h.update(file_path="wrong.py"), lambda h:h.update(symbol_name="query echo"),
                    lambda h:h.update(text=f"def {runtime.QUERY}():\n    return 6"),
                    lambda h:h.update(start_line=2),
                    lambda h:h["metadata"]["source_freshness"].update(status="stale"),
                    lambda h:h["metadata"]["source_freshness"].update(disk_checked=False),
                    lambda h:h["metadata"]["source_evidence"]["span"].update(start=1),
                    lambda h:h["metadata"]["source_evidence"]["source"].update(byte_len=1)]
        for bad in bad_hits:
            with self.subTest(bad=bad):
                self.product = ProtocolProduct(self.project)
                _, previous = self.read();self.product.bad_hit = bad
                row, result = self.read(previous, 2)
                self.assertIsNone(result);self.assertEqual(row["status"], "error")
        with self.assertRaises(ValueError): runtime.require_stable_hybrid({"query":runtime.QUERY}, self.project)
        response = self.product.hit();response["evidence_summary"]["packing"]["partial"] = True
        with self.assertRaises(ValueError): runtime.require_stable_hybrid(response, self.project)

    def test_missing_original_call_or_changed_request_is_rejected(self):
        original = self.successful_probe()
        for index in range(4):
            probe = copy.deepcopy(original);probe["requests"].pop(index)
            with self.assertRaises(ValueError): runtime.validate_cache_probe(probe, None, self.project)
        for change in (lambda p:p["requests"][2]["arguments"].update(query="other"),
                       lambda p:p["requests"][2]["arguments"].update(retrieval_strategy="semantic"),
                       lambda p:p["requests"][2].update(started_ns=0),
                       lambda p:p["requests"][0].pop("response")):
            probe = copy.deepcopy(original);change(probe)
            with self.assertRaises(ValueError): runtime.validate_cache_probe(probe, None, self.project)

    def test_shared_pool_counters_must_progress_without_reset_or_rejection(self):
        original = self.successful_probe()
        for change in (dict(completed=0), dict(completed=True), dict(rejected=1), dict(cpu_limit=5)):
            probe = copy.deepcopy(original)
            probe["requests"][-1]["response"]["diagnostics"]["query_execution"].update(change)
            with self.assertRaises(ValueError): runtime.validate_cache_probe(probe, None, self.project)
        self.product.pid += 1
        row, value = self.read(original["observation"], 2)
        self.assertIsNone(value);self.assertEqual(row["status"], "error")

    def test_each_failed_request_prefix_keeps_actual_counts_no_invented_attempts(self):
        for index, role in enumerate(runtime.SOAK_READ_ROLES):
            with self.subTest(role=role):
                self.product = ProtocolProduct(self.project);self.product.fail_role = role
                row, result = self.read()
                self.assertIsNone(result);self.assertEqual(row["status"], "error")
                calls = row["cache_probe"]["requests"]
                self.assertEqual(len(calls), index+1)
                self.assertEqual(calls[-1]["status"], "error")
                self.assertIn("explicit protocol timeout", calls[-1]["error"])
                self.assertGreaterEqual(calls[-1]["finished_ns"], calls[-1]["started_ns"])
                summary = runtime.soak_cache_summary([row], 3, 0, 3_000_000_000, self.project)
                self.assertFalse(summary["passed"])
                self.assertEqual(summary["recorded_offered_reads"], 1)
                self.assertEqual(summary["expected_offered_reads"], 2)
                self.assertEqual(sum(sum(v.values()) for v in summary["request_counts"].values()), index+1)

    def complete_rows(self):
        self.product = ProtocolProduct(self.project)
        rows=[];previous=None;mutation=None
        for number in range(72):
            at = number*1_000_000_000
            if number % 3 == 0:
                ordinal=number//3;action=ACTIONS[ordinal % 6]
                if action != "restore_api": self.product.epoch += 1
                mutation=dict(action=action, ordinal=ordinal, operation_id=number, index_epoch=self.product.epoch)
                rows.append(dict(id=number, operation="build", status="success", offered_ns=at,
                                 call_started_ns=at+1, finished_ns=at+10, mutation_ordinal=ordinal,
                                 mutation=dict(action=action), response=dict(resolution_freshness=dict(index_epoch=self.product.epoch))))
            else:
                row, previous = self.read(previous, number, mutation)
                self.assertIsNotNone(previous, row);rows.append(row)
        return rows

    def test_summary_keeps_original_operation_N_and_explicit_rpc_totals_and_quarters(self):
        rows = self.complete_rows()
        result = runtime.soak_cache_summary(rows, 72, 0, 72_000_000_000, self.project)
        self.assertTrue(result["passed"], result)
        self.assertEqual(result["recorded_offered_reads"], 48)
        self.assertEqual(result["request_counts"], {role:{"success":48} for role in runtime.SOAK_READ_ROLES})
        self.assertEqual(result["status_request_counts"], {"success":96})
        self.assertEqual(result["validated_reads"], 48)
        self.assertEqual(len(rows), 72)
        for q in result["time_quarters"]:
            self.assertEqual(q["reads"], 12)
            self.assertGreater(q["hits"], 0);self.assertGreater(q["misses"], 0)
            self.assertEqual(set(q["mutations"]), set(ACTIONS))

    def test_summary_rejects_missing_raw_changed_receipt_or_preceding_mutation(self):
        original = self.complete_rows()
        for mutate in (lambda rows:rows.pop(1),
                       lambda rows:rows[1].pop("cache_probe"),
                       lambda rows:rows[1]["cache_probe"]["observation"].update(expected="hit"),
                       lambda rows:rows[1]["cache_probe"]["preceding_mutation"].update(operation_id=99),
                       lambda rows:rows[1].update(finished_ns=rows[1]["call_started_ns"]),
                       lambda rows:rows[1].update(response=[{"query":runtime.QUERY}])):
            rows = copy.deepcopy(original);mutate(rows)
            result=runtime.soak_cache_summary(rows, 72, 0, 72_000_000_000, self.project)
            self.assertFalse(result["passed"])

    def test_clustered_cache_receipts_cannot_certify_temporal_reuse(self):
        rows=self.complete_rows()
        result=runtime.soak_cache_summary(rows, 72, 0, 3600_000_000_000, self.project)
        self.assertFalse(result["passed"])
        self.assertFalse(result["temporal_cache_coverage"])

    def test_spread_offers_with_clustered_actual_calls_do_not_cover_four_quarters(self):
        rows = self.complete_rows()
        # Keep all72 original offered times spread across the interval, but
        # queue all actual operations until its final100us. Preserve ordering,
        # request durations, generation transitions and source responses.
        for row in rows:
            old_base = row["id"] * 1_000_000_000
            actual_base = 72_000_000_000 - 100_000 + row["id"] * 1000
            delta = actual_base - old_base
            row["call_started_ns"] += delta
            row["finished_ns"] += delta
            if "cache_probe" in row:
                row["started_ns"] += delta
                for call in row["cache_probe"]["requests"]:
                    call["started_ns"] += delta;call["finished_ns"] += delta
        result = runtime.soak_cache_summary(rows, 72, 0, 72_000_000_000, self.project)
        self.assertFalse(result["passed"])
        self.assertEqual(result["validated_reads"], 48)
        self.assertEqual([q["reads"] for q in result["time_quarters"]], [0,0,0,48])
        self.assertEqual([sum(q["mutations"].values()) for q in result["time_quarters"]], [0,0,0,24])


class CacheTerminalRetentionTests(unittest.TestCase):
    def test_exhausted_raw_sidecar_keeps_metadata_without_nested_response_bodies(self):
        body = "raw-body-budget-witness-" * 4096
        calls = [dict(role=role, name="status" if "status" in role else "search",
                      arguments={"owned_control": role}, started_ns=10 * i, finished_ns=10 * i + 1,
                      status="success", response={"body": body})
                 for i, role in enumerate(runtime.SOAK_READ_ROLES)]
        probe = dict(protocol=runtime.SOAK_READ_PROTOCOL, server_pid=1001,
                     preceding_mutation={"action": "rename", "operation_id": 3},
                     observation={"lookup": {"state": "miss"}}, requests=calls)
        row = dict(id=4, operation="read", status="success", offered_ns=0, finished_ns=40,
                   response={"body": body}, cache_probe=probe)
        original = copy.deepcopy(row)
        retained = runtime.terminal_operation_metadata(row)
        self.assertEqual(row, original, "normal raw and in-memory original must not be projected")
        self.assertNotIn("response", retained)
        self.assertNotIn(body, json.dumps(retained))
        self.assertLess(len(json.dumps(retained)), 4096)
        kept = retained["cache_probe"]
        self.assertIs(kept["raw_responses_retained"], False)
        self.assertIn("not replayable cache evidence", kept["scope"])
        for key in ("protocol", "server_pid", "preceding_mutation", "observation"):
            self.assertEqual(kept[key], probe[key])
        for before, after in zip(calls, kept["requests"]):
            self.assertEqual({key: value for key, value in after.items() if key != "response_omitted"},
                             {key: value for key, value in before.items() if key != "response"})
            self.assertIs(after["response_omitted"], True)

    def test_failed_request_prefix_and_non_cache_terminal_metadata_are_preserved(self):
        calls = [dict(role="before_status", name="status", arguments={"aspect": "index"},
                      started_ns=1, finished_ns=2, status="success", response={"retained_in_raw": False}),
                 dict(role="symbol", name="search", arguments={"query": runtime.QUERY},
                      started_ns=3, finished_ns=4, status="error", error="controlled deadline")]
        row = dict(id=1, operation="read", status="error", error="controlled deadline",
                   cache_probe=dict(protocol=runtime.SOAK_READ_PROTOCOL, server_pid=1001, requests=calls))
        retained = runtime.terminal_operation_metadata(row)
        self.assertEqual(len(retained["cache_probe"]["requests"]), 2)
        self.assertEqual([c["response_omitted"] for c in retained["cache_probe"]["requests"]], [True, False])
        self.assertEqual(retained["cache_probe"]["requests"][1]["error"], "controlled deadline")
        plain = dict(id=3, operation="build", status="success", response={"large": "body"}, finished_ns=8)
        self.assertEqual(runtime.terminal_operation_metadata(plain),
                         dict(id=3, operation="build", status="success", finished_ns=8))
        self.assertIn("response", plain)


if __name__ == "__main__": unittest.main()
