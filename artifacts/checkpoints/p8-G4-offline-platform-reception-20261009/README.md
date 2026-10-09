# Fixed original G4 platform offline replay

This proposed auxiliary branch is independent of product P/R/G and does not alter its validation inputs or measured refs. It checks out its actual controller event commit into `receiver` and original G4 `260f596582f2d82b8d7c707b61a6b8b6a43b069f` into `source`. It is not a new cold build or a new matrix study.

Nine exact archives from original run 37854847808 attempt 1 are registered: eight distinct exported cold cells and its original collector. Their complete size is 68,741,273 bytes; actual full expansion is 222,194,711 bytes in 153 regular members. The four previously stored macOS originals and four Linux originals were already read in memory with complete CRC/SHA checks. That preliminary transport/source binding is not presented as execution of the original path-dependent collector.

The original collector requires real `Path.rglob`, `resolve`, `lstat`, regular-file/symlink checks, archived observer modules, fixture sources, full Cargo logs, binary copies and stdio evidence. Its CLI also exclusively creates a fresh output directory. The hosted receiver therefore extracts every original member, including all eight binaries and hidden fixture database/config files, into a new owned runner directory. It uses no virtual-Path shim, rewritten predicate or truncated fixture.

The sole replay command is the original fixed G4 script:
```
python3 -B source/scripts/p8_cold_build.py --source-root <exact-G4-checkout> --collect-cells <all-eight-original-cell-directories> --expected-commit 260f596582f2d82b8d7c707b61a6b8b6a43b069f --output-dir <fresh-replay-directory>
```
The original `collect_cells` calls its full source/Cargo/copy/observer/stdio checks for every cell and rejects missing, duplicate, changed or unpassed cells. It does not run Cargo, rustup probes, a native product, a provider or a parity workload in this CLI mode. It binds the original 1087 product files and four cold observers; the newer 1089/9 candidate is outside this scope.

The replay matrix must equal the archived original matrix in every field after mapping only its `source.source_root` to the actual checkout path. Both original and replay JSON bytes are retained. ZIPs, every expanded member and exact source/observer snapshots are checked again after the command. The outcome remains a scoped original G4 platform replay; task dependencies and release approval are separate.

The controller verifies full paginated original run/jobs/artifact metadata, each exact registered name/ID/size/digest/source and successful producer identity before download. It preserves 512 MiB filesystem headroom plus all nine ZIPs, their exact full expansion and 64 MiB output allowance. An unavailable artifact, changed bytes, insufficient capacity, original checker failure or upload failure stays nonzero; no retry, replacement sample or selected subset is accepted. All original ZIPs and files, successful or partial checks and inventories are uploaded with hidden files included.

No raw Actions job logs are downloaded: they are not inputs to the original cold collector. The full Cargo/stdout logs already inside the original nine archives remain intact. Authenticated curl transfer follows the independently reviewed receiver helper: a timeout returns a fixed API-relative error without serializing the argument list or Bearer value. Signed redirect URLs are neither printed nor stored. The job uses only ordinary GitHub-hosted Ubuntu, first attempt, contents/actions read permissions and a single 40-minute job.

This is a draft awaiting non-author review and actual publication. No task is marked complete and no original evidence is relabelled.
