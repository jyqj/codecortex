# Replay of existing ffdc engineering and mechanism CI artifacts

This package adapts the two already fixed 9ebe independent audit scripts at candidate commit `ffdc6f0f97db78cc25a6c026904e7c2adde05d14`. The complete original scripts remain in that commit. `portable-adaptation.json` records every exact replacement, original Git blob/SHA256, new SHA256 and raw log provenance. The verifier reconstructs both scripts from the original candidate Git blobs and checks unchanged function ASTs and all archived raw-log bytes.

This package has **not yet executed against the new ZIPs**. A successful syntax/adapter check is not product acceptance. A successful archive replay verifies existing actual execution evidence and does not rerun the product.

Use Ubuntu Python 3.12 or another Python >= 3.9 with the standard library and Git. No Cargo, third-party Python package, provider, downloaded executable or product code is invoked by these auditors.

Required environment:

- `P7_AUDIT_SOURCE_ROOT`: candidate Git checkout containing full objects for both fixed PR head `ffdc6f0f97db78cc25a6c026904e7c2adde05d14` and actual engineering merge `683d8882c108e4e9be68ebac127f3c709e329711`; their expected complete tree is `9e59b41540eb3769e9ca0a787c9ed60441259d20`.
- `P7_AUDIT_ZIP_ROOT`: directory containing the exact official archive files below.
- `P7_AUDIT_OUTPUT`: new writable receipt directory outside the candidate checkout.
- `P7_AUDIT_EXPECTED_WORKER_PID`: one explicit positive PID from the separately recorded archive preflight; all twelve real resource-stage records must equal this same PID.
- `P7_AUDIT_EXPECTED_MECHANISM_MEMBERS`: explicit original ZIP member count from the separately recorded preflight.
- `P7_AUDIT_EXPECTED_MECHANISM_EXPANDED_BYTES`: explicit original ZIP uncompressed byte sum from the separately recorded preflight.

The last three values are mandatory and fixed to 12783, 4163 and 368834949 respectively; there is no default or inference in the auditors. They were independently read from the actual preflight job 113160685102 of run 37731271464. They are run identity observations, not changed acceptance budgets. Preserve the preflight receipt used to select them.

Archive inputs:

| File | Artifact ID | Bytes | SHA256 |
| --- | --- | --- | --- |
| ci_ffdc_engineering_artifact.zip | 11529288057 | 220992 | 8ac8223cc3278e4316fa83050ad40850985303f7041aa53919fb94650bdcb6d0 |
| ci_ffdc_mechanism_artifact.zip | 11529353250 | 91141978 | c31b0656a8e7e4e091710d2c43b1d399c5b36820bd1ffeb97a373005ff052233 |

Run sequentially from this package directory (or use absolute script paths):

```sh
python3 verify_portable_adapters.py
python3 review_ci_ffdc_engineering.py
python3 review_ci_ffdc_mechanism.py
```

Preserve the exit status and raw stdout/stderr of each command. The mechanism script consumes the successful engineering receipt in the output directory and the same exact engineering ZIP. The candidate raw job logs are already part of this package, including their original BOM, and are read directly beside each script.

Original checks are preserved: 384 worker samples, unchanged watchdogs/queue/FIFO lifecycle and CPU/RSS attribution, full 795 current crate/Cargo input hashes, four separate source/binary/target/receipt inventories, actual 36 queries, equal inputs/budgets, all positive and negative controls, source relabel rejection, exact score and paired bootstrap replay, and unknown provider billing/cache attribution. The original four-arm build paths under `/home/runner/work/_temp/` and recorded compiler hashes stay unchanged. Complete merge-tree equality and fixed raw-log hashes are additional identity checks.

Generated receipt names are `ci_ffdc_engineering_independent_receipt.json`, `ci_ffdc_mechanism_independent_receipt.json`, and `ci_ffdc_mechanism_archive_inventory.json`. Neither an Actions step green light alone nor this prepared package closes a TODO. Original dependencies, current full regression, integrated review and task acceptance remain root-owned.
