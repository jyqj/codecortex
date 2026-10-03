# Author checkpoint: public-v19 protocol v1

Base PR60 `bc8e22bd6b4f85774e7b84a3e6a30da8dc4a3b29`. Exclusive protocol directory; other owners retain their six exclusive repo directories. This checkpoint is a format/analysis preregistration, not a corpus acceptance or ranking result.

**Use actual evaluator schema v1.** `evaluator-query.schema.json` is copied byte-identically from the base. Top-level extra keys are rejected. Use native `answers`, set `expected_files=[]`; no-answer has both arrays empty. Compat requires `answers=[]`, nonempty ordered `expected_files`, and cannot contain no-answer. Native and compat need separate suites/query files; do not make one hybrid row. Put all extra metadata under `annotations.v19`, not top-level. Examples below are invented protocol examples, not public corpus questions.

```
repo directory: public-v19/{serde,express,typescript,requests,gin,vite}/
query_family: v19.<repo>.f0001        # four-digit opaque serial, stable forever
id: v19.<repo>.f0001.en01            # language tag + two-digit variant
native query file: queries.native.dev.jsonl
compat projection: queries.compat.dev.jsonl
source receipt: source-manifest.json
review receipt: review-receipt.json
public counts/hash receipt: corpus-receipt.json
```

Reserve family serials monotonically before drafting, never choose/rename serials based on split or scores. Category uses one of the ten names from09-BENCHMARK. Difficulty is1/2/3. Language describes source language; annotations.query_language describes query language. Source paths are canonical repo-relative POSIX paths. Spans are **UTF-8 byte offsets [start,end)**, not lines, codepoints or chunk IDs. Native groups have grades1–3, explicit alternatives and at least one primary. Primary grade3, supporting grade1; grade2 for required secondary evidence (pre-ranking source decision). Each required facet maps to a specific answer-group id; alternatives are substitutes, groups are separate required evidence. Freeze that distinction before retrieval.

Minimum extra metadata in every row:

```json
{"v19":{"protocol_version":1,"repo_id":"serde","source_sha":"6693a89cca77e0151437da1c7f890090b9ebf04c","global_family":"v19.serde.f0001","intent":"source-derived task description","query_language":"en","hard_scope":{"path_prefix":null},"facets":[],"graph_constraints":[],"mutation_profile":"none","review_status":"pending"}}
```

`annotations.v19.global_family` is the leakage-component key. Initially it equals query_family. Link source-equivalent tasks, translations, paraphrases, positive/negative counterparts and template-derived same-task questions into one global component before freeze. Mere shared words or broad concepts do not establish equivalence. A designated global reviewer assigns the lexicographically smallest member family as canonical key; authors do not independently relabel existing cross-repo components. `relations.json` lists those components when supplied to the checker. Unrelated families may stay singleton. Duplicate question text in unrelated components fails automated review; semantic duplication still needs independent source review.

**Deterministic proposed75/25 split:** SHA256 of UTF-8 `codecortex-public-v19-split-v1\n` + canonical global-family key; first64bits big-endian <2^62 => holdout, otherwise dev. This is a fixed25% probability partition, **not an exact quota**:20/100-family counts can differ from5/25. Do not rebalance, salt IDs, discard hard holdouts or retry keys to hit quota. Every variant/component member receives the same split. Freeze associations first; later cross-split linkage quarantines the whole affected component and requires a versioned adjudication, never quietly moves seen dev into holdout. Preserve original records and counts.

**Holdout custody:** public commits and tuning-owner reports contain only source/license metadata, counts and exact-byte SHA256 commitments. Do not commit real holdout query/gold bodies to these public PRs or send them to the main tuning/integration agent. Keep holdout material in a separately authorized restricted custody location; hash/count receipt includes full private file hashes. Same-workspace files, public Git branches and an agent instruction are **not access control**. If no separate custodian/storage is available, report `holdout_custody_blocked`; do not claim untouched holdout. Authors know their own draft; designate independent reviewers/custodian, exclude authors who saw a holdout from tuning/evaluating that holdout arm. Independently review from pinned source, never product ranking. Review status pending != accepted; author self-signing is forbidden. Quarantine disagreements without deleting records. A twenty-family complete block means reviewed dev + hash-committed, separately held/reviewed holdout families; it does not mean twenty visible dev questions.

Source locks: use `source-locks.json` exact SHAs and primary license hashes. Root licenses do not cover all vendored content. Admit clean pinned checkout files only, explicitly inventory exclusions/licenses/generated dependencies. Source manifest records per-file bytes/hash/reason, SHA/submodules/dirty state. With actual Git source, evaluator `source.commit` is mandatory; gold/queries must remain outside source checkout. Copying a subset permits commit=null only as a clearly labelled snapshot, with original repo SHA independently locked in provenance, and never claim the snapshot passed Git cleanliness verification.

Evaluator locks are **BLAKE3**, not SHA256: individual source file bytes; source digest = BLAKE3 of compact Rust-serde ordered `{path,bytes,digest}` inventory sorted by path; query digest = BLAKE3 of exact JSONL bytes. Protocol/public custody hashes are separately named SHA256. Use the actual evaluator's `freeze` only on newly authored/versioned manifests after source/gold review; `validate` never refreshes locks. Capture source inclusion manifest and verify both digests; never replace SHA256 for BLAKE3 because both are64hex.

All author's changes stay in their repo namespace. Only integrator registers suites globally. No production/scorer/registry/ledger edits. No provider, remote ranking, upstream OCE uploader, secret reading, private-source egress or live semantic runs. Reviewers report holdout **hash/count/status only**. Formal metrics/statistics and margins are frozen in `PREREGISTRATION.md` and `preregistration.json`; unresolved scorer support is a blocker, not permission to improvise a new favorable metric.
