# Round 24: wide toolchain repair, original CI failure, and integration preparation

Closed at 2026-10-09 19:03:56 UTC. Original TODO accounting remains **163/192 done, 29 remaining**; **zero** original TODOs completed in this round or credited from these artifacts.

The first independent wide run (37974375238, source a9448ec...) failed before rustfmt could execute because the fixed Rust 1.95 toolchain did not include cargo-fmt. PW3/RW3/GW3 add only the required build-job component, independently preserve the source chain, and mechanically bind the new source a15034da7c6a6b1dda79a7aa1a1efd843d71fe15. A separately created run 37976092549 remained queued at the 19:01:59 observation. Neither formatter execution nor native protocol test success is claimed.

G9 check 113968501720 (run 37974288651) executed on a test-merge tree identical to source 52bef997f0825451e32fc6291f90cd8df5c8bbd7. Format, Clippy and all default test-target compilation passed. The subsequent benchmark_fixture test failed its unchanged <500ms warm-p95 assertion: the index tool reported 556.28ms. Its full=true corpus case repeats complete builds in one session. The original log has no phase breakdown establishing a cause. Later Python and source-admission steps were skipped. The failure, partial passes and separate MSRV status are preserved without a waiver or retry.

The original G8 150-shard/1500-sample study remains source4fe927..., run37962416564, attempt1. At the original 18:55–18:56 observation it had 3 accepted shards /32 samples; 50k's unique original ZIP was formally materialized but not yet validated, and the original 100k job was still running. The separate cold study remains source044c008..., run37954851017: 25 measurement jobs successful, author reports16 accepted, published custody covers10; these counts are not interchangeable.

A read-only combination of G9, GW3 and cold10ec preserves 39 explicit leaves and all prior histories. The candidate contains four jointly modified files reconstructed from50 nonoverlapping original-line edit intervals, plus exact incoming leaves. Four candidate product blobs were subsequently created with matching server Git OIDs. At round close no combined P/R/G commit existed and independent review remained pending. No native tests, new study, source admission or TODO completion is claimed from this source preparation.

## Artifact roles

- Formal accounting and actual source/run identities: formal-round-closeout.json.
- Original root Git/ref/PR/comment operations: root-publication-receipts.json.
- Original missing-component failure, one-line repair, review and generated binding proofs: first-wide-failure-* and toolchain-* files.
- Independent generated-binding and actual-GW3 checks: independent-toolchain-* files.
- Original full paginated study observations and new-wide startup snapshots: original-studies-* and new-wide-* files.
- Full unmodified G9 native failure log, independently verified checkout and timing report, and static full-build call chain: G9-* files.
- Frozen combination author's source bytes, original donor patches, all input maps and exact leaf plan: wide-cold-combination-candidate-original.json.

The prior source-only canonical reviews remain at their original P/R/G commits. These files do not replace, pool, rename or reclassify any original raw cohort. The original TODO definitions, dependencies, sample commitments, limits and historical failures remain unchanged.
