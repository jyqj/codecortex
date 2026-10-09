# Original G2 platform offline reception

This separate receiver reads nine already-produced original platform artifacts from
run `37877611405`, attempt 1, at immutable source
`0272a1fb152fd76a7cfb22386a580629d4038a64` (tree
`85e9f6fb68d7b4ef79e52e23f1ade2e70abca383`). It does not run a new build,
product, provider, benchmark, or primary study. The selected artifacts comprise
eight distinct Linux/macOS × 1.95/stable × default/semantic cells and the original
complete-matrix receipt. Their nine exact ZIP IDs, sizes and SHA256 digests are
in `registration.json`; the original run has ten jobs, including recovery, whose
mere presence in recorded metadata is not recovery acceptance.

The proposed auxiliary branch is
`task/p8-g2-platform-reception-20261009`, with G2 as its sole parent. Only this
new workflow and this new artifact directory are added. Product sources, original
workflows, source approvals, guard pins, original runs and prior evidence remain
unchanged. This auxiliary controller is not a new product candidate or an
extension of the canonical product validation approval.

## Exact original source and predicates

The controller checks out its actual event commit under `receiver/` and the
fixed original G2 source under `source/`. The actual cold source identity must
contain 1089 committed inputs and reproduce list-manifest SHA256
`edb6b71a4f3190a3482bd9b2ac549c0073957d26a612e780320b54c383fc9e7a`.
That cold list format is different from the p7 map digest
`691ae804ec606969e70180298407fcacc60b0408ea025eece02c7639b50b94ac`.
The four actual cold observer modules must reproduce manifest
`f66cd4a5401eab4b5fc2283adf38775a2a7edb7347654a7aece6ce5a4c8b0935`.
They are not the runtime driver's nine observers.

The receiver invokes the unchanged G2 `p8_cold_build.py --collect-cells` CLI
with real files and paths. All original portable Cargo/compiler/source/binary,
observer, fresh-target and actual stdio-smoke predicates remain in that consumer.
The resulting complete matrix must equal the downloaded original matrix except
for the relocated checkout's `source_root`, with exactly eight passed cells,
zero failed and zero not_run. Source and loaded observers are checked again at
the end. No old G4 matrix or report is substituted for G2 evidence.

## Two-phase transport and capacity

Before reception, the nine original ZIPs total exactly 68,772,411 bytes. Their
member counts and complete expanded sizes have not been received locally and
are deliberately not guessed or copied from G4.

1. Pin original run/job/artifact metadata and whole-ZIP SHA256. Download each
   fixed ZIP with the inherited transfer bounds. Read every member to EOF,
   checking ZIP CRC, actual bytes and SHA256, while writing no expanded members.
   Preserve the observed per-member inventories and total expanded sizes.
2. With all compressed originals already on disk, require free space for the
   complete observed expansion plus the original 64 MiB output allowance and
   512 MiB filesystem reserve. Only then create new exclusive expansion
   directories, check every expanded byte against the prior complete scan, and
   run the original collector.

The original G4 controller had no separate 1 GiB reception cap, and this draft
does not claim one. Its existing transfer maxima, fixed selected ZIP sizes,
64 MiB allowance, 512 MiB reserve and strict original acceptance predicates are
preserved. Rejection keeps original/partial files and capacity observations;
there is no deletion, overwrite, sampling or selective member omission.

## Execution and evidence boundaries

The inherited wrapper remains first-attempt-only on Ubuntu 24.04, at most
40 minutes, with contents/actions read permissions and an always-upload step
covering only this receiver's output, including hidden evidence. Hard
termination cannot guarantee upload. The token-bearing transfer stage never
prints tokens or redirect URLs; the original offline checker child receives
neither GH_TOKEN nor GITHUB_TOKEN.

`author-static-review.json` preserves the draft's actual review stage:
metadata derivation, hashes and exact reverse transformation were checked, but
there was no local Python parser/test, Cargo, product, or ZIP reception.
`registration.json` likewise preserves its pre-execution review-stage label.
Only a future original receiver execution, complete retained evidence and
successful actual process exit can establish reception success.

This evidence checkpoint does not close a TODO or grant release certification.
The task ledger remains 192 total, 163 done and 29 remaining; newly fully
completed tasks: 0. Original G1/c8/G4/other-study failures keep their own source
and outcome.
