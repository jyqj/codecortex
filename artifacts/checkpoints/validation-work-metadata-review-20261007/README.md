# Optional-work metadata boundary review

This record pins the complete seven-path validation-work delta to an immutable
source and an independent, limited code/evidence review. It includes the earlier
SQL-accounting and body-protection changes and the new early metadata probe.

The new two Git blobs came from the tested combined A+B author source. The
derived A-only tree contains 766 crate/Cargo inputs and was not independently
behavior-tested. The combined source contains 768 inputs; its 12 named narrow
tests and format check passed. The source records distinguish those trees and
attribute execution to the author. Full-default and final CI remain separate.

`review.json` is the source-admission record. `independent-review.json` preserves
the peer's fixed-source review and verified narrow-log hashes unchanged. Earlier
failures and source versions are preserved in their original directories.
