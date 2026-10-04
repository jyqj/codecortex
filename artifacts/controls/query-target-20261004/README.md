# Bounded query-target boost correction

Original public metrics remain **FAIL**. This control study is not an independent
public verification or a claim that broad natural-language search is repaired.

## Scope and frozen model

Base `37dd042eaa1209a86e0cafdcd92ae77e036e76f5`; independent controls frozen in
`443ca67` before any production edit. `frozen.sha256` binds the original matrix,
source fixtures and model. Only the diagnosis README and its causal micro-driver
were read; no public DEV queries, gold, raw cases or scorer inputs were used.
No AGENTS.md or local `.agents/skills` exists in this checkout or workspace.

`cc-search/src/query_target.rs` recognizes these **whole-query** forms:

- `class NAME`, `interface NAME`, `type NAME`, `function NAME`, `method NAME`.
  The kind is product taxonomy: `type` means `type_alias`; Go structs are `class`;
  Rust impl members are `method`. A mismatched kind receives no exact-name bonus.
- `method NAME on CONTAINER`: only NAME of kind method receives the name bonus.
- `methods on CONTAINER`: no specific member name is known, so no name bonus.
- `Container.member`, `Container::member`, qualified chains, `(*Container).member`:
  only the last name receives the bonus. A member may be a field or a callable.
- Existing `name:` DSL evidence takes precedence; `kind:` remains the existing
  hard kind filter. `name:Ignite Lantern` now correctly grants the name bonus to
  Ignite even though the remaining lexical text contains only Lantern.

Everything else retains historical token matching, including bare identifiers,
multiple-name prose, explanatory prose, task/conversation augmentation, and
sentences containing code-like fragments. This intentionally leaves the broad
prose receiver regression unresolved. No inferred intent from capitalization,
public tokens, language names or library names. Container/qualifier syntax is a
boost hint, **not** a hard receiver constraint or a resolver: an unrelated symbol
with the same final member name can still receive the bonus. `methods on` does
not enumerate methods or guarantee that a method outranks the container.

`SearchPlan` parses the primary DSL once and gates only `boost:symbol-exact`.
Its +0.18 amount, all other components, lane budgets, parser identities, filters,
normalizer and scorer are unchanged. The exact-target identity tier and its
historical bonus eligibility are preserved separately. Accurate type fragments
remain eligible for direct type/class searches; none is globally demoted.

## Actual fixture evidence

`before/results.json` and `after/results.json` retain all 28 controls through
real CodeIndex build/search, real in-process MCP wire backend, unchanged Rust
normalizer and fresh source verification. No custom ranker/scorer is used.
`comparison.json` retains the complete per-control comparison. Representative
results (engine and MCP agree on the retained prefix):

| Control | Before | After |
| --- | --- | --- |
| `method Ignite` | Ignite class first, .467919; method .467293 | method first, .467293; class loses .18 |
| `class Ignite` | class first, .468333 | class first, same score; method loses .18 |
| `Store::refresh` | method .811909; Store .801568 | method unchanged; Store .621568 |
| `method arrange on Shelf` | method .725607; Shelf .657526 | method unchanged; Shelf .477526 |
| `class Reservoir` | several accurate class fragments | same fragments/scores; direct search preserved |
| `method Drain on Reservoir` | Drain first; Reservoir fragments boosted | Drain unchanged; fragments lose .18 |
| `methods on Lantern` | Lantern first, .487525 | Lantern still first, .307525; other signals can dominate |
| own ambiguous lighting prose | Lantern .458336; Ignite method .446187 | byte-identical hit projections; broad prose still fails |
| `name:Ignite kind:method` | exact-target method first | identical, preserving exact tier |

The direct literal `Ignite` deliberately collides with a type: both remain
exact-target candidates and the class still wins. Qualified syntax removes the
receiver bonus but does not disambiguate same-name kinds. `Lantern.Ready` finds
no property chunk; suppressing the receiver bonus does not manufacture one.
`function refresh` is intentionally kind-mismatched against the parser's Rust
method identity; its bonus is removed, and the method stays first on other
signals. These limits are retained in the actual results.

`python3 check.py` verifies frozen bytes, 168 retained-hit model eligibility
checks, all returned source evidence, 475 score traces using the production
1e-9 tolerance (max error 1.1102230246251565e-16), and 237 common hit projections
whose identity/body/fused score and **every non-name trace component** remain
exactly unchanged. 45 trace changes across both APIs affect only the name bonus.
The one transport difference is the **before** split-context control: engine
retains 6 hits and MCP 5 due to existing packing limits; the common prefix is
identical. All other arm/control results match between the APIs. Packing is
still Partial where reported; this change does not certify complete retrieval.

## Validation and reproduction

Official Rust/cargo 1.95.0, original Cargo.lock, default features, `--locked`.
The environment has the installed toolchain under `/workspace`; Rustup's initial
default home lookup failed before compilation, and subsequent commands explicitly
selected that existing installed home. No permission escalation, alternate remote,
model identity prototype, public rerun, excluded post_index runtime test or broad
suite was used. Four selected `query_target` unit/integration tests pass, along
with formatting and diff checks. See `receipts/` and `binding.json`.

From this checkout, compile the actual library and standalone driver:

```sh
export PATH=/workspace/.cargo/bin:$PATH
export CARGO_HOME=/workspace/.cargo RUSTUP_HOME=/workspace/.rustup
cargo +1.95.0 build --locked -p cc-eval --lib
# Exact rlib names used for these runs are recorded in binding.json.
rustc +1.95.0 --edition=2021 artifacts/controls/query-target-20261004/driver.rs \
  -L dependency=target/debug/deps \
  --extern cc_eval=target/debug/deps/libcc_eval-c10568ffeac0e155.rlib \
  --extern cc_server=target/debug/deps/libcc_server-54f8c2aefb185b1c.rlib \
  --extern serde_json=target/debug/deps/libserde_json-c74784839d633be7.rlib \
  -o /tmp/query-target-driver
# Driver requires two new destinations; never overwrite retained evidence.
/tmp/query-target-driver artifacts/controls/query-target-20261004 \
  /tmp/query-target-fresh-fixture /tmp/query-target-fresh-output
cargo +1.95.0 test --locked -p cc-search query_target --lib
python3 artifacts/controls/query-target-20261004/check.py
```

Before was built at the frozen-control commit (production equal to base). After
was rebuilt after module/plan integration. Reproduction on another build must
use that build's actual Cargo rlib outputs, not assume stable hash filenames.
