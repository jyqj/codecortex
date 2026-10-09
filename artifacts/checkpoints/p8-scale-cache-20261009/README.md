# P8 combined performance and cache-observation source admission

The fixed product is `254009277688d64677361a0ca33e5dea73f295ef`, tree `4202266e59934dba86e1ea10154392247fff15e7`, on main `55902428d49b09bb4a90cee6ccccf985680618bb`. This checkpoint admits the precise source and validation inputs for new execution. No original TODO is complete here: **163 of 192 done, 29 remaining**.

The original fixed `599` scale study finished with an actual 100k native deadline failure and a rejecting aggregate. Its 4 accepted shards and 41 accepted samples remain evidence for that source; none can fill a new study. The original 296 hour still has valid duration, resource, queue, parity and seal evidence, but all recorded search-cache counters were zero. The cache-coverage correction is preserved beside the original failure diagnostics.

Six selected product files add bounded 64/8/1 oracle insert batching, reuse the existing per-prepare dependency window, and route three unchanged SQL writes through the existing prepared-statement cache. They keep canonical bytes, duplicate/order handling, the complete 15-table oracle, SQL values, transactions, generation fences and source identity checks. Existing cancellation and shared nearest-rank fixes remain present. Eight exact e95 scripts and tests add actual cache observations across the hour and fail closed when final sealing fails.

The three independent reviews cover the complete 1087 product inputs and 138 validation inputs. The original v15 historical approval mechanism is retained; only the exact product, review, review-file and registry-digest bindings are to be installed after this admission commit. The complete source review preserves 29 previously approved deltas byte-for-byte and reviews the six new or extended paths.

The new exact execution must run the original complete CI, 150-shard/1500-sample scale study, six runtime profiles, full lifecycle, recovery/rollback, eight platform cells and original failure-gate controls. All registered deadlines, memory/output budgets, seed, sample counts, fanout, dirty/resume limits and parallelism remain fixed. The auxiliary 8e study has a different source and contributes no new-source acceptance samples. Its 41 controls omit the seven new inline controls, which must actually pass in the unchanged full CI.

The source review is not a runtime result, task closure, quality approval or release approval. All earlier failed and successful source identities retain their own scope.
