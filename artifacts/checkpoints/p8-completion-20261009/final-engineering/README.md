# P8 final engineering evidence archive

This archive preserves 1,518 original files (139,705,330 uncompressed bytes) from the local engineering work preceding final P2 source review. Original source IDs, raw bytes, failures, controller cancellation, blocked reviews and explicit `not_run` outcomes remain unchanged. The archive does not change any TODO status or provide formal scale, one-hour runtime, platform, release or P2 approval.

`files.manifest.json` maps every original workspace-relative file to its tar part and records SHA256, byte length and file mode. Extract **both** tar parts into the same fresh directory to restore the relative paths. The two runtime build packages retain all three original binaries; their paired observation packages retain all 230 sealed files each. No file listed by either original outer seal was removed. Cargo caches and large checkout copies are excluded.

`summary.json` records evidence groups and their original outcomes. In particular, the 083 release compile failed, the SQL-ordinal attempt was explicitly cancelled, two Clippy attempts failed before the successful c6 check, the 6956 platform review found acceptance counterexamples, and the c6 capacity preflights remained `not_run` because registered disk thresholds were unmet. The bce short runtime observations and the 570 Python controls keep their own original source identities; they are not relabelled as later-source executions.

## Verification

From this directory, verify the delivered files and every tar member:

```sh
python3 -B verify_archive.py
```

To also extract the evidence into a new temporary directory and invoke each runtime pair's preserved original offline verifier:

```sh
python3 -B verify_archive.py --runtime
```

This command checks retained evidence without starting a product measurement. Both original paired verifiers were executed successfully during archive creation; their commands and results are in `verification.json`. Each build seal contains 15 files and each observation seal contains 230 files. All 1,518 original source files were rehashed after packing to detect concurrent changes.

If an existing Git repository contains the declared prerequisite, additionally verify the source bundle:

```sh
python3 -B verify_archive.py --runtime --repo /path/to/codecortex
```

`DELIVERABLES.json` hashes every delivered top-level file except itself. Its SHA256 is supplied separately by the parent delivery receipt. `build_archive.py` records the exact local construction recipe; `verify_archive.py` is the portable consumer checker.

## Source history

`sources.bundle` passed `git bundle verify` and contains exactly these three refs:

| Ref | Local commit |
| --- | --- |
| `refs/heads/task/p8-final-candidate-20261009` | `570b2afb33d19cb56e110fa0130b529541327fdd` |
| `refs/heads/task/p8-scale-20261009` | `958c7d58e8f4486a0dd3917395a8883a58c1aaff` |
| `refs/heads/task/p8-platform-review-20261009` | `88c580cdd216e4347d0a625ade92a865ddecf6a3` |

Its **only prerequisite** is `7354db236c9d9850a75f31672697ae9eab44565e`. It is an incremental bundle: the prerequisite commit and its reachable history must already be available. The bundle preserves the local source IDs used by the engineering record and excludes the subsequent commit that will carry this archive. No remote SHA equivalence or final source approval is implied.

The completed source intake and engineering audits are included. The later formal P2/R review remains a separate artifact and is not part of this engineering archive.
