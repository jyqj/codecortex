# Module input safety and G3 boundaries

## One read owner, no execution

`cc-model::input_file::read` is the bounded regular-file reader shared by `.codecortex.json`, captured module configuration and fallback compact Rust/Go source reads. The project-model layer still owns configuration path admission. The parser crate extracts syntax only; its historical `import_resolver` filesystem API and language-agnostic candidate generator are removed. Runtime module resolution is owned by `cc-index::module_resolution` and consumes immutable admitted data.

Application settings have a 1 MiB limit. Invalid, unreadable, non-regular or symlinked application settings log a warning and retain the existing defaults-plus-environment-overrides policy; they are not silently interpreted as successfully loaded settings. Module configs retain 1 MiB/file, 16 MiB aggregate and 1024-input bounds; inheritance depth is 32 and effective configurations have their own aggregate bound. Compact source reads are capped at 4 MiB, including an extra-byte growth check. Already captured text is size-checked before copying. Repository-relative read paths have at most 256 components.

The Unix implementation pins the trusted project root as a directory descriptor, checks each component using fstatat with AT_SYMLINK_NOFOLLOW and opens each component with openat/O_NOFOLLOW. Intermediate components must be directories; the final component must be a regular file both before and after opening. O_NONBLOCK prevents a final FIFO replacement from blocking the open. Every returned descriptor has one File owner and closes on every error path. It does not execute build scripts, package-manager commands, configuration code or network fetches.

The root is caller-selected and may have a platform alias such as /tmp. This is not a sandbox against a privileged actor changing mount points, hard links, the trusted root's ancestry or moving already-open directories; it is not an atomic filesystem snapshot. Contents may change during a read. Scanner content hashes and pre-publication config verification remain separate consistency fences. Reads supplied from scanner-owned text do not repeat disk validation on each import.

Non-Unix builds perform a component-by-component regular-file/symlink check (including Windows reparse attributes) but do not claim descriptor-relative race resistance. Native Windows/Linux testing and remote CI are separate evidence requirements. A macOS pass must not be relabelled cross-platform certification.

`path_guard::resolve_indexed_path` still enforces exact indexed spelling and project containment. It now rejects non-regular paths before returning. As it returns a path rather than a handle, its guarantee is validation-time only, not a promise against later path replacement by a concurrent actor.

Special manifest identity uses the exact final filename. Ordinary TypeScript inheritance files such as `base-package.json` and `subpackage.json` must not be parsed as package manifests merely because their names have the same suffix. The standalone `package.json` manifest retains its package interpretation; arbitrary multi-purpose manifest formats are not compiler-certified.

## Module budgets and missing knowledge

Go packages above 4096 admitted members return Unknown with reason `go_package_member_budget_exceeded`, at most 64 candidate previews and no selected target. They are not represented as an invalid or partially successful package. Existing evidence/frontier byte and count limits still apply independently.

Module requests validate source-path shape, identifier/condition lengths and aggregate context before language-specific work. Rejected long inputs return Unsupported with an explicitly bounded import preview and a length-delimited digest identity; different hidden suffixes must not collapse into one decision. These are diagnostics, not successful facts for a truncated import.

JavaScript workspace and Python project ownership use ancestor-key lookups, not a scan of every unrelated workspace/root for each import. Rust workspace-alias lookup uses an entry-owner index, retaining multiple owners as ambiguity rather than scanning all crates and choosing the first. Go's selected workspace participants and their requirements still cost proportional work; they are actual dependencies and are not claimed O(1). Model discovery, file inventory construction, copied snapshots and high-fanout dependency continuation also retain non-constant costs.

## Compatibility and verification

Vue/Svelte script importers use the same captured local JS/TS rules instead of the removed generic fallback. This does not certify framework transforms or resolution of arbitrary `.vue`/`.svelte` module targets. Other unmodeled language module rules return Unsupported rather than selecting an unrelated JavaScript file. Symbol/name heuristics outside the declared module subset remain separately labelled capabilities.

Database schema 21 persists document manifests/spec stamps and per-file chunk policy stamps and requires an isolated rebuild from schema 20 and earlier to preserve original chunk bytes/coordinates and remove obsolete path guesses and dependencies; see [SOURCE_CHUNKS.md](SOURCE_CHUNKS.md). The captured data layout remains ProjectModel version 3; the metadata key remains project_model_inputs_v1. No test opens or clears a user's daily project index. The removed Rust crate helper is an internal source API change, not a removed MCP tool; all 14 public input contracts are checked independently.

Tests include child-process deadlines around FIFO inputs (including application settings), linked directories, root-anchor replacement, oversized/recursive inputs, invalid module ownership, bounded Go package sets, actual four-language parser/SQLite targets, reopen/parity and public MCP graph results. The `p3d_cost` fixture reports scanner/capture/build/lookup observations separately. Its pure queries run after the fixture source/config tree is removed; this proves disk independence in that fixture, not a global syscall counter or p95/RSS/100k certificate.

Normative reference for descriptor/flag semantics: POSIX open/openat, https://pubs.opengroup.org/onlinepubs/9799919799/functions/open.html (consulted 2026-09-28). The implementation report and exact frozen receipts establish what actually ran; this document alone is not a passing gate.
