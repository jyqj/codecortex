# PR #144: completed legacy CI, fixed source evidence

The actual [legacy CI run 37657882021](https://github.com/jyqj/codecortex/actions/runs/37657882021)
completed successfully on all three jobs: `check` 112917553425, `msrv`
112917553501 and `security` 112917552966. This checkpoint records the connector
responses and decoded job logs obtained independently by `/root/ci_history_fix`.
It does not substitute an earlier run or the separate P7 Engineering workflow.

The run's source head is `eb7cdc55aa94c8d6865bed14fa37fff08080af33`.
All three checkout logs name the actual PR merge commit
`389bcf601b6c25dd3a299a94a87f1c71c2481351`. Its GitHub Git-object tree is
`25558c46010e085d13f134f6ff623e7ce285cbd3`, identical to the source head tree.
The reconstructed fixed Git inventory contains 776 crate/Cargo inputs, with
map SHA256 `baf2bd238706a0fd88b91c80357d1780415afd5b917ef56d4bf283f3932e8ee6`.
This inventory is explicitly distinguished from a downloaded runner manifest.

## What actually executed

| Check | Actual result |
|---|---|
| Format, Clippy, compile every default test target | All steps succeeded |
| Default regression in the existing authorized scope | 183 target executions; 2,574 passed, zero failed, 65 ignored |
| Resource harness and source integrity | 46 and 75 Python tests passed |
| Reviewed source v9 | All 776 inputs admitted |
| New historical entry | Passed: 731 packing inputs, 246 evidence files, 708 E3 inputs, 58 imports, nine unchanged old helpers, 192 task definitions and four views |
| Subsequent semantic HTTP capability and normal stdio query fence | 17 tests passed: six capability-read, ten status, one ready-epoch fixture |
| Later P0–P5 regressions and explicit product stdio commands | All original workflow commands executed successfully; exact targets and cases retained |
| P5-B / P5-C / P5-D steps | 17 / 32 / 12 passed; their ignored cases remain recorded |
| Default and semantic product disabled-stdio contracts | Both named contract cases passed with their explicit product builds |
| MSRV | Both existing Rust 1.95 checks succeeded |
| Dependency audit | Printed advisory-database revision found zero vulnerabilities and no warnings |

Across this job there were **3,039 passing Rust test executions, zero failures
and 126 ignored executions**, plus **144 passing Python test executions**.
Repeated invocations remain repeated; these totals are not counts of unique
tests. The index retains 371 complete target executions and three list-only
headers. The separate 58-case P7 deadline matrix is not added to these totals.

The actual legacy `check` job printed **rustc 1.99.0
(b940084d7 2026-09-28)**; the separate MSRV job printed **rustc 1.95.0
(59807616e 2026-04-14)**. Their full version rows are preserved in
`compiler-observations.json`. The separate P7 Engineering job is not the source
of either product digest below.

The default product digest printed by the builder is
`866ac758cfbaa999384f8634aa4c07697fb6e95a800402fcc0bfd8b3a67dcc80`;
the semantic product digest is
`b7717ddf7af596cfc6cc11dd4a20eecf57d2994dda8d4c880a66bb6269110e33`.
Both builder summaries name checkout `389bcf6`, 776 inputs and the correct
feature set. The workflow uploaded zero artifacts, so complete build receipts,
binaries and per-case observation JSON files could not be downloaded.

## Scope and retained failures

The fixed workflow does not set `P7_017_NETWORK_POLICY`. These successful
normal stdio runs are **not isolated network-denial gates** and do not replace
the two preserved local isolated failures. No claim of zero socket attempts,
complete P7-017, G7, live authorization or release acceptance follows.

The run's real task snapshot remains **152 done / 40 unfinished**, with P7-013
in progress. The formerly failed legacy CI sequence has now passed; the root
reviewer owns task acceptance together with the separate current P7 regression
evidence. The original failed run 37653732411 and local deadline/isolation
failures remain intact in their earlier checkpoints.

## Evidence and verification

`review.json` records the independent conclusion and limits.
`test-summary.json` maps each of the 28 original command blocks to its API step
and results. `p7-013-related-regressions.json` retains the original 87 selected
default-regression rows plus the newly executed HTTP and P5-B/C/D portions.
`stdio-execution-scope.json` separates normal product contracts from isolation.

The eight original log/API payload files are bounded in
`raw-observations.tar.gz`, with exact member sizes and SHA256 values in
`raw-members.json`. The complete derived case index is compressed separately
as `test-index.json.gz`. Job-log wrapper JSON is not duplicated inside the
archive. API content strings and decoded UTF-8 logs are preserved; pre-decode
HTTP bytes and response headers were not exposed by the connector.

Cargo stderr can print the next target before the previous target's buffered
stdout is drained. The audit pairs ordered target headers with complete
`running N tests` / result blocks, checks every named case count, and excludes
explicit `--list` invocations. It also handles a case name and terminal `ok`
split by tracing output. API timestamps alone are not used to separate steps
which began within the same second.

From a checkout containing the fixed source objects, verify with:

```sh
python3 -B artifacts/checkpoints/pr144-legacy-ci-passed-20261007/verify_storage.py --repo .
```

This re-reads evidence and fixed Git objects. It does not run a product, test
suite or benchmark and does not modify task state.
