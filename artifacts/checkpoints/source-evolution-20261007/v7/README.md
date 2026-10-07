# v7: optional-work metadata boundary

The new source admission changes only four verifier constants, the pinned
registry and the explicit CI source selector. The fixed v3 base, nine historical
helpers and all earlier source/review evidence retain their original bytes.

The complete product has 768 crate/Cargo inputs. Its seven-path validation delta
comes from a fixed A-only source and a separate limited review; the three-path
Python capture delta is unchanged. The A-only tree has 766 inputs and was not
behavior-tested separately. Both the author-combined source and the admitted
product match the recorded 768-input manifest exactly.

The actual source CLI passed, all 14 current negative controls passed, and an
independent reviewer reconstructed the full union and checked its fixed pins.
The roadmap generator also passed for 192 tasks: 150 done, 41 todo and one in
progress. These checks certify source/plan integrity only.

The behavioral checkpoint records the original 12 narrow passes, 13 passing
semantic-http scope/exact tests, and all four failures from the complete local
default continuation. Its 64 original files are preserved, with 44 large raw
observations/logs stored losslessly in an archive. The storage check verifies
bytes and does not change test outcomes. Final-head GitHub CI is a separate gate.

See `receipt.json` for immutable source/review identities, execution attribution,
commands and limits; `guard-review.json` is the peer review. Earlier v4/v5/v6
records and both previous CI failures remain historical facts.
