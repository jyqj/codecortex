# Independent installed source approval check after bounded reclaim

Reviewer: `/root/p7_wiring`. This check reads fixed Git blobs and current bytes;
it does not import the guarded implementation, run Cargo, or claim current runtime
acceptance. The original source and validation approval logic remains unchanged.

The first aggregate review at `3d50314976c8a5e4eea65fae433b8755b6f3db00`
contained the correct product commit and 795 input hashes, but retained an older
`source_tree`. The reviewer rejected that metadata inconsistency. Its fixed-commit
finding is preserved. The initial partial receipt is also retained with its
specific omitted check documented in the finding; it is not the final approval.

The corrected final check returned exit 0 against:

- Product: `09291fdf4d968b0929d598cd5df6a3d1fbd3d6cc`.
- Product tree: `8f47f647d327221adcdec103bb1ee756d029fda5`.
- Independent review: `7b5d6f1fa436f4c655b0c0fafc66b521faeb480e`.
- Registry SHA-256: `b9e520984d04241104ead7cd26635967d9ce4d5f8f8700ce57b9b1d24c03e31a`.

The verifier independently hashes all 795 source inputs, 35 source deltas and 99
validation inputs, verifies the entire referenced review-record set, binds the
non-author reclaim review to its four exact source deltas and equal local/remote
product tree, and checks all 14 exact function source hashes. Only the three fixed
pin assignment values differ from the previously reviewed guard; the normalized
whole-file SHA-256 is still
`e6c98069a125a8990d458087b8f61aaed77a061c565b1a2edb7281ac127a08e6`.
All 99 independently selected validation input bytes match the prior fixed CI head.

One early final recheck could not read the new fixed review path from Git. That
actual failed read and successful subsequent tree/byte recheck are recorded in
`object-read-followup.json`, without inventing a cause or treating unavailable
proof as success.

The root agent owns the actual full historical proof CLI and new CI execution.
This artifact does not inherit old runtime results for changed source, close a
TODO, or approve P8 corpus/family/custody work. This commit contains only P7 review
artifacts; the public DEV preparation branch remains separate.
