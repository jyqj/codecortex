# P7-013: current deadline and fault evidence

**Status: progressed; normal-schedule regression failure remains open.** The fixed
source is `5af7ac0089ee7522e78ff2ce2468f881c8cf70f2`. No P7-013 production code,
original oracle, deadline, task state or acceptance condition changed in this round.
The measured primary matrix is **57 passed / 1 failed / 0 ignored**, covering 58
distinct existing functions. Commands, exact executable identities, all case names
and their results are in [p7-013-receipt.json](p7-013-receipt.json).

## Original responsibility and observed coverage

The authority is the P7-013 brief in
[TASK-BRIEFS.md](../p7-implementation-planning-20261002/TASK-BRIEFS.md), the C11/C12
contracts in [02-CONTRACTS.md](../../../docs/roadmap/code-index-v2/02-CONTRACTS.md),
and V11/V15 in [06-VALIDATION.md](../../../docs/roadmap/code-index-v2/06-VALIDATION.md).
Their exact bytes are hashed in [source-identity.json](source-identity.json).

| Original responsibility | Current source evidence | Result and limit |
| --- | --- | --- |
| Immutable total deadline; semantic child never exceeds parent | `execution::tests::semantic_lane_share_never_extends_the_total_deadline`, `async_timeout_keeps_parent_usable`, query encoder absolute-budget and publication-barrier tests | Executor 5 and query-encoding 13 pass. They preserve the total-budget model; they do not replace the public transport regression. |
| Fake slow request and real HTTP cancellation; physical work remains bounded until exit | `p7_acceptance_matrix`, `p7_production_fairness_model_review`, `protocol_cancel_keeps_http_deadline_bounded_and_later_auto_request_can_fallback` | Original fake/loopback capacity, cancellation and FIFO controls pass. Public recovery under the unchanged 150 ms semantic deadline has one failure, described below. |
| Auto keeps local results with visible insufficiency; explicit semantic cannot pretend readiness | Original public deadline fixture, acceptance matrix, query-encoding failure-classification tests | Successful traces and existing controls are retained. Full original deadline target is not certified by the isolated diagnostic. |
| Network waits outside query locks and DB leases | Original production fairness tests exercise foreground/background overlap, retained capacity/pins, close and BEGIN IMMEDIATE; original status snapshot target runs current and independent implementations | The scoped existing controls pass. No new blanket proof of every possible lock interleaving is claimed. |
| Failure is not cached as ordinary complete success; current query/config/generation domains remain distinct | `same_request_fault_recovery_and_repeated_failure_never_reuses_complete_success`; existing V11 cache, generation, ready-epoch, source-fence and consumption targets | All seven remaining integration targets pass 23 functions. Completed-result cache and query-vector cache are distinguished by the original assertions. |
| Resolve query-inline encoding authority explicitly | Current `semantic-http` build feature and enabled/network/query-network configuration gates; encoder unit tests and the shared P7-014 parameter matrix | Default network permission remains false. P7-014 matrix executions are referenced once, not counted as additional P7-013 functions. No paid-provider gate is opened. |

## Retained failure and the one diagnostic

The original normal-schedule `p7_query_deadline_public` target returned one pass and
one failure. `real_http_deadline_closes_transport_and_auto_keeps_local_search_and_context`
failed in `semantic_source` at line 261: a recovery request produced `Timeout` where
the original assertion requires `Complete`. The semantic child limit remains
150 ms and the parent remains 8 seconds. No time budget or assertion was relaxed.

The original fixture persists session traces only after a scenario's assertions.
Completed earlier scenarios have raw receipts, but the failing recovery response
and its end-to-end duration were not retained by that trace call. Semantic lane
elapsed values in other successful traces cannot supply the missing measurement.
The cause remains unresolved; scheduling/resource contention is only a hypothesis.

Exactly one same-source, same-target diagnostic used `--test-threads=1` and passed
both repeated functions. It is kept as a diagnostic, excluded from the 58 primary
functions, and does not supersede the original failure. All remaining selected
targets were executed once afterward. See [p7-013-deadline-failure.json](p7-013-deadline-failure.json).

## Completion boundary

The parent confirmed the original CI did not execute this public deadline target
and is adding a separate P7 engineering workflow. P7-013 must remain open until
the unchanged normal-schedule target and its applicable matrix actually pass at
the final integrated source and that evidence is reviewed. This checkpoint records
local author execution; it does not certify later source, full CI or real-provider
semantic quality. The independent review included here covers only the P7-014 GC
retention change.
