# Reviewed chain and PR181 history — round 14

This append-only publication preserves the reviewed P/R/G/H preparation and two different PR181 observations at their actual times. Packaging only reads bytes, checks SHA/Git blob identities and reads every ZIP member for CRC; it does not rerun source validators, Cargo, CI or research.

## Time and identity boundaries

- The `pr181-retirement/` report and unposted comment draft concern head `27e60348f01d16298dd4d28f48dd1e2d47ee687c`, still open/draft in the original 2026-10-09 10:59:46 UTC recheck. They are historical planning records, not an executed closure and not a review of the later head.
- The subsequent official observation records PR181 externally **merged at 11:00:55 UTC**, head `d7426175ae21a9619cbcb393e41ec535695972d1`, merge commit/main `b9412406e11422d7cf914458a8bfbbd58cf94eaa`, base `55aa2bcf355441585bcf980e1d6f4fab8eebe59d`. Root did not post the old draft, close PR181 or perform this merge. Its source/history consequences are reviewed separately; this package does not reinterpret the old retirement recommendation as an approval of the new head.
- P is `c92eb5ac7ece70d1f62271d7e2dacac513c285b5`; R is `f89d5feec91ae611d52614b8fe217bcd7fd5cf64`; G is `b2bbd5817ce00367cea52696b098c6eafe4aa0a6`; H is `399a71a4c11e7041ce9946fba4d3f72200c47155`. The frozen root preparation snapshot records H's successful initial native preparation with 1,246 selected entries. **The original H v15 had not started and was not accepted at that snapshot.** Static G/H and forward-readback reviews do not supply its execution credit.
- The independent source audit and P's six-command native evidence (29 tests) already reside under R/staging H at `artifacts/checkpoints/p8-public-api-merge-bfcc-20261009/`; their large original containers are not copied again here. Their actual P/cached-target scope is unchanged.
- Ongoing new-main/H2 work and subsequent actual execution are outside this frozen package and will be appended separately. No artifact, source, selection or prior failed/pending result is overwritten.

## Containers

| Published container | Contents |
|---|---|
| `todo-audit-pr180-v15-and-forward-preparation.zip` | Existing 460,708-byte audit package, byte-for-byte reused. |
| `todo-audit-pr180-actual-G-and-forward-readback.zip` | Existing 364,170-byte audit package, byte-for-byte reused. |
| `selected-chain-and-PR181-history.zip` | Five R thin-review originals plus their original selection; six historical PR181 review files plus their original selection; both frozen root observations. Exactly 15 members. |

The newly packed ZIP lays out `scale-intake-primary/round14-R-thin-review/`, `pr181-retirement/` and `root-round14/`. Original selection bytes remain intact; `manifest.json` records both actual container members and earlier suggested repository paths where present. The reused ZIPs remain independent containers and were not unpacked into the new ZIP. Every upload item is at most 8 MiB; no split or reassembly is needed. `upload-index.json` gives exact local sources, repository destinations, sizes, SHA-256 and Git blob IDs.

## Original task accounting

**163 done /29 remaining; newly completed original TODOs: 0.** PR history, source review and packaging do not close tasks. Existing scoped component passes remain valid at their original identities. Complete registered same-source scale/statistics, original hard dependencies and the original P8-007 actual mixed-path DB lock acquisition wait observation remain outstanding where previously recorded. Availability probes are not relabeled as that missing observation. No additional telemetry architecture or threshold is introduced.
