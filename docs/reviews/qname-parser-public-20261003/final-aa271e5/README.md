# Final bounded follow-up: P1/P2 CLOSED, PASS for scoped integration

Fixed production source: `aa271e52b9c2aa52e05696af96ac52116963e57e`.
Fixed documentation head: `49e0330754e6451e7eb5e91863f432ce7eaf816c`.
This independently reviews only the C/C++ P2 declarator-name repair and preservation
of the already repaired Python proof. Both historical REJECT reports, source
fixtures and raw failures remain untouched in the parent and repair-f4df9a8
prefixes. No new production edits were authored here.

## Original C++ P2 is closed

The exact original input and strict leaf boundary assertion were rerun, unchanged:

```cpp
namespace grove { template<class T> T leaf(T x) { return x; } }
```

Real production engine and real MCP JSON-RPC now return **leaf / function /
absent qname**. The independently authored original C++ name-only micro gold is
byte-identical, and the unchanged native normalizer/source verifier/scorer
returns **recall10=1** on both APIs, recovering from the earlier repair's 0.
The entire source_evidence proof equals the rejected target's original proof;
source bytes/owner are not changed to manufacture a match. The native driver is
the exact prior independent source, reused without editing it or cc-eval.
`fixed/` retains complete actual engine/MCP envelopes and native scores/gold.
The driver's `before` argument selects its existing positive-score assertion;
this run is against aa271e5 and is explicitly **not** relabeled as an old baseline.

The native C++ parser's namespace-free-function Method classification remains
preexisting debt. Its incompatible kind is still rejected, and no qname is
published for this function. This review does not certify that parser taxonomy
as correct, invent a qname, or relax the AST family guard.

## Three new independent declarator probes

`independent_cpp_final_declarators.rs` adds:

- A C named-struct return with nested parenthesized/pointer/function declarators:
  return-type Token and parameter decoy/argument poison hints cannot replace
  real name harvest. This directly checks AST boundary/hint behavior.
- A valid C function with 70 pointer edges: exhausted bounded traversal omits
  name, without falling back to long or the parameter decoy.
- A C++ explicit const-char-pointer conversion operator: unsupported declarator
  remains nameless even with a same-position poisoned Method hint, while adjacent
  supported known method retains its name and Method kind.

All three pass. An initial review test incorrectly assumed native C symbol
catalog support for the deeply nested function-pointer-return form; it does not
emit harvest. This limitation is distinct from the boundary-name fix (C native
extraction is unchanged). The corrected guard probe supplies an explicit poisoned
hint seeded from a plain real function at the boundary coordinates; it does not
construct a persisted/public identity. `native-symbol-assumption-raw.log` preserves
the initial failure. `compile-raw.log` preserves a corrected test syntax typo.
Neither is counted as a product failure or hidden as a passing run.
The output comparison initially assumed the entire leaf query had exactly one
hit; restored namespace hits also appear. It now requires exactly one leaf hit,
no callable T name, and the same source proof for leaf; all full outputs remain
retained. This does not relax native identity or positive-score acceptance.

## Python proof remains intact

One exact independent decorator-owner test and the prior strict public-proof
target were rerun. Canonical lexical Class/Method/local Function IDs, decorator
reference/call ownership, original Unicode/CRLF bytes, correct qnames, both owner
endpoints, unique SQL-surviving declaration and matching persisted identity all
pass. Existing whole-function local-class chunk semantics are retained.

Real engine/MCP ordinary Python micro gold remains correct=1/wrong=0. All five
ordinary Python hits equal the previous fixed target in **every field**, including
complete metadata, scores/traces/source/document proofs. Original decorated-class
queries still expose Outer/Class/Outer, Inner/Class/Outer.Inner and
pulse/Method/Outer.Inner.pulse on both APIs. No scorer/gold injection is involved.

## Independent receipts and limits

This final turn executed **6 precise tests, 6 PASS**: three new declarator probes,
one unchanged original leaf assertion, one exact Python owner test, and one
strict public proof test. Logs are `declarators-raw.log`, `leaf-strict.log`,
`python-owners.log`, and `python-public-proofs.log`. `comparison.log` records the
separate actual-output/native checks; `compare.py` reproduces them.
Author's 591 suite is inherited evidence only and was not rerun or counted here.

Direct official Rust1.95, official original Cargo.lock/--locked, normal inherited
proxy and owned build/index caches continued unchanged. New test rustfmt and
review diff checks passed. `source-manifest.json` binds fixed target, product
source hashes, original driver and historical review byte preservation.
Reproduce the native run with the prior independent `repair-f4df9a8/run_native.py`
against this fixed checkout, an owned output directory and its `before` argument;
run only the four precise test commands reflected in the logs.

**Bounded integration judgment: PASS for these P1/P2 fixes.** No additional
blocker was found in this scoped review. This is not whole-project/V19 closure,
C++ taxonomy certification or complete unknown-declarator coverage. No public
DEV/frozen gold/scale/provider work, author broad suite, excluded runtime/GC/WAL/
kill/staging/denied/private paths, remote PR merge or deployment was executed.
V19 and parent items remain open.
