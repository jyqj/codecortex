# C8 original study custody

This directory preserves all 12 original GitHub Actions ZIPs from run 37902429727, attempt1, executed on c8be5afaac568ffd40ef86d3795423c3b73c9f39. It is added on top of that exact commit: all source and pre-existing artifacts remain present, including the historical evidence unique to this branch.

The 12 ZIPs total 19,797,295 bytes and are stored as19 ordered, content-addressed chunks of at most2MiB. No ZIP member is removed, recompressed, edited or substituted. Each original metadata file retains its GitHub ID, source, size, digest and expiration date. Repository custody does not change or extend the original Actions expiration metadata.

The study ended in failure: the100k preflight failed, measure was skipped and aggregate failed. The independently accepted historical subset remains4 shards /41 samples; partial failed100k stages provide no additional accepted sample. This archive does not certify150/1500, relabel any execution as current C or G2, close PR178 or close a TODO. Formal counts remain163 done /29 remaining.

From a checkout containing this directory, restore any one original ZIP with:

```sh
python3 restore_original_zip.py manifests/11617588539.json --output /tmp/11617588539.zip
```

The output must not already exist. The unchanged restoration helper validates every chunk's bytes, SHA256 and Git blob identity, then the whole original ZIP SHA256. It performs no network access, extraction or native measurement. Run it from this directory or pass its full path and the corresponding manifest path.

`catalog.json` is the complete current12-original inventory. `evidence/artifact-local-inventory-at-cutoff.json` is retained verbatim as the earlier7-present snapshot; the later five-capacity receipt explains the completed intake. `evidence/terminal-review.json`, the accepted subset and the original failure/aggregate logs retain their distinct scopes and original identities. The actual publication commit/ref is bound by the external publication receipt, avoiding a self-referential commit claim inside this tree.
