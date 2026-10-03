# Checkpoint TODO

- [x] Fixed PR121 base and remote refs verified; existing real Rust compiler verified read-only.
- [x] Independently reproduce cache/live poisoning and actual startup-boundary swallowed error on fixed base; preserve source/logs.
- [x] Fallible session/MCP chain; update all direct production/test callers without infallible fallback.
- [x] Isolated busy/conflict fixtures with owned writable project/cache dirs; startup, cache/live, task count, active preservation and genuine retries.
- [x] Valid explicit None MCP session, normal project real stdio, rejected-config real binary startup, switching and reopening.
- [x] Bounded session/MCP/eval regressions, default/semantic-http all-targets checks and strict Clippy; preserve failures/NOT RUN.
- [x] Source/evidence commit `6011116a1bb6f069c1d4b65cc20fe9f19b9b0003` pushed; exact remote branch verified in `remote-source-head.txt`. This follow-up records delivery only; source is unchanged.
- [ ] Draft PR — NOT RUN after explicit API Forbidden; no API fallback.
- [ ] New-head remote CI — NOT RUN after explicit API Forbidden.
- [ ] Parent integration on `candidate/integrate-gate-parallel-retry-20261003` and independent acceptance; parallel HOLD remains.
