# Independent parser → public qname review: REJECT

Product: `4e3e5355d8e4c642574eb84bdf73b7373bf3c4a7` (PR 132).
Baseline: `88f2cf099c8b81f3acef485fd5ac9b01c63ce790`.
Review scope: Python lexical declarations, decorators and call owners; real source
boundaries, indexing, complete public engine/MCP output and unchanged native
micro-gold scoring. This is an independent new fixture, not the author's 17/497
receipt or public DEV. Product implementation, Cargo.lock, evaluator/scorer/gold,
ranking/budgets and task statuses were not edited.

## P1: decorated classes publish function identity and lose class results

Minimal independently authored input:

```python
@decorate
class Outer:
    @decorate
    class Inner:
        def pulse(self): return 1
```

Both `search("Outer")` and `search("Inner")`, using the real engine and the real
MCP JSON-RPC backend, demonstrate this regression:

| Fixed snapshot | Public results (name / kind / qname) |
| --- | --- |
| base | Outer / class / absent; Inner / class / absent; pulse / method / absent |
| product | Outer / function / Outer; no independent Inner or pulse hit |

The product's Outer function hit contains the entire decorated class source.
This is not safe omission: it publishes a false function declaration identity
and changes existing public kind taxonomy/results. Complete outputs are retained
in `baseline/decorated-class-public.json` and
`current/decorated-class-public.json` (each contains engine and MCP envelopes).

Root cause: `python/mod.rs:845` unconditionally passes any decorated definition
to `extract_function`; that helper accepts a class's name, defaults its missing
parameters to `()`, and synthesizes `def Outer()`. The real class is also emitted
at its inner-node coordinates. `handle_class_def` routes decorated class members
through `handle_member_function`, similarly synthesizing a Method for Inner.
These bogus parser symbols are preexisting. The new owner-first hint lookup in
`chunker/boundaries.rs:236` now selects the wrapper's bogus Function/Method
instead of the actual AST class's symbol; line 282 trusts that hint's kind. The
source-bound association proves exact endpoints and SQL survival of the wrong
parser declaration. SQL consistency alone therefore does not establish truthful
AST declaration identity. The baseline/current public comparison establishes the
new externally visible regression, distinct from the preexisting parser defect.

Required product correction (not made here): dispatch decorated classes as
classes, preserve exactly one canonical wrapper class envelope, distinguish
class AST declarations from function AST declarations before promoting a hint,
and keep nested class/member results. Re-run both rejecting tests and this
engine/MCP comparison; correct class qname may be published only after complete
class proof. Do not normalize this failure by changing taxonomy or gold.

## Bounded independent receipts

| Check | Result |
| --- | --- |
| nearest declaration, conditional/local/nested classes, static/class/property/async Method; nested local Function/no receiver | PASS |
| Unicode identifiers, original CRLF bytes, decorated method single wrapper, canonical local/method call ID/UID | PASS |
| real malformed/same-line grammar, real Unknown fallback, explicit unknown capability and absent structure | PASS: no identity |
| decorated class canonical class-only parser declaration | REJECT: Outer has Function + Class symbols |
| full index → public split method/local function/SQL-surviving repeated name | PASS: real split chunks, exact persisted owner and symbol; replaced first duplicate omits qname |
| decorated class safe public omission/class kind | REJECT: Function instead of Class |
| own normal methods, base/current real engine + real MCP → unchanged native scorer | PASS: missing=0, correct=1, wrong=0 recall10 |
| every original engine/MCP hit field and metadata against actual same-input base | PASS: all 5 hits; includes scores/traces/source/document identities on both APIs |

`parser-raw.log` preserves the actual 3 PASS / 1 failure run;
`public-raw.log` preserves the actual 1 PASS / 1 failure run. Tests deliberately
remain red on this product. `public-compile-raw.log` retains this review's initial
usize SQL binding compile error; `public-fixture-lifetime-raw.log` retains the
initial tempdir lifetime error. These were corrected only in the new review tests;
those initial errors are not product findings. Final native build logs and full
outputs live under `baseline/` and `current/`. `field-parity.log` records the
independent field comparison. Native scores were never made by filling metadata;
wrong qname is a wrong independent gold alternative evaluated against unchanged
actual output. All normalized hits were checked against original source bytes.

## Reproduction and scope limits

Direct official Rust 1.95.0 was downloaded from static.rust-lang.org and installed
under `/workspace/scratch/qname-review/rust`. Official Cargo.lock and `--locked`,
normal inherited platform network/proxy, owned Cargo/target/index caches were used.
No mirror, env-i or permissions change. `.agents` is empty in this environment;
no repository AGENTS.md/.agents skills were present. The QNAME source identity
design and implementation README were read. An initial attempt to list root's
Rust paths received Permission denied; that path was stopped and no escalation
or alternate access to those denied files was attempted.

Run new tests with the direct Cargo binary, RUSTC/RUSTDOC pointing to that direct
installation, CARGO_HOME/CARGO_TARGET_DIR/CODECORTEX_CACHE_DIR pointing to owned
scratch directories:

```
cargo test --locked -p cc-index --test independent_qname_parser_public
cargo test --locked -p cc-server --test independent_qname_public_boundary
python docs/reviews/qname-parser-public-20261003/run_native.py BASE_CHECKOUT OUT_BASE baseline
python docs/reviews/qname-parser-public-20261003/run_native.py PRODUCT_CHECKOUT OUT_CURRENT current
```

`run_native.py` compiles its standalone driver against each real snapshot's
cc-eval library without editing cc-eval; it creates a fresh owned temporary
fixture each run. Its checked-in direct-toolchain paths document this run's
layout. `compare.py` reproduces field parity and the decorated-class counterexample.
`source-manifest.json` records fixed SHAs and hashes of review inputs,
outputs and product sources relevant to this review.

This bounded review does not certify every language, every fallback parser,
every same-line grammar, lifecycle or transaction scenario. SQL transaction adversity
belongs to the separately assigned worker. No old excluded semantic runtime,
broad runtime, GC/WAL/kill/staging/denied42/private localdiag scenario was run.
No public DEV, new gold, frozenDEV, scale/provider/release workload was opened or
executed. No throughput certificate, merge, deployment, V19 or parent closure.
The decorated-class P1 blocks acceptance despite the other bounded passes.
