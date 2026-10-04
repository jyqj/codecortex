# Independent PYGO declaration-kind v2 source-semantic review

Source-semantic verdict: **PASS** for fixed candidate
`97c478cdb05d2bb85f852ff42af5452b00ba6e3a` only. Admission is **not decided**;
this is an independent review for root's subsequent admission decision, not an
admit signature. No default selector, central TODO, product, candidate artifact,
ranking, scoring, provider, or historical admission state changed.

Applicability is exactly Requests/Python and Gin/Go's new declaration-kind v2
candidate. Python definitions whose nearest enclosing declaration is a class
(including static/class/property and async definitions) are method declarations;
free and function-owned nested definitions are function declarations. A local
class inside a function owns its own methods. Go receiver declarations are
methods; ordinary functions remain functions. Express/JS and TypeScript retain
their historical taxonomy. This is neither a four-repository uniform version nor
new independent samples. Requests' original broad function convention had author
and owner support; Gin's historical taxonomy remains unresolved. No historical
gold error is claimed.

## Independent evidence

`audit.py` imports no review oracle, candidate replay, author tool, product parser,
scorer, or evaluator. It uses Python's standard AST and a newly authored Go
standard-library parser helper. It independently joins the original gold/source
maps with original owner accept receipts and fixed original native rows. Every
alternative (including class/interface/variable controls) is uniquely bound to a
source declaration by name and byte span; Python lexical qname and Go receiver
ownership are checked. Source hashes are checked against original admission
locks. All proposed bindings and owner-row hashes are independently reconstructed.
Only counts, pins, and self-authored miniature examples are exposed in this review
prefix; original query/gold/source bodies remain in memory.

| Evidence | Requests | Gin | Total |
| --- | ---: | ---: | ---: |
| Original native rows | 91 | 67 | 158 |
| Alternatives rechecked | 141 | 127 | 268 |
| Independently required deltas | 85 | 81 | 166 |
| Unchanged compat projections | 83 | 55 | 138 |

41 source files and all 9 original license/NOTICE files were verified from fixed
Git blobs. The candidate-retained legal bytes equal those blobs, with no
normalization. Source-map bytes, owner receipts, query text, qname, span, facet,
primary, thresholds, weights, related groups, language and all other fields are
unchanged. Independent recursive JSON token traversal checks overlay identity,
exact kind locations/tokens and line/file hashes. Reversing only those patches
reconstructs every original native line byte-for-byte, proving zero non-kind byte
changes. Derived alternatives permit only literal method, with no dtype,
wildcard, alias, or scorer changes. Original compat expected-file projections are
also checked directly. Original 301 native / 256 compat / 280 related groups
remain historical counts, with zero new independent samples.

Original admission PR91 is fixed at
`5385f5a7a2a875c6d5cbd049bdde039bf71bbf32`; the historical source review is
`6c1416109003bcff0c1911307a4af5bd48870517` (the supplied PR1336... text was PR13
concatenated with this SHA). Original author/source pins are recorded in
`input-pins.json`. No path outside this new prefix differs from the fixed
candidate, so retained historical old1671/allPartial/raw/gold/source/admission
bytes already present at that commit remain untouched; original external pinned
inputs are read through Git without rewriting them.

## Language evidence and adversarial tests

The candidate's **benchmark taxonomy is the user's explicit decision**. Standard
language references establish lexical declarations and receiver/runtime behavior,
not a retroactive mandate for historical benchmark labels:

- [Python function and class definitions](https://docs.python.org/3/reference/compound_stmts.html#function-definitions): definitions and class suites determine lexical ownership.
- [Python staticmethod](https://docs.python.org/3/library/functions.html#staticmethod), [classmethod](https://docs.python.org/3/library/functions.html#classmethod), and [property](https://docs.python.org/3/library/functions.html#property): callable access/runtime descriptors differ from declaration ownership.
- [Go method declarations](https://go.dev/ref/spec#Method_declarations): a method declaration has a receiver.

Accessing a Python static method can return FunctionType; a class method or bound
instance method can return MethodType; a property's getter is a function stored
in a descriptor. These runtime facts do not change class-owned declaration labels.
The miniature tests directly check those distinctions. Further tests cover
same-name wrong owners, nested classes, local classes inside functions, nested
functions, async/conditional class definitions, value/pointer Go receivers,
interfaces/variables, fake declarations in strings/comments, and UTF-8 offsets.
Mutation tests reject subsets, extras, duplicate items, forged owners/source pins,
non-kind field relabels, and dtype/wildcard/alias kinds. All 12 tests pass.

## Reproduction and limits

Python 3.12 and official Go 1.23.12 were used. `/usr/bin/go` is an unrelated program;
its initial build attempt failed. The official Linux amd64 archive was fetched
from `https://go.dev/dl/go1.23.12.linux-amd64.tar.gz` into ignored `.scratch`,
and verified against Go's official download metadata SHA256
`d3847fef834e9db11bf64e3fb34db9c04db14e068eeb064f49af747010454f90`.
An initial free/type source-symbol assertion was too strict about package-qualified
names; receiver checks remain exact, while free/type names bind by last component
and unique syntax byte span. These development failures are not counted as passes.

With fixed Git objects available and a Go compiler installed:

```sh
D=artifacts/checkpoints/public-dev-pygo-v2-independent-20261003
GOCACHE="$PWD/$D/.scratch/go-cache" GO111MODULE=off \
  "$D/.scratch/go/bin/go" build -o "$D/.scratch/declarations" "$D/declarations.go"
python3 "$D/audit.py"
python3 "$D/test_independent.py"
```

`result.json` and `audit.log` contain only counts/pins. `artifact-manifest.json`
seals this independent evidence. Raw candidate manifest pins were recomputed:

- Change manifest: `f70f9b2ea562cb7fe8dca44f341e24671af0f84a072ff878e80f10fb67189507`
- Candidate artifact manifest: `1a0b9ed8a02eb770672d1f0baa7ef330451d865a6d174ea8eab4c8dc823603e9`
- Candidate gold: `8e65f94b66c18bcca5f1e78e904fa882ea296efa23b1b000c5ce4a2ac7d15a6b`
- Derived Requests native: `7847fbe8326977121e964ee3729d47ce4855e7ce51b32c3918d8bd5ef77c3730`
- Derived Gin native: `df968bdbd7422d93c7e6e4e41d3e67053c11bca3b6be595a60999abf0d480f77`

No Rust workspace tests, ranking/score runs, private-fault tests, or live-provider
calls were needed or run. The verdict covers source-backed declaration semantics
and exact overlay preservation; it does not independently re-certify behavioral
relevance or semantic independence of historical correlated groups. Draft only;
no merge or deployment.
