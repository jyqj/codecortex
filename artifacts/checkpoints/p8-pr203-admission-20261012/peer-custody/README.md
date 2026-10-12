# Independent review of checkpoint 19 transport

Reviewer: `/root/acceptance_audit`. Candidate source:
`0692ed27c4f86a254484e76155fe07a55ab2e09c`.

## Decision

The exact scoped workflow change is acceptable, pending the separate source
admission record and real GitHub CI. It deletes the remote upload for checkpoint
19 and removes the final step's acknowledgement of that nonexistent upload.
Checkpoint 19 still freezes local bytes and acknowledges successful upload 18.
No original TODO is closed by this review.

The workflow's declared 45-cell attempt can create at most 992 artifacts:
45 cells × (one capacity + 19 progress + one final + one complete-shard artifact),
plus one build artifact and one aggregate artifact. Additional artifacts and
rerun attempts must be counted separately; 992 is not a bound for arbitrarily
many attempts on the same run.

## Executed evidence

`checkpoint19_fixture.py` verifies the exact fetched source hashes before and
after importing the original unchanged capture functions. It parses both
workflow versions and checks that their only structural differences are removal
of `upload_19` and removal of the final step's `PREVIOUS_ARTIFACT_*` environment.
The fixture supplies explicitly synthetic byte streams and artifact tokens.
It does not launch Rust, native binaries, a measurement, a provider call, a
GitHub workflow, or any original source-admission validator.

Actual command: `python3 -B checkpoint19_fixture.py`, run from this directory's
absolute path. Recorded execution on 2026-10-12 from 07:01:51.183038 UTC to
07:01:51.389005 UTC returned exit code 0. `fixture-execution.json` preserves the
full argv and stdout/stderr hashes.

| Case | Last confirmed checkpoint | Final pending chunks | Raw-only verdict |
| --- | ---: | --- | ---: |
| Upload 18 succeeded; upload 19 skipped | 18 | 19, 20 | 1 (reject) |
| Upload 18 failed; upload 19 skipped | 17 | 18, 19, 20 | 1 (reject) |

In both cases, combining prior confirmed chunks with the final bundle recovers
the exact 667-byte synthetic stream. Neither case invents an acknowledgement for
checkpoint 19 or 20. The manifest continues to state that native EOF is unclaimed
and any bytes after the confirmed remote boundary are unknown, not zero.

This proves the local carry-forward behavior of the unchanged capture code. It
does not prove that a real final upload succeeds. If the runner or final upload
fails, the confirmed remote boundary remains 18, or 17 when upload 18 failed.
Skipping upload 19 therefore retains a larger volatile tail until the final
upload succeeds. No incomplete prefix becomes a complete native measurement.

## Evidence files

- `source-custody.json`: immutable source paths, Git blob IDs, SHA-256, byte counts.
- `independent-workflow-custody-review.json`: decision, boundaries and case results.
- `checkpoint19_fixture.py`: bounded executable fixture and structural comparison.
- `fixture-execution.json`, `fixture.stdout`, `fixture.stderr`: actual execution.
- `fixtures/`: original synthetic streams, snapshot manifests, chunks and results.

This review covers transport semantics. The separate full product/validation
inventory review, subsequent source admission, required CI and PR management
belong to the root workflow and are not claimed complete here.
