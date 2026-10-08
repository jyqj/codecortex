# Public DEV kind annotation review — 2026-10-03

No established original-gold error was found. Requests has a documented broad
`function` convention; Gin's original convention remains unresolved. The 166
declarations independently bind to class-owned Python definitions or Go receiver
methods, but the frozen protocol does not define whether `kind` describes a
declaration category or a broader function category. All normative resolutions
remain open. No gold patch or newly admitted candidate is created.

## Frozen identities and ownership

- Product PR131: `88f2cf099c8b81f3acef485fd5ac9b01c63ce790`.
- Admission PR91: `5385f5a7a2a875c6d5cbd049bdde039bf71bbf32`;
  receipt SHA256 `b4388ca60612f6734719b348bf30e05dd0752b7b754f47cc078b590e35c1e425`.
- Diagnostic intake: `f40dea9277ef134dcab47dd83a467aef5ad9e339`, used only to
  establish the affected scope. No product raw hits, scores or outputs are inputs
  to `review.py` or the annotation decisions.
- Requests author `e49f9ada5826b4206d2128f3bfb8d31603ff42fa`, owner review
  `3aae8bf2a690213426af91427dc837fedbe50c83`.
- Gin author `949a9471f552d496e85b27c460c7b76536f13803`, source owner
  `ba79bcfc3f2bcc622297ef53b5de37ce205a5c89`, repair owner
  `5aafcda4a5098c8407d9373eafc3185e59932550`.

Environment `/workspace/.agents` and `.codex` were empty. Repository instruction
discovery at the product base found no AGENTS.md or SKILL.md. This ownership is
only this new directory. It does not alter the concurrent qname task, parser,
scorer, ranking, default selector, admission, raw outputs, thresholds or TODOs.
No historical failure tests, excluded repositories, private/old42 inputs or live
providers are used. Ordinary origin Git and GitHub API operations are separate.

## Independent findings

| Scope | Requests | Gin |
|---|---:|---:|
| Frozen native DEV rows | 91 | 67 |
| All alternatives source-map/hash verified | 141 | 127 |
| Affected alternatives / answer groups | 85 / 85 | 81 / 81 |
| Affected queries | 58 | 43 |
| Primary / supporting affected alternatives | 56 / 29 | 39 / 42 |
| Affected required-facet group links | 85 | 81 |
| Affected local correlation components | 55 | 42 |
| Affected four-repo global correlation components | 55 | 42 |
| Original local correlation components | 82 | 66 |
| Frozen compat projections verified | 83 | 55 |
| Changed compat projections / counts / components | 0 | 0 |

These are annotation counts, not retrieval scores or independent-question counts.
The component memberships are read from the frozen admission registry/relations;
no new equivalence decisions are made. Source definitions are matched by name and
exact byte start, full declaration end and only trailing whitespace. Python
qualified ownership and original source-map ownership also match exactly. Go
receiver ownership matches the source-map symbol exactly. Original file and span
hashes are checked against the admission locks and gold source maps. This is a
kind-only review, not recertification of behavioral gold, facets or graph claims.

Requests has 79 instance methods, four property accessors and two static methods
in the affected set. Its author `scripts/author.py:42–44` explicitly labels every
non-ClassDef as `function`. The source-owner `review_dev20.py:88` explicitly
asserts the supplied `function` label, and all current reviewed question/gold
canonical hashes bind. This documents a deliberate broad declaration convention,
not an accidental label inferred from current product scores. It does not prove
that this convention was normatively standardized for native-v1.

Gin has 81 receiver declarations. Official Go AST (`go/parser`, no product
imports) independently distinguishes them from its 39 ordinary function controls.
The original/repair owner receipts accept the source facts and preserve current
row hashes. They contain no explicit declaration-versus-broad-function taxonomy
decision. That ambiguity cannot be resolved by a low score or by current parser
behavior. Gin is classified `unable_to_determine_original_kind_convention`.

