# P8 Round 43 — PR management and acceptance delta

Frozen at 2026-10-09T21:44:00Z.

This checkpoint records only new facts since Round 42. It does not merge an evidence branch into `main`, change workflow budgets, dispatch or rerun studies, relabel samples, or mark a subgate/PR/CI result as an original TODO.

## Outcome

- Authoritative `main`: `51138c236cf2ed47858e2306f9c7bfd3600aaef9` (tree `24df8be11955fda1f2e320de483e9eb4ddd1ffb3`).
- Authoritative tasks blob: `959a25ba851ff88f286bab0fa14167878129b1dd`.
- Ledger: **192 total / 163 done / 16 in progress / 12 todo / 1 blocked = 29 remaining**.
- Newly completed original TODO IDs: **none**.
- Cumulative progress after the 163-done baseline: **0/10**.

## New accepted facts

1. PR #196 completed seven exact-head workflows and 26/26 jobs, including the one-hour soak, and the concurrent owner merged it as `51138c…`. The four narrow installer/archive/sparse-action fixes are useful product progress but do not complete P8-017/018/019 or any priority original TODO.
2. Cold-only run 37954851017 is genuinely terminal: 152/152 jobs succeeded and its 150 measurement slots cover 1k/5k/10k/50k/100k at N=30 each. The aggregate artifact was downloaded and independently checked for official/local SHA-256 equality, ZIP CRC, JSON identity, five-scale populations, unique sample/receipt IDs, source/run/attempt binding, and workload counters.
3. That cold aggregate closes only the evidentiary coverage portion. It explicitly retains PID-only snapshots rather than whole-process-tree peaks, and it contains no qualified paired-effect/causal gate. Complete raw shard custody/integration review also remains in progress. Therefore P8-005 is not marked done.
4. PR #195's new bound head restores sequential A+B spooling, adds the A-fits/A+B-SQLITE_FULL control, removes the public witness field, and pins the original nine-field JSON contract. The old three source blockers are statically repaired. Exact-head CI is still nonterminal and source/binary-bound representative causal evidence is still absent, so the PR remains Draft/HOLD.
5. The independent dirty4096 50k preflight artifact passed full ZIP/seal/plan/source/binary/parity review, but it is a different profile and contributes zero credit to the original dirty200 population.

The JSON reviews under `review/` are immutable unattached blobs produced independently by three agents and attached here without rewriting their content.
