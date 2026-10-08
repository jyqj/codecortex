# Five reviewed D0 source changes adopted by P5

P5 `f5319d6a04f83aabe9dd635137382f4782d8d189` has parent G3
`24b0cba53178d14ef6c02355ee271fa0067be3e0` and changes exactly five crate
files. Its complete production inputs match the actual D0 engineering source
`d0cb69c601e530dcef738af0c2dffb8d3b8bcf28`; the complete 136 validation
inputs remain those already reviewed on G3.

The changes reuse a bounded dependency window within one prepare, add the
nonunique doc-key lookup index for fresh and current schema 25 databases, and
batch the streaming oracle's prepared inserts at bounded 64/8/1 row tiers.
The independent review retains the original source, schema, epoch, incomplete,
typed-value, duplicate, 15-table oracle, capacity, and failure boundaries. It
does not claim that all scale costs are removed or infer a causal wall-clock
speedup from different hosts.

## Actual engineering validation

The original [D0 engineering run](https://github.com/jyqj/codecortex/actions/runs/37846370300)
completed all 12 registered commands, including format/Clippy, targeted
dependency/schema controls, original streaming-oracle regressions, and the full
P8 scale test target. It recorded 49 passing test executions, representing 48
unique cases; one original optional actual-MCP test remained ignored and no test
failed. The unmodified release build and complete 1k/10k diagnostic raw then
passed independent replay with the original build/shard validators.

`D0-engineering-originals.tar.gz` retains 83 original evidence members, including
both diagnostic ZIPs, control and capacity ZIPs, complete build metadata, actual
job log, original validator outputs, exact source files, independent reviews,
and the P5 object reconstruction. Its manifest records every member's size,
SHA256 and mode, plus the original release binary's exact Actions reference.
The binary remains the D0 binary; no receipt or raw engine identity is rewritten
as P5.

The independent full 150-cell D0 scale study is a separate fixed controller
commit `ca72a2dde363e35203f7b3cb30ed00729d460fd7`,
[run 37854240827](https://github.com/jyqj/codecortex/actions/runs/37854240827).
Its workflow and controller implementation are not adopted into the P5
validation domain. This directory also preserves the second independent
registration cross-check produced after that separate study was registered.

The source review is scoped implementation acceptance. Complete P5 CI and the
original TODO execution/dependency gates remain required. At this checkpoint,
0 original TODOs are newly closed and 29 remain.
