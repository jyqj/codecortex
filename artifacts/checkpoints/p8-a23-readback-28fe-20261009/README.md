# Fixed A23 original artifact readback

This is a read-only data reception controller prepared during round 18. It addresses an unavailable local execution service by using ordinary GitHub Actions to receive existing original artifacts. It does not rerun any product workload and does not replace the active 150-shard A23 study.

## Fixed inputs and execution boundary

The actual reviewed product source is `a23bb72d3c954f385b99fe81ce9189885c208557`, tree `58147c952505c44da1f41eb4b9c31643f2303b96`, with 1087 native inputs. The workflow copies its controller outside the product checkout and then obtains this exact source at the actual workspace root. The prepared controller commit has no product-source admission of its own and its source identity is never substituted for A23.

[controller/protocol.json](controller/protocol.json) fixes 18 original ZIPs by artifact/run ID, name, repository identity, byte length and SHA-256: five runtime observations, backfill, lifecycle, eight platform cells, original platform collection, recovery and original failure gates. Original observations retain their budgets, source/binary identities, statuses and denominators. A failed transfer or readback does not change an original native outcome.

The runtime module reuses three unchanged review scripts from fixed archive `4630e635cd762bfbd2726cf1dd49e37803c06034`; it verifies original raw, receipt, stdio, statistics, cache, resource and parity evidence without executing the retained binaries. The platform module calls the existing portable collector, checks recovery raw, and runs read-only queries on explicit copies of archived databases after rejecting nonempty WAL. Gate results are read from their original and retained replay records; no native gate or Docker execution occurs here. Historical review prose that says Mac is preserved as original content; the new execution receipt records the actual Linux environment and data-only scope.

Transport checks bind the official metadata, preserve the complete original ZIP bytes, validate safe regular paths and actual compressed-stream EOF/size/CRC, and seal inputs before and after review. Partial downloads and failed reads are retained and keep the overall result nonzero. Both source and controller inputs are checked before and after. The upload stage includes regular files only and records skipped view symlinks, while retaining original hidden fixture files; checkout credentials are not persisted.

## Review and execution state

[preparation-manifest.json](preparation-manifest.json) records the static findings and the preserved, unexecuted drafts. Runtime and platform modules have been statically reviewed by a non-author. The whole-package review and the first actual Python parse, bounded transport controls, and complete data readback remain separate steps. No prior reviewer report or successful seal-only CLI is treated as this controller's execution success.

The root will publish the controller only after the exact prepared package receives its independent static review. The named audit branch is single-use for that publication; it is not a measurement continuation and must not be reused to reset any old D0 attempt or quota.

**Original task ledger: 192 total, 163 done, 29 remaining; zero newly closed.** Full original scale acceptance remains pending, and no task definition, dependency, threshold or source-admission pin is changed by this preparation.
