# P7-012: read-time dense artifact coverage

This checkpoint preserves the original author runs and independent static review.
It adds no new Rust test execution or task status change.

| Source | Identity |
| --- | --- |
| Baseline production | `65eb87d70bd7bfd10d251b5d1cbb25850958196b` |
| Final independent fixture | `6d860d0a9fc28bc89331789e6c9e1c4e0a7197be` |
| Fixed source | `f6c860c96f22e39bc3ced5a81891e3070ffbada5` |
| Fixed source tree | `50890bf1bd5adfe1b38cb12270ffad6ded7b83fd` |
| 769-input source manifest SHA-256 | `bb08eb49c0740ccf19cc54dc18fa31674a32c5c78ade4332cd6122b7e80cfeb1` |

The final fixture is unchanged between the baseline and fixed-source behavior
runs. A real worker publishes three synthetic embeddings; payload damage occurs
after its physical job exits and the worker closes. The baseline returns
`Complete` after in-scope cache loss or corruption: **2 pass / 2 fail**. The fix
reports a noncacheable `Partial`, retains valid candidates, and observes repairs
on repeated queries without an epoch change.

Fixed-source runs: new fixture **4**, exact unit **21**, original HTTP-feature
scope/acceptance targets **13**, manifest/allocation integration **11**, and fusion
acceptance **10**: **59 passed / 0 failed / 0 ignored**. Two strict clippy scopes
and workspace formatting also passed. Baseline reruns are not added to this total.

## Evidence layout

- [validation.json](validation.json) indexes the original run receipts and
  executable identities. Each command verified source inputs before and after.
- [independent-review.json](independent-review.json) is the independent static
  review. Its author did not rerun the Rust behavior tests.
- [Acceptance scope](p7-012-acceptance-scope-20261007.md) and its
  [structured record](p7-012-acceptance-scope-20261007.json) cite the original
  P7-012 task brief and the separate V19 quality responsibilities.
- [archive-index.json](archive-index.json) records every original file size and
  SHA-256. Direct receipts, review, source manifest and acceptance files are
  byte-for-byte copies, including their historical absolute paths.
- `raw-artifacts.tar.gz` losslessly holds all original logs and per-run source
  manifests. It also retains the original compile-failing fixture and the two
  baseline production files needed to reconstruct the source differences.
  No log lines, failure details, JSON diagnostics or manifest entries were removed.

The first fixture compilation failed because two digest constructors were
crate-private. That compile-only attempt remains under `baseline.*`; it executed
no behavior tests. The corrected fixture retains validated input digests from the
real worker/provider calls. `baseline-v2.*` is the actual behavior-red run.

Original receipt log and source-manifest basenames resolve to archive members.
Extracting the archive beside the receipts restores the original flat layout.
The archive uses fixed timestamps and does not contain absolute paths or links.

Verify all packaged bytes and recorded counts without rebuilding:

```sh
python3 artifacts/checkpoints/p7-dense-artifact-coverage-20261007/verify-evidence.py
```

For a checkout containing the fixed source inputs, additionally pass
`--source-root /absolute/path/to/checkout`. This optional check compares all 769
inputs. The validator does not certify later HEADs or independently rerun tests.

## Acceptance boundary

P7-012's original engineering requirements are rank-only fusion, honest coverage,
and distinct timeout/empty/partial outcomes. Parent acceptance and completion of
the P7-011 hard dependency are still required before its task status changes.
Full V19 corpus/holdout/statistical quality, P7-019's own ablation and P7/G7 overall
acceptance remain separate. Synthetic and loopback evidence provides no live
semantic-effect certification. The four previously recorded default-scope
failures on the baseline remain historical failures; this checkpoint claims only
the explicitly listed fixed-source scopes.
