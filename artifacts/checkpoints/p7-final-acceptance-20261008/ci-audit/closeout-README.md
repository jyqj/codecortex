# Fixed ffdc closeout audit adapter (prepared, not yet executed)

The original reviewer is fixed by Git blob and SHA256 in
`closeout-adaptation.json`. The versioned adapter changes only the current
archive/source identities, portable input paths, exact observation-count inputs,
and reporting labels. Reversing the explicit substitutions reproduces the
original reviewer byte for byte. The new original-RPC-count assertion strengthens
a quantity that was previously only printed as a constant.

Every lifecycle, retry, actual SIGKILL exit, lease, cache, GC, source, HTTP,
query correctness and internal-fault assertion remains present. In particular,
the actual post-wait product_exit timestamp must precede the claimed lease
expiry; a preceding kill_time is insufficient.

A separate ordinary Actions preflight must report the actual ZIP member count,
L3 event count, L3 request count and original driver RPC count. That preflight
is not acceptance. Root reviews its output and supplies four explicit expected
integers for this auditor; the auditor does not compute its own expectations.

Dependencies: Python 3 standard library and Git. No Cargo, provider or product
process is run. Use a separate checkout containing exact ffdc source blobs and
the downloaded original ZIP. Output must be a new path whose parent exists.

```sh
export SOURCE_ROOT=/path/to/fixed-ffdc-checkout
export CLOSEOUT_ZIP=/path/to/github-actions-artifact-11529028468.zip
export PYTHONOPTIMIZE=0
python3 artifacts/checkpoints/p7-final-acceptance-20261008/ci-audit/review_closeout_ffdc.py \\
  --expected-members "$CLOSEOUT_EXPECTED_MEMBERS" \\
  --expected-events "$CLOSEOUT_EXPECTED_EVENTS" \\
  --expected-mcp-pairs "$CLOSEOUT_EXPECTED_MCP_PAIRS" \\
  --expected-original-rpcs "$CLOSEOUT_EXPECTED_ORIGINAL_RPCS" \\
  --output /new/audit-output/closeout-review.json
```

The four environment variables used as CLI arguments must contain the reviewed
literal preflight integers. Do not populate them by calculating lengths inside
the acceptance invocation. Run Python without optimization, preserve nonzero
exit and raw stdout/stderr, and retain any failed attempt before a correction.

The complete GitHub job log is included unchanged, including its BOM and final
newline: 1895508 bytes, SHA256 fb4fa22160300e168e94685aac0d79c2046e4838e05de3ae9ae6bfbc52379300.
The steps metadata reports that this actual job finished successfully, but
neither it nor this prepared script substitutes for the independent ZIP replay.
No task ledger is modified by this adapter, and its output is limited to audit.
