# Owned-writer repair: published source proof

The runtime controller now requires every owned work item, sampler thread,
product process, stdout reader and exit watcher to stop before it seals evidence.
An interrupted product construction is tracked even when its handle was never
returned. An unfinished owner produces an explicit failed, unsealed result;
fully stopped successful and failed executions retain their original seal rules.
This repair does not claim to recover an inaccessible partially constructed
product handle.

## Immutable publication chain

| Role | Commit | Tree |
| --- | --- | --- |
| Product P3 | `362a8216537dd1df614077abc6b6e2f98ef8a054` | `caf479a6dc3800262d8b42fd92b7de337025b38d` |
| Independent review R3 | `32f86004f823d21a3b2412f1e652bb3d581c9315` | `22249ab23e14136aaf8caed73c35c7013979e739` |
| Admission PINS3 | `adfdfdfa9e668ff1ffb9d07de1a36ac7292d2a66` | `dc86b1a26bdbcc6541da1686ce17d6575164c110` |

P3 is the direct child of the previous evidence head
`44f3c06b1d57709daeaa8bfcc8879f4a15b02570`. R3 and PINS3 are successive direct
children. This directory is appended after PINS3; none of the prior evidence is
rewritten or assigned a later execution identity.

## Dynamic controls and independent review

The unchanged ten-case independent reproducer exposed seven false seals in the
previous combined product. Six cases also retained actual RPC-file changes after
return. The corrected product passed all ten cases: each unfinished owner remained
unsealed, while stopped successful and failed controls kept their expected behavior.
The actual sampler, subprocess, stdout-reader and exit-watcher controls are retained
in [the owned-writer evidence](../round7-owned-writers/README.md), along with original
results, the script, timing limits, provenance qualifications and raw files.

The final two integrated runtime and test inputs passed **381 P8 Python tests** in
69.178 seconds. The receipt records their before/after hashes and the original
pre-commit execution HEAD. No local Rust compilation is claimed for this Python
repair. All 1,087 Rust/Cargo inputs remain identical to the independently reviewed
owner candidate `599a7050e7d52b5b7b93975c419138e175b3f754`.

The complete 136-path validation inventory changes only the runtime and its test
hashes in this increment. The prior 15 source-review records, six incremental
records and 30 reviewed source paths are retained. The new increment adds its own
two input reviews. R3 preserves the previous main review as an exact historical
copy. The 4,797 original raw archive members were independently checked against
their original files and index.

PINS3 changes three guard constants and four registry fields only. The v15 version,
base, acceptance predicates, exclusions, CI selector and six frozen historical
inputs remain unchanged. The validation field changes only two hashes.

## Actual PINS3 execution, including the original failure

The first full-proof attempt is preserved in
[source-proof-original-failure](source-proof-original-failure/receipt.json).
It exited 1 when the inherited v14 proof tried to fetch original historical commits
from a local `origin` whose shared repository directory no longer existed. The
other three commands passed. That attempt remains failed.

The local origin was restored to the same public GitHub repository and missing
original historical commits were fetched. No source, guard or acceptance change
was made between the attempts. The restored execution at the same actual PINS3
commit passed all four unchanged commands:

- Full `verify_reviewed_source_v15.py` proof, including actual v14 execution:
  **211.010 seconds**, exit 0.
- Original task-plan consistency check: exit 0.
- Declared documentation facts check: exit 0; this is not runtime certification.
- Working-diff whitespace check: exit 0.

The [restored receipt](source-proof-restored/receipt.json) has SHA-256
`b80e2010513ba4ca0d0cf27369a1e449aaebc9169f0130b30770f3bc4ae8bc06`.
The complete proof log has SHA-256
`564bb4839801ad990737980f2e14345d2fcaa43f323e2e5463336ad5a2fd1347`.
Both attempts record identical before/after HEAD and four input hashes. The
independent final readback checks their original receipts and logs without rerunning
or relabeling them. The local origin repair receipt is also preserved here.

## Original task accounting

The original plan remains **192 total / 163 done / 29 unfinished**. This increment
completes **zero original TODOs**. It establishes controller reliability and source
admission only. Full scale and each dependent workload, recovery, platform and
release task still require their own original acceptance evidence. No task status,
dependency, sample count, deadline, scope or performance threshold changes here.
