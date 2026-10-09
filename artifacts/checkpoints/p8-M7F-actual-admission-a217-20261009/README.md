# M7F original source-admission execution

This is an evidence-only selection for execution source `fffd950d5b34b0180f308db1428a57d5bf358bf1` (tree `7fd27832748e18cf878132fedf27571d0c20de1f`). It preserves 21 fixed small records, 1,043,301 original bytes, in a deterministic gzip/USTAR container. The manifest binds each member to its original source path, mode, size, SHA-256 and Git blob identity. All sources remain retained separately.

The original v15 command actually exited 0 on 2026-10-09, from 15:06:51.734459 to 15:14:35.352893 UTC (463.548594 seconds). The original task-plan command then exited 0. Source, review, guard inputs, actual private HEAD and index were identical before and after each invocation. Product admission covers 1095 product inputs, 71 original-BASE differences and 142 validation inputs. The projection used real immutable Git objects and the complete required files; it was not a materialized checkout of all historical archive files.

M7F changes only the exact preselect test-fixture formatting hunk reported by the original M6 CI failure. Its review commit adds four evidence files, and its final commit changes only the four original v15 identity constants and corresponding registry bindings. The original guard algorithm is unchanged. The original M6 formatting failure, subsequent skipped check steps, and loss of an earlier M6 local receipt remain historical facts.

The SQL exclusion candidate and PR188 are not included in M7F's implementation or ancestry. Their static equivalence review and the limited Python SQLite performance negatives remain separate evidence. They are not relabeled as Rust or 100k outcomes.

This bundle does not contain the source projection, historical object cache, raw measurement ZIPs, executables, private transfer responses, or later CI snapshots. A 432-byte historical-object recovery readiness summary is included only as the small metadata record already bound by the handoff. Public Git object responses are immutable repository metadata and contain no private transfer references.

No local Rust formatter, compiler, regression tests or native workload was run for this admission. The new source's normal CI remains separately observed. This evidence does not complete any TODO: 163 done, 29 remaining, 0 newly completed.

To inspect the retained original bytes, extract `M7F-source-admission-evidence.tar.gz` with a standard tar reader and compare each member with `payload-manifest.json`. `readback-receipt.json` records complete source/member readback and byte-identical second serialization. The builder is retained for reproducibility; rerunning admission or native workloads is not needed to inspect this container.
