# V11 cross-build admission review — blocked, not green

Fixed subject: PR58 source `83a6b54ab1e71db033264e3b4e8d4f0a1d5319ad`.
This evidence-only block changes neither production nor Cargo nor the gate ledger.

## Result and authority

Both **V11/derived** and **V11/frozen-spec remain `not_run_cross_build`**.
No migration build, binary hash, runtime invalidation result, old pinned-handle
result, or reopen result is claimed. `admission-inventory.json` explicitly records
zero builds/tests and null binary hashes. Two distinct source revisions are not
two distinct admitted parser/rule/encoding versions.

The authoritative remaining-gates table at the fixed subject, lines 25–26,
requires "Two isolated fixed builds with admitted version transition" and says
"Future alternate version requires cross-build invalidation, not manual test key
mutation". C12 (`02-CONTRACTS.md`, lines 92–107) requires seed plus
parser/project-model/resolution versions and chunk source document versions.
ADR-0003, P6-003/P6-014, requires complete encoding identity and incarnation
fencing. These requirements preclude declaring this source review a runtime pass.

## Reproducible findings

Run from any directory in this checkout:

```sh
python3 artifacts/checkpoints/cloud-cross-build-admission-20261003/verify_admission.py --check
```

The script reads immutable Git objects, including every ancestor of the fixed
subject touching the ten named admission source paths. It saves exact source SHA,
tree SHA, Cargo.lock SHA-256 and individual file SHA-256; missing historical files
are represented explicitly. The evidence covers committed reachable history at
this subject, not unavailable uncommitted/local versions or future branches.

- Parser export labels, project-model v3, resolution v1, document v1,
  `source-chunks-v2` and `utf8-bytes-div-ceil-4-v1` first enter committed history in
  `0a56a257f9a92c54d06ea5be0ce1d1763917a527`. Earlier committed v1/v2
  project-model or parser label variants are unavailable, despite the v3 spelling.
  No subsequent recorded admission version change exists in this ancestry.
- Frozen encoding v1/Cosine first enters in
  `ff458bc591b4e7e444af4464d6eef2513cdb335c`; it remains v1/Cosine at PR58.
  The historical checkpoint itself reports compilation errors in its commit
  message; it is not asserted to be a runnable migration baseline.
- Schema 21 at `0a56a257…` and schema 22 at PR58 are real committed versions.
  Current `index_migrate.rs` admits adjacent 21→22 migration in place. This pair
  cannot prove an incompatible parser/rule/encoding transition. It also cannot
  substitute for the separately required old pinned-handle test.
- `spec.rs` accepts bounded tokenizer **identity strings** in standalone specs;
  this is not executable support for arbitrary tokenizers. Production assembly
  stamps `TOKEN_ESTIMATOR` (`semantic_wiring.rs`, lines 309/319), and
  `admission.rs::tokenizer_gate` rejects all other estimation bases (lines 220–231).
  Changing an identity string in a test would not constitute a real tokenizer
  migration.
- `spec.rs`, lines 24–27 and 77–83, explicitly requires a spec-version bump for
  new metric variants/admitted values; `VectorSpace::new` stamps the sole current
  version. No alternate metric/version execution path is admitted today.

Unsupported alternate metrics/tokenizers are **nonapplicable to the current
supported feature set**, but the ledger's future migration requirement remains
**strictly not run**, not closed or globally nonapplicable. Existing source-change,
query instruction/model/dimension and synthetic schema-reset receipts remain
their own bounded evidence; none is repeated or promoted here.

## Executable next block once a real transition is supplied

The parent should obtain a product-owner-approved next parser/rule or frozen-spec
version with its actual implementation, rebuild/migration policy and sanctioned
consumer. This is a prerequisite, not authorization to invent support. Production
ownership is required for that work; this worker requests no production files
merely to fabricate a validation scenario.

1. Pin the last compilable supported version and its real successor by full Git
   SHA. Use detached worktrees and separate target/cache roots. Build each with
   `cargo +1.95.0 build --locked -p cc-server --features semantic`, preserving
   logs, toolchain/lock/fixture hashes and SHA-256 of every executable. Resolve any
   unavailable historical dependencies through the official build toolchain;
   never patch old admission constants or fixture keys.
2. Add the same dedicated helper integration test to each isolated build (test
   source hash recorded; no production patch). Use public CodeIndex indexing and
   QueryHandle capture to create/warm real file/symbol/directory/chunk consumers
   in build A; retain its actual Arc-owned handle. Synchronize using bounded IPC
   barriers while build B opens/indexes the shared fixed fixture through its
   sanctioned migration path. Do not simulate an old handle with a serialized key.
3. Compare the retained A handle, fresh B handle and B reopen against fresh B
   authoritative results. Record generations, document versions, fixture bytes,
   derived outputs, cache observations and errors. Require the specified migration
   safety semantics: no old-generation result labeled current, no old document
   attachment after transition, and correct cold/reopen results. An old handle's
   historical result or rejection must be interpreted using the declared pin/fence
   contract, not an invented universal rejection requirement.
4. For an actually admitted new metric/tokenizer/encoding version, route real
   deterministic offline provider/admission/encoding consumers through both builds;
   verify namespace/spec transition and manifest fencing/reuse rules. Compare
   warm consumption with fresh computation; capture reopen and old in-flight
   publication behavior. Keep each case's actual supported feature boundary.
5. Only then provide a runtime receipt for parent ledger reconciliation. If the
   successor is unavailable or the old checkpoint cannot compile without changing
   its production implementation, preserve `not_run_cross_build` with raw failure
   evidence instead of replacing it with a manually bumped key.

## Validation performed here

The admission inventory was regenerated and checked twice byte-for-byte from
committed objects. This validates evidence provenance and the stated lack of an
available admitted transition; it does not validate cache runtime behavior. No
network provider, credential, query gold, production source, Cargo file, public
CI configuration, or task/gate ledger was modified.
