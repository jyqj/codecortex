# Historical integration verification, version 2

Run `python3 scripts/verify_historical_integrations_v2.py` from the repository.
The command checks fixed historical identities and the current task definitions.
It does not execute Rust, a product, benchmark, cloud run or provider request,
and does not accept new evidence or close a task. Missing Git provenance commits
are fetched by their full immutable SHA through the repository's normal origin.

## Why this entry point exists

Legacy CI run `37653732411`, PR head
`149aa04f24ddcfd02c3aa5626a59343e88d74866`, failed at
`verify_packing_integration.py:62`. That historical helper compared today's
entire task-state map with its frozen historical map. The legitimate changes
were P7-011/012 `todo` to `done`, P7-013 `todo` to `in_progress`, and P7-018
`todo` to `blocked`. The old E3 helper has the same current-state assumption
for P7-018. The old helpers, assertions, manifests and checkpoints remain intact.

The new command reads the historical task states from the integration which
actually recorded them, while independently checking today's original task
definitions and the normal current plan gate. It imports or executes neither
old historical helper; it has no pin override or alternate-root command option.

## Fixed identities and preserved checks

| Subject | Immutable integration or baseline | Checks |
|---|---|---|
| Packing | `37dd042eaa1209a86e0cafdcd92ae77e036e76f5` | Complete 731 input inventory, each original source blob, the original four-clone adaptation, production subset, 246 evidence files, historical workflow equality, all 192 historical task states |
| E3 | `88f2cf099c8b81f3acef485fd5ac9b01c63ce790` | Complete 708 product inputs from `e3c04fed903c4d0e3c26b5d7cf0e055a6f7c5207`, 58 imported files, seven historical task states |
| Current task definitions | `6d02d77f018a5965a6f289b0b43558ed4b9f8322` | All 192 identities and their order, every original definition and dependency, the exact non-progress plan fields |

E3 integration `88f2cf0` has the measured E3 source as its sole parent and
introduced the production, imported and final-tooling identity manifests.
Its final-tooling manifest binds the historical `tasks.json` to SHA256
`85c771e79166e2d7e596ffff44c514d7c9c41966641c3c8cb3e64cd7c5c469b2`.
This establishes the E3 task snapshot independently of the current checkout.

Before interpreting their rows, the new verifier compares the current packing
manifest and all three E3 manifests byte for byte with their fixed Git blobs.
The nine legacy helpers/registries must also match the fixed task-baseline tree.
Expected identities are never selected from `HEAD`, a task evidence entry,
environment configuration or the current manifest's own claimed digest.

The original **current** gates are retained independently: 37 remaining-gate
rows, V19 open, strict causal speedup and statistical significance both false,
and absence of the excluded historical paired directory. These checks are not
replaced by a statement about a historical gate snapshot.

## Allowed progress fields

| Location | Fields allowed to change |
|---|---|
| Each task | `status`, `evidence`, `implementation_notes` |
| Top-level plan | `status`, `current_phase`, `next_task`, `last_implementation_date`, `execution_note` |

Existing fields cannot disappear; `implementation_notes` may be added to a
task which previously lacked it. Unknown fields are rejected. Every other
field, including nested acceptance, scope, steps, deliverables, hard and
conditional dependencies, validations, rollback and `acceptance_subgates`, must
remain identical to the fixed baseline. Canonical JSON comparison preserves
types, so a boolean cannot replace an integer even where Python equality would
consider them equal. The baseline `tasks.json` SHA256 is additionally fixed as
`cc453fe700a01dd756d35e543d2ceae1f0b748bf97e9542ba0519767f5359ccc`.

Allowing a progress field does not establish that its evidence is sufficient.
After checking definition fixity and progress shapes, the command runs the
current `code_index_plan.py` without write mode. That check still enforces valid
statuses, completion evidence, hard dependencies, navigation and all four
derived views. Evidence acceptance remains an independently reviewed decision.
The verifier refuses optimized Python and uses explicit checks rather than
assertions that optimization could remove.

## Controls

The dedicated stdlib suite is
`python3 -B -m unittest discover -s tests/source_integrity -p test_historical_integrations_v2.py -v`.
It covers fixed historical snapshots, legal current progress, changed
acceptance/dependencies/conditions, boolean/integer substitution, inventory and
field drift, invalid progress and completion, tampered or missing evidence,
symlink substitution, the retained current gates, and forbidden CLI overrides.
The controls use immutable Git data and temporary fixture files; they do not
alter current tasks, rewrite historical inputs or execute payloads.
