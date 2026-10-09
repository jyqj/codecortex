# Round 25 — startup repair, reviewed integration and fourth original G8 shard

Original ledger: **192 total / 163 done / 29 remaining / 0 newly completed**. Cutoff: 2026-10-09 19:49:09 UTC.

The original G8 50k repetition 0 passed the unchanged original validator. Complete original custody is in parent commit `80be48c96470f7ef52a51e448521bf4baa4f93b4`, bringing that study to 4/150 shards and 41/1500 samples. The earlier local executable-mode failure is preserved.

The actual second wide startup failure led to one exact rustfmt repair and restoration of two required existing Rust modules in the build checkout. Source-only independent review and literal binding review now cover GW4 `008f1ec0ae16b9b6aff9f20d033b17b2219168f8`; its new run `37982220020` was queued at the recorded observation. Both failed earlier runs remain failed with zero measurements.

PR #195 combines the reviewed cold and wide code under final source P `58a890857a6b6c2da874491817beb6d561897a9a`, R `b39fdd8c858755d1a92c176efc5340907f4a3757`, and G `34dc4e14dc96d077eae6173ab6885370323eff3f`. Its final native CI remains pending. The original 44+12 Python controls passed on five exact PCW2 files that are unchanged in final P; execution identity is retained. An extra CI step was rejected by the original v15 full-file constraint and was reverted. No guard or timing threshold was relaxed.

The cold-study owner still declares 16/150 accepted samples, with 10 in published original custody. New intake responsibility for unclaimed 100k repetitions 12–29 and 50k repetitions 1–10 is recorded in PR #189 comment 6088047678; successful jobs alone do not increase acceptance.

Canonical full source reviews remain at their actual R commits. This checkpoint preserves the incremental failures, controls, independent binding proofs and root publication receipts without changing task definitions, dependencies, raw measurements, N or budgets.
