# Original local P8 history bundle

This incremental bundle preserves the original local source and review commit objects for this batch. The six advertised heads contain 47 commits after the excluded baseline `6d02d77f018a5965a6f289b0b43558ed4b9f8322`. Original commit SHAs, authors, committers, timestamps, messages and parent relationships remain intact.

The bundle is **560,244 bytes**. Its SHA256 is `0db458b30244f383c1658db83cb97728b15b340289011d2c83d328ef69694bc4`.

## Fixed snapshot

| Advertised ref | Original commit |
| --- | --- |
| `refs/heads/work/p8-combined-evidence-20261007` | `6a2333befefbaf8c2d615f0668f3ce80c2187001` |
| `refs/heads/work/p8-failure-gates-20261007` | `b81d786d1f5ff56ab0363822cfe7ded515164543` |
| `refs/heads/work/p8-local-ten-todos-20261007` | `16f2685ae7a428bac5ad4be5804c84166764fe7c` |
| `refs/heads/work/p8-measurement-strata-20261007` | `a59dc75a5acac6be2857423465a9c652d89e4a98` |
| `refs/heads/work/p8-release-evidence-20261007` | `9e9d5cc153253ea20383333331dff7c32fcfbf62` |
| `refs/heads/work/p8-scale-incremental-20261007` | `c9cfe22137b31a177e162bd8279c4f371d75708d` |

All six actual refs were checked against these fixed commits before creation and were checked again afterward. The create command passed all six refs and `^6d02d77f018a5965a6f289b0b43558ed4b9f8322`. `git bundle create`, `git bundle verify` and `git bundle list-heads` each exited zero. The advertised heads exactly match the table. The bundle's sole prerequisite is that same excluded baseline.

`manifest.json` records the exact commands, byte count, SHA256, prerequisite and advertised refs. The original stdout and stderr for all three Git commands, before/after head listings, bundle header and ordered 47-commit inventory are retained alongside it. `SHA256SUMS` covers the bundle and supporting files, excluding itself.

## Import into an existing repository

Use a repository containing the baseline and its history. This bundle is incremental and cannot replace an initial clone. The following commands verify it and import the six heads into a separate archive namespace without rewriting existing work branches:

```sh
git cat-file -e 6d02d77f018a5965a6f289b0b43558ed4b9f8322^{commit}
git bundle verify /path/to/p8-local-history.bundle
git fetch --no-tags /path/to/p8-local-history.bundle \
  'refs/heads/work/*:refs/archive/p8-local-history-20261007/*'
```

After import, the archive refs expose the original local source and review history for inspection or replay. No force update is needed. Import was not executed as part of this receipt; bundle verification was performed against the existing repository, and no additional clone or product test run was created.

## Evidence boundary

The bundle captures the fixed heads above. It does **not** include the later commit that stores this bundle or any subsequent integration, publication or review commits. This avoids a recursive claim that an artifact contains its own future commit.

Remote GitData publication can assign different commit identities while preserving file bytes. This archive preserves the original local identities and authorship so that the original implementation and review history remain available. CI and source-integrity admission continue to depend on their actual fixed source and review pins. The existence or successful verification of this bundle grants no source admission, performance certification or TODO completion.
