# Independent 100k diagnostic originals

This preparation binds product P `c9f95c9a21e4aeb28a8cfcde5f3afc94c142e119`, review R `48917a5b48a386c0fd49804a089183e641ad2bba`, and actual G `bcc3ada39603d5dfa0a902d7e188f1a86accafca` / tree `7afc7fcab3bedfd31c05b87ab0cd10c70d148997`. The run, attempt and event remain unknown until Root observes the real run. Fill them in a new bound copy of the templates; never infer them from an earlier study.

The workflow is `.github/workflows/p8-scale-100k-diagnostic.yml`. Its only opt-in label is `p8-scale-100k-diagnostic-run`, tested against `github.event.label.name`; it also permits `workflow_dispatch`. There are no user-supplied workflow inputs. Root alone publishes the source, changes labels/refs and downloads original artifacts. Publishing may trigger ordinary workflows under their existing rules. The capture workflow's upload/download action references are `@v4` tags, not immutable commit pins; retain the original resolved action version from actual job logs.

## Source and execution contract

The final source keeps the original native Rust, scale driver and helper. The diagnostic calls the existing full-stage driver at 100000 files, shard 0/30, registered repetitions 30, seed 12648430, workload `scale_wide_dirty_v1`, dirty budget 4096 and resume limit 1024. The independent fanout contract stays 8/128; the original 100k plan skips fanout because it belongs to the 1k scale. Native deadline 18000000 ms, output 536870912 bytes, complete stage/parity rules and physical oracle profile remain unchanged. No existing G8 or GW4 run is modified or resumed.

The build job has a 45-minute timeout. It performs formatting, original Rust scale protocol controls, the 15 harmless host controls, then builds one executable through the original driver. The diagnose job has a 350-minute outer timeout. It has 20 finite checkpoint deadlines, 900 seconds apart from the original monotonic launch timestamp. Upload durations do not reset that clock. The finish step waits for actual driver termination within the outer deadline, including the original native five-hour allowance plus the driver's 120-second timeout margin. This does not guarantee the runner survives, that a job finishes, or that all output is uploaded.

## Inputs retained once

Preserve complete original ZIP bytes and official run/job/artifact metadata. `artifacts-index.template.json` defines the local caller manifest; its artifact rows carry the exact official name, ID, size and digest, run/attempt/head binding, formal local ZIP path and the saved official metadata/transport reference. This template is a locator, not a substitute for original API responses. Missing official digests must remain explicitly unverified.

| Artifact namespace | Evidence | Acceptance boundary |
|---|---|---|
| `p8-100k-diagnostic-build-RUN-ATTEMPT` | Original build receipt, Cargo stdout/stderr, before/after source maps and ELF | Verify original bytes and producer. Original `validate_build` remains the build predicate; its `--hash-file` is not a benchmark. |
| `p8-100k-diagnostic-capacity-RUN-ATTEMPT` | Original capacity receipt | Environment evidence only; zero measurement credit. |
| `p8-100k-diagnostic-progress-RUN-ATTEMPT-00` through `-19` | Immutable checkpoint manifests, pending byte chunks and point-in-time metadata copies | Uploaded prefixes only, potentially ending inside a JSON record. Early driver exit legitimately skips later periodic steps. |
| `p8-100k-diagnostic-original-RUN-ATTEMPT` | Final original shard, original stdout/stderr, capture state and checkpoint 20, observer and supervisor receipts | May be absent or partial after runner loss. Original raw/shard predicates decide actual complete stage evidence. |

No artifact outside these namespaces is silently treated as this diagnostic. The reader permits any actually available subset and reports missing evidence as unknown. It does not manufacture an empty final file or successful result. ZIP traversal/symlink/duplicate protection and member CRC/SHA checks are custody protections only; they do not add a performance gate.

## Custody reader

`read-original-diagnostic.py` is pure offline Python. It never imports the original driver or executes the ELF, Rust, a workload or any remote action. Once Root has supplied originals, its explicit future command is:

```sh
python3 -B read-original-diagnostic.py --binding binding.actual.json --official-index artifacts-index.actual.json --output intake-attempt01
```

The output directory must be new. Preserve the command's integer exit, stdout and stderr with its generated receipt. Exit 0 means no internal contradiction was found in the supplied identity/custody records. Native failure can legitimately coexist with reader exit 0. It does not mean all artifacts, all raw records, all native stages or a successful original validation exist.

For each stream the reader checks original interval names, offsets, lengths and SHA256; overlapping inconsistent chunks fail. Repeated uploads of an identical pending chunk retain all locations without increasing the prefix or creating another sample. A local ACK is checked against its exact checkpoint manifest and supplied original artifact ID/digest. If that older artifact is unavailable, the claim remains unverified. Conversely, receipt of a genuine artifact preserves its bytes even if the runner died before recording the next local ACK. The last upload therefore does not need a later checkpoint merely to establish custody.

Streams include native raw/worker stderr, original supervisor stdout/stderr, driver stdout/stderr and capture wrapper stdout/stderr. Metadata snapshots retain original bytes even when they are incomplete JSON. Capture faults remain `partial_with_capture_faults` independently of native exit. Faults from missing checkpoint manifests can be unknown; the absence of observed faults is not proof of none. All bytes after the last verified retained prefix remain unknown unless final originals provide them. No raw prefix is invented into complete JSON, EOF or a completed 100k run.

The wrapper's driver return code and negative signal are reported separately from the native report and worker exit. A live supervisor, an upload success, or wrapper exit 0 is insufficient. Native success claims require matching original driver/native/shard records, but still do not replace the original raw/parity validator. Descendant cleanup remains unknown where the original supervisor did not establish it.

## Original predicates, prepared but not executed

`call-original-validator-once.py` verifies the fixed original driver/helper SHA values before importing their exact paths. It provides one explicit selected call per invocation and never starts a new build or scale workload. Root will choose the call after real evidence exists; do not run these during preparation.

1. `build`: original `validate_build(build_directory, exact_source_root)` against the actual G checkout and preserved ELF. Preserve its original result once for reuse.
2. `raw`: original `inspect_raw(raw_path, registered_plan)` for retained complete raw evidence or a finite original failure-boundary replay, with the exact plan. Incomplete coverage or a truncated line may correctly fail. Retain that failure instead of weakening the predicate or inventing a tail.
3. `shard`: original `validate_shard(shard_directory, original_build_record, preserved_binary, original_build_receipt_sha256)` after the same build was accepted. It may invoke the preserved ELF's existing hash mode, not a workload. Its full original source/engine/parity/coverage predicates remain intact.

The original source root for build validation must be a real isolated checkout at actual G, with the required source domain and two original driver files. Do not substitute P3 for G in native source identity, fabricate HEAD or patch the driver to fit a recovered directory. The custody reader's source-map checks are transport evidence; they do not silently replace this original execution context.

Even if this diagnostic has a fully accepted successful 100k shard, it belongs to its own exact G/run/attempt/binary population. It is not one of the old G8/GW4 registered 150 slots and does not close a task. Full cohort and release decisions remain separate.

Preparation validation is AST parsing only. No reader, original predicate, ELF, Rust, product or Actions execution has occurred in this preparation.
