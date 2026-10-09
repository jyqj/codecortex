# Cold-only source admission: actual execution evidence

This archive preserves 21 fixed execution and source-binding files listed in the handoff, plus that unchanged handoff itself: 22 regular members, 1,051,773 original bytes. It does not contain the source projection, Git object cache, raw measurement ZIPs, private transfer responses, or later CI snapshots.

The fixed source chain is:

- Parent M7F: `fffd950d5b34b0180f308db1428a57d5bf358bf1`.
- Product P: `fb814074a6a14f87d09cb392ad07ec1c497383b6`, tree `6291bfed72df691ae47303682f8417f0d8ee4676`; exactly eight reviewed changes.
- Review R: `d96aec38f0c520d8914a5b974951b781ba87c82b`, tree `0b1b8e38e139ec930cc21169ef83945d251aa112`; exactly 15 added review and existing-control records.
- Execution G: `044c008c9459cfa61e7701db2eb868342c508a39`, tree `2dafb408835b5bec6cb7cfa5533d46082595f0ae`; only the two original binding files, with the verifier's four identity constants changed. The guard algorithm is unchanged.

The original v15 command executed once from 2026-10-09 15:37:29.775954 UTC to 15:41:47.584776 UTC and returned exit 0 in 257.724678 seconds. The original task-plan command then returned exit 0 in 0.115581 seconds. Both executions preserve separate before/after input and index snapshots. A subsequent byte-and-mode verification confirms all 1,257 required source, validation, guard and selected review files remained unchanged. The inventory is 1,095 product inputs, 71 original-BASE deltas and 145 validation inputs.

The execution directory was an explicitly materialized required-file projection with real private Git HEAD/index and the original historical proof objects. It was not a complete checkout of all historical archive files. One earlier preparation-only failure incorrectly assumed that every inherited fixture had mode 100644; the identified fixture already had its correct original mode 100755 and exact Git blob. The correction changed only the preparer verification. No guard invocation occurred in that failed preparation, and its record is retained.

Rust formatting, compilation, the new Rust tests and cold-only native measurements were not executed by this admission. Existing author Python controls and the independent source review remain in the fixed R commit. No M7F CI or historical native result is relabeled as a cold-source result. The later root publication of Draft PR #189 does not alter the handoff's truthful earlier statement that its author had not created refs or triggers.

Formal task status remains 163 done and 29 remaining, with zero newly completed tasks. This archive is evidence preservation and source admission, not a complete benchmark or release acceptance.

`payload-manifest.json` records each original source, member, byte count, SHA-256, Git blob OID and mode. The USTAR members are sorted regular files with zero owner/time fields; gzip uses mtime 0. Full reopening and a second serialization checked exact original bytes and deterministic output. The archive SHA-256 is `37799c127697fcf7de42f39da2b75c56541fcffd7178949ded8176f0cecc9f85`.
