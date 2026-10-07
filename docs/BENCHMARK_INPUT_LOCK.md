# Offline benchmark input locks

`scripts/lock_benchmark_inputs.py` adds a preparation-only input-integrity check
around the existing cc-eval suite/run manifests. It runs no benchmark, network
transport, compiler or product binary. It does not make a release candidate or
complete P8-001; the original dependency on P7-020 remains in force.

## What is locked

The caller provides an explicit `--root` and a JSON specification with exactly
`schema_version: 1`, `scope: "preparation_only"`, `inputs`, `bindings`, and
`unresolved`. Every input has a unique `id`, a canonical relative POSIX `path`,
`kind: "file"` or `"tree"`, and one or more `roles`.

Required roles are `binary`, `config`, `corpus`, `queries`, `scoring`, `model`,
`environment`, `source`, `source_receipt`, `build_receipt`, `execution_receipt`, and
`report`. One explicit file can serve multiple roles, such as a suite manifest
containing configuration, scoring and model identity. Additional retained logs or
raw data can use `supporting_evidence`. Missing roles fail preparation; they are
not silently filled with empty or zero values.

File records contain exact SHA256, byte length and permission bits. A tree means
all files and directories below that selected path, including empty directories;
there are no implicit exclusions. Added, removed or renamed members fail
verification, as do changed content or modes. Symlinks and special files are not
admitted. The lock output must be outside the selected inputs and is created
exclusively, without overwriting an existing lock.

Two inventory passes detect changes during preparation/verification; an individual
file also checks its identity before and after reading. This is a bounded offline
check of the selected inputs, not an atomic filesystem snapshot or a continuous
monitor of future writes.

## Receipt relationships

Bindings use JSON pointers into explicit file inputs. Supported forms are:

| `kind` | Required fields | Meaning |
| --- | --- | --- |
| `sha256` | `receipt: {input, pointer}`, `artifact` | A supplied receipt field must equal the selected file's actual SHA256. |
| `tree_sha256` | `receipt: {input, pointer}`, `artifact` | A receipt's complete relative-path-to-SHA256 map must equal the selected tree's files. Directory membership/modes are additionally locked. |
| `equal` | `left: {input, pointer}`, `right: {input, pointer}` | Exact typed JSON equality, useful for source identity and build/execution profile agreement. |
| `value` | `at: {input, pointer}`, `expected` | Match a predeclared literal, such as build features, source commit or a recorded exit status. |

Every binary requires SHA256 relationships to distinct supplied build and
execution receipts. The specification should also bind the complete source map,
recorded source commit, build options, profile/features and the actual report
inputs. The lock checks those relationships; it does not authenticate receipts or
establish that an invocation actually happened. Preserve the original raw
build/execution records and their execution owner. A path, Git HEAD or byte hash
alone is not a source-to-binary-to-execution proof. The lock does not override
cc-eval's existing `binary_source_binding` warning.

Environment is limited to explicitly selected existing records or fields. The
script never enumerates process environment variables, reads external credentials,
or queries host hardware. Use recorded OS/architecture/toolchain/profile and
allowlisted run fields. Missing CPU/RAM/provider-cost information remains unknown;
put unresolved obligations in `unresolved`. Synthetic model identities remain
synthetic, and a fake run cannot become a live-quality report through this lock.

## Commands and external pin

```sh
python3 scripts/lock_benchmark_inputs.py prepare \
  --root /path/to/retained-input-bundle --spec input-spec.json --out input-lock.json
python3 scripts/lock_benchmark_inputs.py verify \
  --root /path/to/retained-input-bundle --lock input-lock.json \
  --expected-lock-sha256 <SHA256-from-the-reviewed-lock-receipt>
python3 -m unittest discover -s tests/evidence_lock -v
```

Keep the emitted lock SHA in a reviewed receipt or another trusted reference.
Verification requires that external pin. Recomputing a digest from an edited lock
and treating it as the old pin would authorize a different lock, not reuse the
original one. The entire input bundle can move to a different explicit root if
its selected relative paths and bytes stay identical. External binaries and raw
receipts must remain available; absent inputs fail closed.

Successful verification means `matching_preparation_inputs`. Both
`release_candidate` and `execution_attestation` remain false, and unresolved items
are returned unchanged. Failed verification exits 2 and never rewrites a report,
source file, existing receipt or original lock.
