# worker contention measurements scoped source review

Exactly one new P7-015 fixture path, with actual slow-worker, foreground progress, complete timing and stale-publication measurements. Scoped source acceptance only.

Read all 505 lines of the frozen fixture, original lifecycle/HTTP control attribution and the complete measurement interpretation. Read actual command/source/artifact receipts and independently verified the lossless archive: 34 original files, eight archive members, 26 directly stored originals, no missing bytes. The fixture freezes seeds, joins each request wave before failure assertions, retains successful rows and failed ordinals, exercises the real CodeIndex/post-index worker and retires held old-space inputs before release.

- The isolated source has 765 inputs and was not behavior-run. The author source with 769 inputs executed the tests; the exact sole added blob is preserved.
- All 384 requests are samples within one Rust function, not extra tests or TODO IDs. A process termination can leave a current wave incomplete; unavailable panic timings cannot be recreated.
- Observed 1119.096 ms held and 1351.478 ms quiet tail requests remain in the complete sample set. Descriptive 32-point quantiles do not prove a P99 degradation gate, new SLA or causal speedup.
- Runner/server/tree CPU and RSS ownership remain unknown. In-process FakeProvider capacity is not the HTTP admission gate; the unchanged six HTTP controls are separately recorded.
- Bounded desired projection into reconcile_after_rebuild and original resource/performance acceptance remain open. No full P7-015, new combined source, scale, quality, G7 or release certification.
