# P8 cache observer follow-up source review

This checkpoint accepts the exact cache observer source at P4
`f5518ca6d21f54cfd324e33edc3b739608ae28b8`, parent G2
`d2d492b329a5630bf4a5fa6bc4b6eac30d07b13c`. It does not close a TODO.
All 1,087 crate/Cargo source inputs remain identical to the accepted P3/R2;
one validation input changes and one is added, for 136 validation inputs.

The new original-duration soak explicitly observes local cache misses, hits,
generation invalidation, current source grounding and same-process query pool
progress. It retains the existing 3,601 operation / 2,400 read denominator.
Each compound read has four named RPCs, all included in its latency and raw
records. Actual completion times determine quarter coverage. Original runtime
budgets, resource sampling, mutation/branch/compaction cycle and full-build
15-table endpoint oracle remain unchanged. Mixed workload behavior is unchanged.

`cache-engineering-originals.tar.gz` contains 167 files / 5,533,275 original
bytes. `cache-engineering-manifest.json` records every member, mode, byte count
and SHA-256, together with the archive SHA-256. Packaging actually read back and
verified every member. It preserves initial rejected candidates, independent
counterexamples, their fixes, original and new protocol-control output, source
manifests and actual small-probe raw execution records.

The actual small probe ran the original G product binary from Actions artifact
11573313154 (`product/codecortex`, SHA-256
`9ebbd85103ae3d4e88fc13d2fcf6144438a0dfbcfba44cbe6a4e0232bf8a8f7e`)
with the explicitly distinct v3 observer. One full build and six real mutations
were followed by 14 compound reads / 56 read RPCs. The observed generations were
5, 10, 16, 22, 27, 32 and 32; the final restore preserved the generation and
correctly hit the cache twice. All other pairs were miss then hit. The process
closed with EOF and exit 0. The binary itself is not duplicated in this archive;
its original artifact identity, member and digest remain explicit.

`actual-small-probe-independent-audit.json` and its inspector were produced
after the archive snapshot and are separate files here. That non-author review
matched all 58 sealed file digests and every request/response/wire row, verified
all 14 probes again, and bound the root-authored probe script separately. The
probe lasted 0.648 seconds and was sequential. It supplies neither one-hour
coverage, a performance/resource certificate, full endpoint parity nor a final
P4 build. Final long-duration cache validation must run independently at its
actual source and binary identities.

`finalize_cache_source_review.py` binds the fixed source inventory and review
evidence; it does not infer execution from hashes. `import_published_commit.py`
reconstructs only connector-published Git objects and requires an exact Git SHA
and tree match before admitting them. Original source gates, CI behavior and
the six frozen v14 proof files are unchanged. The subsequent guard commit
updates only the three fixed pins and the complete reviewed registry.

At this checkpoint: 192 original TODOs, 163 done, 29 remaining. No original
acceptance definition, dependency, sample size, limit or task status changed.