Frozen admission PREREGISTRATION.md §Metric definitions requires exact supplied
symbol identity fields, but does not specify `function` versus `method`. The
Requests retained original preregistration is separately hash-bound. No confirmed
parser misclassification is established in this ownership. Counts by decision:
85 documented broad-function convention; 81 unresolved convention; zero confirmed
original-gold errors; zero confirmed parser errors. Normative decisions remain
open for all 166 items.

## Language semantics and versioned proposal

[Python's data model](https://docs.python.org/3/reference/datamodel.html#instance-methods)
distinguishes function objects stored in classes from bound method objects.
[Static methods](https://docs.python.org/3/reference/datamodel.html#static-method-objects)
return the wrapped callable without binding; class methods bind the class.
[Go method declarations](https://go.dev/ref/spec#Method_declarations) have an
explicit receiver. These language facts support a declaration taxonomy; they do
not by themselves select the historical benchmark's annotation convention.

`versioned-proposal.json` proposes a separately reviewed declaration-based v1:
class-owned Python definitions (including static/class/property accessors) and
Go receiver declarations use `method`; free functions and functions nested inside
functions use `function`. It is **not admitted** and does not silently redefine
native-v1. `item-review.json` supplies the minimal conditional per-item kind
proposal, with original query/gold/alternative raw hashes, original source byte
coordinates, source-map and owner-review bindings. It publishes no query, qname,
gold answer, source or raw hit bodies. Query IDs and qualified names are hashed.
Raw JSON-token hashes and newline-inclusive query-line hashes are explicitly
distinguished from canonical owner-review hashes.

If the taxonomy owner selects this new version, exactly the 166 kind fields are
potential changes. In-memory hypothetical proposals verify every other field is
identical, and all path-only compat projections are identical. No hypothetical
scores, revised query files, candidate admission, accuracy claim or holdout
conversion is produced. Confirmed-error patch list is empty. The old admission
and every raw/score file remain untouched; a future adopted candidate needs its
own reviewed manifest and admission version.

## Hand-authored tests and reproduction

`test_taxonomy.py` uses only independently authored miniature sources. Five tests
cover class/instance/static/classmethod/property/nested-function ownership,
Python runtime binding, Go value/pointer receivers and ordinary functions,
same-name different receivers, fake declarations in strings/comments, UTF-8
offsets, exact raw JSON token preservation and open proposal state. A static
method may be a `method` declaration under the proposal while its accessed value
is still a Python function. This distinction is intentional and tested.

Python 3.12.14 and official Go 1.23.12 are used. `/usr/bin/go` was an unrelated
program and rejected `build`; no product build was attempted. The official Go
archive was downloaded into ignored `.scratch`, verified against the official
Go download SHA256, and used solely for this independent AST helper. An initial
source-binding assertion encountered a non-function Go control; the kind review
was narrowed to `function` declarations while retaining source-map/hash checks
for those controls. Both development failures are retained in the validation
history, not counted as passes. Final five tests pass; no Rust workspace or
provider tests were run.

With the exact public Git objects above already fetched:

```sh
D=artifacts/checkpoints/public-dev-kind-review-20261003
GOCACHE="$PWD/$D/.scratch/go-cache" GO111MODULE=off \
  "$D/.scratch/go/bin/go" build -o "$D/.scratch/go-taxonomy" "$D/go_taxonomy.go"
python "$D/review.py"
python "$D/test_taxonomy.py"
python "$D/verify.py"
```

No source corpus is materialized or uploaded. The 41 source files needed by
answer alternatives are read in memory from pinned Git blobs. Nine retained
legal-text files are copied unchanged and hash-manifested; licenses/NOTICE in
original inputs remain untouched. Reproduction needs a local Go compiler (the
verified compiler archive/cache/binary is ignored), but no live provider or
product binary. Draft only; no merge or deployment.

The ordinary whitespace check reports the retained Werkzeug LICENSE CRLF bytes as
29 trailing-whitespace lines. They are preserved byte-for-byte. The scoped check
with `core.whitespace=blank-at-eol,blank-at-eof,space-before-tab,cr-at-eol` passes;
no original legal text is normalized.
