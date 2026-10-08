# Round 6: runtime failure cleanup

This checkpoint imports only the two runtime files reviewed from PR #160 at
`d2d492b329a5630bf4a5fa6bc4b6eac30d07b13c`. The product's complete 1,087
Cargo/crate inputs remain identical to execution commit
`599a7050e7d52b5b7b93975c419138e175b3f754`. The existing source guards, task
definitions, sample counts, concurrency profiles, deadlines and thresholds are
preserved.

The original driver could lose statistics after an oracle difference and could
try to seal an observation before an owned executor worker had stopped following
a raw-evidence budget exception. Independent controls reproduced these paths.
The correction drains owned workers using the existing bounded cancellation
policy, records unconfirmed termination as an unsealed failure, and retains
statistics when the oracle reports an actual difference with exit 1.

`independent-failure-path-review.json` contains the independent agent's source and
failure analysis. `root-independent-review.json` records the main reviewer's
separate execution of the 14-test suite. The original and patched test logs are
retained, including the three observed failures of the original implementation.

The three `.tar.gz` archives preserve all files from the original, patched and
main-reviewer controls. Every member was rehashed against the corresponding
inventory and the source files were checked again after archiving. These controls
use real executor threads with explicitly fake product, compiler, oracle and
statistics protocols. They establish cleanup behavior, not product performance.

The success path also changes executor shutdown and report sealing. Consequently
the replacement observer requires new actual C1/C4/C8/C16 profiles with 900
requests each, an actual one-hour soak, and the 768-request fake-provider backfill
under newly bound source/build receipts. Existing 599 evidence keeps its actual
commit and observer identity. The unchanged scale execution continues against
its original frozen 599 inputs. The applicability boundary is documented in
`evidence-applicability-boundary.json`.

No original TODO is closed by this checkpoint. The task ledger remains
163 complete out of 192, with 29 remaining. Task closure requires the complete
original acceptance chain and independent review of the actual new execution
artifacts.
