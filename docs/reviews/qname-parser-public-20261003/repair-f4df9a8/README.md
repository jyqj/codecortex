# Repair follow-up: original P1 CLOSED; new P2 REJECT

Frozen repair source `f4df9a83514e6ba901143547368ce4a3bee21940`, evidence head
`dbeefb478a996b7dd093b3c2eddde571e2c68178` (PR136). Independent comparison
against original product `4e3e5355d8e4c642574eb84bdf73b7373bf3c4a7`.
Only new review tests/evidence are authored here; the product fix is inherited.
Original review README, REJECT/logs/outputs remain byte-for-byte unchanged.

## Original P1 is closed on this target

The original independently authored four parser tests now all pass. New stronger
public acceptance uses the exact original Outer/Inner/pulse fixture plus a
Unicode/CRLF, two-decorator, conditional nested-class, staticmethod variant.
Every real class/member hit must have the correct Class/Method, dotted qname,
original snapshot/slice, both owner endpoints, exactly one surviving SQL symbol
with that kind/name/qname/envelope and persisted identity row with the same
qname/kind/owner. Real engine and MCP outputs independently reproduce all three
original Class/Class/Method results with Outer / Outer.Inner / Outer.Inner.pulse.

A separate new parser case checks decorator calls/references in enclosing scopes,
canonical local class parent IDs, one Class/Method/Function per lexical qname,
and nearest-function call ownership. Small local classes inside a whole-function
chunk are checked as actual AST boundaries; no independent public class chunk
is invented. Leading documentation can generate multiple chunks for one class:
the Unicode fixture has two Outer hits with the same truthful class proof.

The original public absent-qname test is retained verbatim at
`../../python-declaration-fix-20261003/original-tests/independent_qname_public_boundary.rs`
and in the original Git history, along with its old raw failure. Its obsolete
active copy is removed from this review branch; none of its assertions was
rewritten. The new acceptance requires positive correct proof, not omission.

## New P2: C++ rejected Method hint loses the actual declaration name

Independently authored minimal real input:

```cpp
namespace grove { template<class T> T leaf(T x) { return x; } }
```

| Target, real engine and MCP `search("leaf")` | Public hit |
| --- | --- |
| original product | leaf / method / grove::leaf |
| repair | T / function / no qname |

The old C++ parser wrongly treats namespace-contained free functions as Method;
that parser classification is preexisting and unchanged here. Correcting a
boundary to Function is reasonable, but replacing its actual name with return
type **T** is a new public name regression. Search still retrieves the actual
source bytes, yet native identity matching loses the real function name.

`compatible_hint` now rejects the namespace Method hint. In
`chunker/boundaries.rs:315`, the fallback uses the function_definition's `type`
field (`T`) instead of extracting its declarator's name (`leaf`). The real parser
symbols remain unchanged. No authority is available for qname after rejection;
omission alone would be safe, but publishing T as the symbol name is not.

A new strict test requires the actual leaf boundary with Function taxonomy and
rejects a callable boundary named T. It fails on this fixed target. Complete
before/current engine/MCP outputs are retained in `*/cpp-public.json`; each was
obtained through production indexing/query and real MCP JSON-RPC dispatch.
Independent name-only C++ micro gold intentionally does not request disputed
preexisting Method taxonomy or qname. The **unchanged** native normalizer/source
verifier/scorer gives recall10 **1 before / 0 after** on both APIs. Gold/output
were never patched or injected. `cpp-micro-gold.json` and `cpp-native-scores.json`
retain the actual inputs/results. The scorer verifies every hit against the real
fixture bytes. This is a diagnostic of the new regression, not a product PASS.

Required repair: derive a truthful C/C++ declarator name when incompatible hints
are omitted; do not use return type as a function name or relax gold/kind guards
to hide the regression. Native C++ parser taxonomy remains a distinct preexisting
limitation. Production code is intentionally unchanged by this independent review.

## Independent bounded receipts

- Original parser suite: 4 PASS (`closed-p1-tests.log`).
- New decorator/call ownership and real-language snapshot tests: 2 PASS.
- New strict public class/proof test: 1 PASS (two fixtures).
- New strict C++ function-name guard test: 1 REJECT (`new-tests-raw.log`).
- Before control: 1 snapshot test PASS on original product (`before-raw.log`).
- Actual ordinary-method native engine/MCP micro gold: correct=1/wrong=0;
  every field of all five hits exactly equals original product on both APIs.
- New/old input comparison spans Rust trait/default method/local function,
  C++ qualified out-of-class member/template free function, grouped Go
  struct/interface/alias and named type, TS namespace/callable variable/value,
  JS named function-expression/generator/getter/static async method, and C struct.

All six language fixtures preserve parser symbols. C/JS/TS chunks and admitted
identities are identical. Go changes unnamed boundaries to Oak/Named and Named's
Class to TypeAlias; Rust trait default method boundary becomes Method. Admitted
identities are unchanged for both. C++ has the name/type regression above and
loses grove::leaf identity. TS has no admitted identities in this namespace
fixture on either target; these snapshots do not claim public TS qname coverage.
`guard-before.json`, `guard-current.json` and `comparison.log` preserve the exact
observations, not just parser symbol listings. Author's 570 regression passes
remain inherited facts and are not counted as independent passes.

`compile-raw.log` retains this review's corrected test formatting compile error;
`single-chunk-assumption-raw.log` retains the corrected review assumption that
one declaration must have exactly one chunk. SQL symbol uniqueness and positive
identity remain mandatory for every returned chunk; this correction accommodates
actual documentation partitioning, not duplicate declarations. Final logs above
are separate. The historical rejected tests/logs were not overwritten.

Direct official Rust1.95, original Cargo.lock/--locked, normal inherited network
and owned caches continued from the first review. New Rust files pass rustfmt.
`run_native.py` compiles the standalone new driver against each snapshot's
unchanged cc-eval; `compare.py` reproduces both closure and regression checks.
The pre-fix control uses an isolated worktree with only the new snapshot test.
`source-manifest.json` binds fixed targets, product files and evidence hashes.
No public DEV/new frozen gold, scale/provider/release, excluded old runtime,
GC/WAL/kill/staging/denied/private paths, DB worker scope or remote PR merge/deploy.
V19 and parent items stay open. Overall acceptance remains REJECT due to new P2.
