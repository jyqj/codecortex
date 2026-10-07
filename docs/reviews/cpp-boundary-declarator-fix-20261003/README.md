# C/C++ boundary declarator-name repair

## Root cause and bounded change

Base PR136 `dbeefb478a996b7dd093b3c2eddde571e2c68178`, product
`f4df9a83514e6ba901143547368ce4a3bee21940`. Independent PR134 final
`61390db1493718e1d1cb3c65e5061f91aaeedabb` identified the new regression in
`docs/reviews/qname-parser-public-20261003/repair-f4df9a8/README.md`.
This checkout has no AGENTS.md or .agents/skills instructions (searched workspace
and the base tree); `/workspace/.agents` is empty.

For `namespace grove { template<class T> T leaf(T x) { return x; } }`, the old
C++ parser labels the namespace free function Method. PR136's AST-family guard
correctly rejects that hint, but its fallback reads the return-type field `T`.
Fixed product **aa271e52b9c2aa52e05696af96ac52116963e57e** extracts `leaf` through
the function declarator, publishes Function, and leaves qname absent.

The only production edit is `crates/cc-parsers/src/chunker/boundaries.rs`.
The bounded walk follows explicit declarator/name edges through pointer,
reference, parenthesized, qualified and template declarators; identifier,
operator_name and destructor_name are supported terminals. It never walks into
return types, template arguments, parameter names or function bodies. Unknown
forms and exhausted depth omit names and reject hints. Conversion operators
(`operator_cast`) deliberately remain unsupported and omit their names.
Rust impl owners still use their actual type field. Existing kind compatibility
rules, including the prior qualified-member Method refinement, remain unchanged;
C/C++ hint names are now checked against the declarator too.

Native C++ symbol extraction/taxonomy is unchanged. Namespace free functions
still have the preexisting Method symbol; top-level qualified definitions may
still have native Function taxonomy. This patch neither repairs that broader
parser debt nor constructs a new qname/identity. A separate taxonomy fix needs an
owner-approved plan covering containers, qualified owners and persisted identity.

## Measured results

Rust 1.95.0, original Cargo.lock, `--locked`, default features:

| Scope | Passed | Failed | Ignored |
| --- | ---: | ---: | ---: |
| cc-parsers lib and tests | 252 | 0 | 0 |
| seven precise cc-index targets | 30 | 0 | 1 |
| four precise cc-server public targets | 11 | 0 | 0 |
| cc-search lib | 298 | 0 | 0 |
| Total | 591 | 0 | 1 |

The ignored index test is the existing `special_config_child` helper, exercised
by bounded FIFO parent tests. New four parser tests are included in the 252, not
counted twice. The two PR134 review files are imported unchanged in assertions
(the public file is rustfmt formatted); the original strict leaf test now passes.
`clippy.log` is `cargo clippy --locked -p cc-parsers --lib --tests -- -D warnings`;
`fmt.log` is `cargo fmt --all -- --check`. Both pass.

New tests cover C free functions returning named structs, namespace/template
free functions, nested namespaces and nested return types, qualified methods,
operators/destructors, template specializations, pointer/reference declarators,
functions returning function pointers, poisoned compatible-name hints, rejected
Method hints, legal qualified Method refinements and unsupported conversions.
On an isolated exact-base worktree, the same four new tests produce **2 pass /
2 fail** (`base-declarator-tests.log`); fixed tree is **4 pass / 0 fail**.
`initial-test-assumption.log` preserves the first development failure: the test
incorrectly assumed the existing top-level qualified parser records were Method.
The corrected test explicitly supplies the already-allowed Method refinement;
no production taxonomy was changed to satisfy that assumption.

## Real engine/MCP and unchanged original microgold

`native_driver.rs` is byte-identical to PR134's independent driver. Its `before`
argument selects the original C++ recall10=1 recovery assertion; it is run against
the fixed product, not relabeled as an old baseline. `run_native.py` builds the
real locked cc-eval/cc-server library and executes that driver. Real indexing,
engine search and MCP JSON-RPC dispatch are used. Every normalized hit passes the
unchanged native source verifier; no hit/output is injected.

| C++ name-only native recall10 | Engine | MCP |
| --- | ---: | ---: |
| Original PR134 before control (retained there) | 1 | 1 |
| PR136 repair rejected by PR134 (retained there) | 0 | 0 |
| This fixed product | 1 | 1 |

The C++ hit is `leaf / function / absent qname`, never `T / function`.
Original C++/Python correct/wrong gold bytes and driver are verified unchanged by
`compare.py`. Scorer, normalizer, source verifier, budgets and original fixtures
are unmodified. The Python ordinary-method gold still gives correct=1/wrong=0;
all five ordinary hits on both APIs match the previous repair's complete hit
fields, score/trace/source/document evidence exactly. Both decorated-class
fixtures' public Class/Class/Method qnames survive; imported strict public tests
also check exact source/SQL identity/owner proofs including Unicode/CRLF.

`guard-fixed.json` and `comparison.log` show native parser symbols unchanged for
all six languages. C/Go/JS/Rust/TS chunks and admitted identities match PR134's
repair snapshot exactly. C++ chunks change to the truthful leaf name; existing
admitted identity tuples stay unchanged and no leaf identity is invented.
Ancestor document versions reflect corrected structural input. Independently
created database envelope incarnation, lane timings and their serialized packing
bytes vary; whole-envelope equality is not claimed.

## Reproduction and preserved boundaries

Set PATH=/workspace/.cargo/bin:$PATH, CARGO_HOME=/workspace/.cargo and
RUSTUP_HOME=/workspace/.rustup. Commands used:

```sh
cargo test --locked -p cc-parsers --lib --tests
cargo test --locked -p cc-index --test independent_qname_repair_guard --test independent_qname_parser_public --test python_declaration_identity --test qname_owner_diagnostic --test qname_identity_proof --test qname_identity_transaction --test p3d_boundaries
cargo test --locked -p cc-server --test independent_qname_repair_public --test python_declaration_public --test qname_identity_lifecycle --test qname_public_diagnostic
cargo test --locked -p cc-search --lib
python3 docs/reviews/cpp-boundary-declarator-fix-20261003/run_native.py
INDEPENDENT_GUARD_SNAPSHOT="$PWD/docs/reviews/cpp-boundary-declarator-fix-20261003/guard-fixed.json" cargo test --locked -p cc-index --test independent_qname_repair_guard real_language_guard_snapshot -- --exact
python3 docs/reviews/cpp-boundary-declarator-fix-20261003/compare.py
```

Original failed reviews/evidence remain untouched in PR134/PR136 history.
No scorer/gold/budget relaxation, public DEV/new frozen gold, scale run, old
post_index_worker_crosses_pages_and_reopen_reuses_artifacts or containing server
broad suite, GC/WAL/kill/staging/EROFS/private/42export, CI metadata or priority
pressure production edits. Tasks implementation notes/TODO are updated without
changing task status or parent acceptance. V19 and P7 parent items remain open.
Only normal origin push and one draft publication attempt are authorized; no
merge or deployment. Publication receipt is recorded separately.
