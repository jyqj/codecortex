# Express / Requests current public dev admission

Base protocol3babaa5. Frozen authors: Express465e9e0bd435e2e30c08de8702f78a0d10c49c8e and Requests e49f9ada5826b4206d2128f3bfb8d31603ff42fa. Independent source/content reviews: Express0c0ec9e8906493373faf800ca37b78c589f38324; Requests c2026ebad0b6037f1c30b7d2b34055e211849b70 and3aae8bf2a690213426af91427dc837fedbe50c83. `evidence/admission.json` locks62 exact input file SHA256s, actual evaluator/build identity, source/query/license/review records and current suite entries. No author shard, gold, scorer, production, registry or main ledger changed.

## Bounded decision

Both current source/suite sets are admitted **as development snapshot corpus**, not a formal held-out quality result. Express70native/59compat, Requests91native/83compat:161 native dev records,142 compat projections. All four actual evaluator validates and both own protocol checks pass. All27 input source files match source manifest sizes/hashes; Express additionally matches recorded Git blob identities.251 gold spans pass admitted-file/byte/UTF8 checks. Root licenses match immutable upstream locks; Requests9 retained BSD-source/notice files match independent lineage-review hashes, all3 helper normalized lineage witnesses are true. Helper source bytes may be indexed with their full BSD notices retained; those helpers remain excluded from current gold and checker verifies zero overlap with their excluded gold spans. This does not claim a specific copied historical release/private permission agreement. No new official-source downloads were needed.

Express independent repair review locks the exact current70-row native and59-row compat files and verifies8 repaired plus62 unchanged content accepts. Requests joins current canonical query/gold hashes against independent accept records:19 initial,20+20+15 remaining old rows,17 delta (one revised+16 new), covering91 current IDs exactly. Old f0022 failure is preserved in author/reviewer history; current corrected row alone is counted as current accept. Review75 and80 are not assumed ancestor-related: four old review record files are independently byte-identical at the two frozen commits. Pending author annotations are not rewritten into self-signed acceptance; this sidecar is the independent development admission decision.

Source roots use `commit=null` snapshot locks. Express original-checkout/Git-origin proof and Requests fixed archive provenance are retained, but neither current snapshot is claimed a verified clean Git checkout. Development admission is source/input/provenance/content consistency, **not retrieval capability or ranking quality**, and not a six-repository corpus completion claim. Formal600 accepted-family count, complete confirmatory20-family blocks and clean holdout remain0; custody blockers persist. Gin and TypeScript are outside this two-repo block, even though later separate review metadata is available.

## Global task / template / fact audit

Read all161 **current explicitly public dev** query/gold records and selected admitted source facts. Checked6370 cross-repository pairs for normalized exact query duplication:0. Existing reviewed local components67+82=149 are preserved. Ten targeted source/task pair decisions distinguish actual source obligations, shared API/trace templates and merely shared HTTP vocabulary. No cross-repository **same-task equivalence** was proved. Two shared-fact associations are conservatively unioned for leakage/correlation analysis; this produces147 global correlation components, **not147 certified independent questions**. Their registry SHA256 is `5b1a55296d9cdd6e43bf1998469d281f3fd83b02ba69eabcf8d5dbdd1802381d`. It does not prove full semantic independence or unseen-repo generalization.

`cross-repo-decisions.json` contains opaque dev family IDs, reason codes and exact current record/source-span hashes only; no question/gold/source excerpts. `global-components.json` is the sidecar correlation registry. Generic templates or common HTTP words alone are insufficient to claim duplicate tasks. Full corpus/global component adjudication, including other repositories and blocked/unread components, is still open. Do not rewrite existing rows, scoring denominators or split merely to adopt this sidecar; future preregistered aggregation must explicitly bind it. Already public dev never becomes clean holdout by new ID or alias.

## Reproduce without ranking or protected-body reads

Fetch only the five authorized frozen repo commits above if not already present. The checker reads a hardcoded allowlist of current dev suites/query files, admitted source files, source/license metadata and body-free independent review records. It does not checkout/read old, isolated or held-out question/gold bodies, execute author scripts, call providers, run search or produce ranking scores. Relation records may carry opaque IDs outside current dev; component projection ignores those unseen nodes, never opens their bodies. Public results contain hashes/counts/error codes and current dev suite entry paths.

```sh
python3 crates/cc-eval/benchmarks/public-v19/protocol/global-dev-review/check_admission.py \
  --evaluator <exact-receipted-cc-eval-binary> --output /tmp/v19-dev-admission
```

Default evaluator binding uses the existing exact offline build receipt in `artifacts/checkpoints/v19-corpus-audit-20261003/validation.json`. For another workspace, build locally offline and retain its actual Cargo compiler artifact:

```sh
python3 crates/cc-eval/benchmarks/public-v19/protocol/global-dev-review/build_validator.py --output /tmp/v19-eval-build.json
python3 crates/cc-eval/benchmarks/public-v19/protocol/global-dev-review/check_admission.py \
  --evaluator <binary-reported-by-build-helper> --evaluator-build-receipt /tmp/v19-eval-build.json \
  --output /tmp/v19-dev-admission
```

The checker verifies the binary SHA256/actual compiler target/build exit and exact evaluator source/Cargo compatibility against frozen83a6b54; a different source/schema is blocked. A rebuilt compiler artifact has its own receipt, not a borrowed old binary hash. This route was actually exercised successfully with a fresh offline Cargo command, captured in `new-build-receipt.json` / `new-build-admission.json`. Original-build replay reproduced `admission.json` byte-identically. No production runtime or ranking was invoked.

Eight meaningful tests pass: real four-suite scoped admission, source/root-license/BSD-notice tampering, incomplete independent acceptance, non-dev split drift, refusal before loading a non-allowlisted query file, and transitive component counting. Synthetic mutations run in temporary read-only-derived layouts; author corpus remains untouched.

Initial receipts are retained: first prototype wrongly treated formal/global scope-limit codes as content errors; corrected development gate permits only the three explicit unclosed formal scope codes and still leaves formal counters0. A second prototype incorrectly demanded PR75 ancestry in PR80; replaced with exact byte equality of their four independent frozen review record files. No question, gold, source or author assertion was relaxed/edited to pass either check. Actual final receipts have0 errors.

## 后续 Gin 差量

原两 repo frozen block 保持不变。可选三 repo 复现与计数见 [gin-extension/README.md](gin-extension/README.md)；运行时显式传入 `--include-gin`。
