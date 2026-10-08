# Original measurement evidence and archive format

These directories preserve the original observations, source and build receipts, independent reviews, and artifact metadata for the fixed executions identified in each package. Acceptance is recorded separately for each workload and task. An archive or packaging check does not turn an incomplete workload into an accepted result.

## Package identities

| Directory | Recorded execution |
|---|---|
| `scale-599` | Fixed `599a7050e7d52b5b7b93975c419138e175b3f754`, scale run `37835810882` |
| `runtime-296` | Fixed `29682890c89511dd6f477a6bf48bd969aa1537af`, runtime run `37844310853` |
| `lifecycle-599` | Original lifecycle artifact `11576107053`, run `37835809247` |
| `platform-p7-gates-599` | The exact 24 original artifacts listed in the packaging manifests and independent preservation review |

Original ZIPs, derived packages, and execution results have different identities. The official artifact metadata identifies each original ZIP. Derivation and preservation records bind every retained member to that ZIP by path, length and digest, and identify any omitted executable explicitly. A derived `.tar.gz` package is not the original GitHub artifact ZIP.

The lifecycle/runtime/platform/P7/gate packages retain the original non-omitted members byte for byte, including raw reports, failures, SQLite evidence, fixtures, configuration and source/build receipts. Only the native executables explicitly bound to original build receipts are omitted from those derived packages. Two P7 process-inspection executables without that binding are retained. Use the original artifact identified by its official metadata when an exact replay requires an omitted executable; a new compilation is not a substitute for its recorded binary identity.

Every archive-fidelity review states its own scope. Comparing packaged members to original ZIP bytes verifies preservation; it does not rerun a workload or re-establish product acceptance. Historical failures, ignored tests, `not_run`, unknown resource values and inconclusive results retain their original meanings.

## Restore archives delivered in parts

Large archives are delivered as ordered byte parts to stay within the Git transport request size. Splitting does not alter the archive bytes or recompress its members. Each `<archive-name>.parts/archive-parts.json` records the part order, individual sizes and SHA-256 digests, the restored filename, and the complete archive digest.

| Archive | Part manifest |
|---|---|
| Lifecycle original members | `lifecycle-599/11576107053-non-elf-original-members.tar.gz.parts/archive-parts.json` |
| P7 closeout original members | `platform-p7-gates-599/p7-closeout-original-raw.tar.gz.parts/archive-parts.json` |
| Complete original scale build ZIP | `scale-599/original-artifacts/11576810573/p8-scale-build-37835810882.zip.parts/archive-parts.json` |
| One-hour soak original members | `runtime-296/soak/11585132175-non-elf-original-members.tar.gz.parts/archive-parts.json` |

From the repository root, restore an archive to a new output path:

```bash
python3 artifacts/benchmarks/p8-completion-20261009/restore_archive_parts.py \
  artifacts/benchmarks/p8-completion-20261009/scale-599/original-artifacts/11576810573/p8-scale-build-37835810882.zip.parts/archive-parts.json \
  /tmp/p8-scale-build-37835810882.zip
```

The helper verifies every part and the complete restored archive, refuses to overwrite an existing output, and does not extract or execute archive members. Without the optional output argument, it writes the original archive filename beside the `.parts` directory. Run it with normal Python settings so its integrity assertions remain enabled.

The preserved preparation records may list a complete local archive path that existed when packaging was reviewed. For the entries above, the committed representation is the `.parts` directory; restoring it recreates that complete archive with the same recorded size and digest. The preparation snapshots remain unchanged.

Independent byte comparisons cover the lifecycle and P7 closeout 19 parts and the scale build three parts. The soak part manifest and its separate review bind its three parts to the complete reviewed archive. `archive-restore-cli-review.json` also records an actual restoration of the 9,212,603-byte scale build ZIP, its exact match to the retained original, and refusal to overwrite it on a second invocation. No archive members were extracted by that control.

Any additional archive supplied with its own `.parts/archive-parts.json` uses the same restoration contract and records its own complete identity. Consult its manifest and preservation review before treating it as an original ZIP or a derived package.
