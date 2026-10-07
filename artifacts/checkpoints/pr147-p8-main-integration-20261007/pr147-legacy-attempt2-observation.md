# PR #147 old-head attempt 2 — read-only final observation

The one authorized re-execution of the failed `check` job passed. This result applies only to head `753b66846a96cdb3cc541fe50f94b7ccdb05fe07`, actual checkout `248f81a19048a36daa06cec804beaab35ec02577`, tree `8b7b243b1d7ece132aa0497149535d4145882ac4`, with original PR base `b951f27d3ed50b7755bc2456c6425355f753ec17`.

Main advanced during this execution. Root is integrating the newer base; the resulting new head requires its own normal CI. This old result does not certify that new integration.

- Run: https://github.com/jyqj/codecortex/actions/runs/37665494198/attempts/2
- Actual new check job: https://github.com/jyqj/codecortex/actions/runs/37665494198/job/112947172558
- Actual new check execution: 2026-10-07T18:24:21Z to 2026-10-07T18:42:56Z.
- Every job record and step is success. The security and MSRV records were carried forward from attempt 1; they are not additional executions. Exact old/new job mapping and identical timestamps, runner IDs and steps are recorded in `carried-forward-jobs.json`.

## Actual output and source binding

The review matched all 28 printed executable command blocks to the fixed old head's workflow. No product process or Rust test was launched locally. The only local verification parsed saved Actions output and fixed Git blobs.

`tests::benchmark_fixture` actually reports `ok` in both original workflow invocations: default regression at raw line 1150 and optional HTTP at line 6295. Each surrounding cc-eval library result is `39 passed; 0 failed; 5 ignored` (lines 1152 and 6297). Success output does not print the new per-tool duration, so no new warm duration is claimed.

Three current plan outputs at raw lines 4967, 5097 and 8468 agree: 192 tasks, 153 done, 37 todo, 1 in_progress, 1 blocked, 4 views. The old candidate therefore has 39 unfinished tasks. All three bind the actual task SHA256 `88b309f8723049ab0ee1e7e9baa59efa2c658560e19729dc9cb4240f0409afe7`.

The actual v9 source guard at raw line 5088 passed with 776 complete inputs. The actual historical-v2 guard at line 5098 passed with packing 731 inputs / 246 evidence files, E3 708 inputs / 58 imported files, and all 9 preserved legacy helpers unchanged. It still reports V19 and full P7 open and does not grant content acceptance or 100k/quality results. No source observation here refers to a later 785-input integration.

The HTTP/status/stdio checker actually reports all original 17 tests verified (6 DB + 10 status + 1 stdio). The original explicit stdio success cases at lines 7193, 8288, 8383, 8449 and 8590 cover the normal product adapter, P5 B/C/D public contracts and final semantic product adapter. All later workflow gates through the final stdio case and post-job cleanup are success.

The check job uses Rust `1.99.0 (b940084d7 2026-09-28)`, runner image `ubuntu-24.04` version `20260927.320.1`, and a full-match hit of the original cache key `v0-rust-check-Linux-x64-cbcac849-3495a27c`. New GitHub runner ID `1000002706` is an execution identity, not proof of physical-host or resource independence.

## First failure and retry boundary

Attempt 1 remains a real failure: `benchmark_fixture` reported index warm p95 501.93 ms against the unchanged strict `<500 ms` assertion. The original index case has `full=true`: its measured warm calls perform full rebuilds, rather than index-cache reads. The report selects the minimum of two measured calls after one discarded warmup; it does not expose the two original durations or per-stage CPU/I/O observations. The root cause remains unproven and unfixed.

The earlier failure raw log and review remain unchanged, checked by their original SHA256 values. This single subsequent success neither removes that failure nor establishes its cause. No retry beyond the single root-authorized failed-check re-execution was invoked by this reviewer.

`attempt2-review.json` contains the checked actual JSON, raw-line anchors, full step command match observations, runner/cache evidence and scope limits. `minimal-excerpt.log` contains a numbered selection of the original timestamped log lines. The unabridged raw log remains `check-job.log`. `evidence-sha256.json` binds these files and references the unchanged first-attempt evidence. No new full per-case/target index or archive was built.
