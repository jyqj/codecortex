# Independent platform, recovery/rollback and failure-gate acceptance

Executed source: a23bb72d3c954f385b99fe81ce9189885c208557. Full tree: 58147c952505c44da1f41eb4b9c31643f2303b96. Product input map: 1087 files, SHA256 4e0aa6bbfd4d5bea00416bca2cd99318d865ffc526fdcb45897c4d300de3ac00. The cold-build row-list manifest is a different encoding of those inputs and retains its original SHA256 33a1a1b06e9d6a76960b3fb03ff1b3e6f22848dcbf148ec3591e551d181781db.

## Accepted delegated scope

`independent-acceptance.json` maps the unchanged original P8-011/012/013/016 acceptance to complete original evidence. It accepts the assigned scope while leaving task status and dependency adjudication to the parent. Observed original task ledger: 192 total, 163 done, 29 remaining. Earlier partial audit reports remain unchanged; their pending entries are resolved by separately bound later reports.

- Platform: eight actual native fresh-target release builds, each job's 59 controls and native stdio; actual original collector job success plus independent original collector replay exit 0, 8 passed / 0 failed / 0 not_run. MSRV 1.95.0 and observed stable 1.99.0; Linux x86_64 and macOS arm64. `platform-eight-cell-collector-audit.json` binds every cell and replay. Only input/source directory fields differ between original CI and transported collector receipts.
- Recovery/rollback: seven actual production-linked fault tests, three local cases, three active HTTP seeds, original 495-file seal, and 14 read-only database snapshots. Current/old historical source identities remain distinct. Raw MCP/HTTP response, EOF, cleanup, cache, generation, source/config and backup observations are retained.
- Failure gates: six original production-linked Rust controls and seven actual independent invocations of the exact retained CLI; expected/actual exits 1,1,1,1,2,2,2. The library-only zero-plan assertion stays library-only. Every original artifact and copied fixture byte remains unchanged.

## Archive selection

Use **only** the exact `files` list in `archive-selection.json`, preserving its relative paths. Each listed file has a measured byte count and SHA256. The manifest itself is the additional archive file. It references the original ZIPs already preserved by the parent; their contents must not be substituted with derived copies. This selection excludes compiler binaries, Docker image bodies/layers, copied fixtures/databases and collector input copies. Original ZIP references, artifact IDs, metadata records, size/hash and member prefixes are in `independent-acceptance.json` and the selection manifest.

All seven gate commands and actual stdout/stderr/output hashes are in `gates-retained-cli/replay-report-v2.json`; the exact executed helper is in `audit-scripts/gates-retained-cli-replay-v2.py`. Reproduction uses original artifact 11592180029: `cc-eval` plus the five `case-*/retained-fixtures/<name>` prefixes enumerated in the acceptance report. Verify its ZIP SHA256 792a4fa9cfdcc3d9f1bf077a20e3d951958c144142fac1e81183aa132c4dbd25 and ELF SHA256 daac7973861d3f7a6a419dfa34a2563af5796a227b016903a5b91c30a793f80a before copying into a new owned replay directory.

The compatible image is `ubuntu@sha256:f610ab94648195aa356059f5b41d6085c9d4d903c072430cdd1af7bdb646106b`, selected from the retained official OCI index. The exact original index, child manifest, config, actual image inspect and pull/GLIBC receipts are included. Docker execution used network none, read-only original mounts and distinct output. This is deterministic Linux amd64 compatibility execution on macOS arm64, not native timing or platform measurement. The existing arm64 tag was unchanged.

The exact helpers use the original source worktree read-only and deliberately create new output directories. Preserve actual original command records; a future replay must use a separate owned output prefix. Do not rerun a helper on its existing output directories.

## Preserved validation-environment history

The first CLI probe hit an older GLIBC; the compatible official image resolved that. A first read-only fixture replay correctly could not let the original normalizer rewrite metrics; V2 used exact working copies and preserved originals. Initial database mode=ro copy replay could not open WAL-format main files without sidecars; final read-only immutable replay required proof that every WAL was absent or empty. These inspection failures and relevant raw outputs remain as history; they did not change original product artifacts or acceptance rules.

## Related CI identity

The parent-reviewed original CI checkout was GitHub merge commit 75649f8cb08dcd5427e7e58b1ca2b8fa67c9d037, whose complete tree equals the execution tree above. Keep that actual checkout identity. The parent-owned `review-ci/ci-original-check-audit-v2.json` and `review-ci/actual-ci-p7-checkout-tree-equivalence.json` are referenced by exact hashes, not copied into this selection. No full scale/soak or release claim is made by these deterministic scopes.
