# Round 40 finite evidence checkpoint

This append-only checkpoint preserves 54 explicitly selected original files (3,596,158 bytes) on evidence parent `579f3dbbba3650cceb0a2c4af6acf1618e57ff99`. It changes no source, task definition, task status, validation threshold or running study.

The archive contains:

- Two complete non-author PR193 source reviews, covering its six changed paths in separate scopes, and the resource-overlap report. These are static reviews, not native test or performance results.
- Chunk formatting successor `7da43a80bc00698101de5619dd486a3393a667a3` source-admission records. The original v15 CLI returned 0, but its outer wrapper returned 1 because the raw Git index checksum changed after an intervening index-refreshing command. Original content, modes, staged objects and HEAD remained unchanged. Both before/after records and the failure explanation remain intact. The task-plan CLI then ran once separately and returned 0; the guard was not rerun.
- Complete original ordinary-CI log and review for Cold formatting successor `10ec828beb94200ec679302c45115760ebd55ee9`. Those results do not relabel the independent running cold study at `044c008c9459cfa61e7701db2eb868342c508a39`.
- Complete original G6B soak log and its scoped review. This is a log observation, not an independent complete raw-ZIP acceptance.
- The complete two-file Oracle equal-output-lifetime candidate, its patch, manifest, final non-author peer and finite preparation evidence, together with actual source-admission records for `f6750cad0df45af4ec2dddc456e76cc188f3a35a`. That candidate had no new source ref, PR, label or native cost run. It does not revise the older 100-pair experiment or establish an overall parity speedup.
- Three root records: Round39 closed at 2026-10-09 17:15:27 UTC; Round40 prioritized the already-running original full study at PR193 while keeping PR192's separate 1350-sample protocol unstarted; original helper acceptance reached 1/150 shards and 9/1500 samples at the recorded first-intake checkpoint. Each earlier peer and log retains its own capture time and scope. Independent cold samples are never pooled with PR193.

`selected-files.json` provides the exact source-to-member mapping. `payload-manifest.json` records every member's bytes, mode, SHA256 and Git blob OID. `build_bundle.py` uses sorted regular USTAR members, zero owner/time fields and gzip mtime 0. Complete reopening matched every original source, and a second serialization matched the archive bytes. `readback-receipt.json` records that packaging verification.

The 67 files already selected at `95e54f22340d0cd1a0b16e1de01ad81fc9bf5934` and 40 files at the parent checkpoint are not selected again. Distinct actual executions retain their separate stdout/stderr paths even where their bytes happen to match. Guard views, private Git/object caches, raw index binaries, API/tool envelopes, native ZIPs/ELFs and private transfer references are excluded.

The package is prepared as a candidate; creation or movement of the evidence ref is a separate root operation. Formal task accounting remains **163 done, 29 remaining, 0 newly closed**.
