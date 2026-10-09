# Exact late-evidence custody

This directory preserves the complete original inventory of 798 logical files.
Use inventory.json unchanged (515025 bytes; Git blob f8f31e705671917602baba778fd4250df9421d7e;
SHA256 d3afe1f9ca400fed9b2431d29fbc658e1f2f0dc04a6041c4533453630f42d1aa).

The 754 non-ZIP logical paths map to 330 distinct payloads in small-evidence.tar.gz.
The 44 logical ZIP paths map to 29 distinct original ZIPs. All corresponding
43 whole-file or chunk objects appear under objects/ with their original
physical_pool.objects pool_path. One shared 4MiB segment is stored once;
two original build ZIPs of the same size remain distinct whole-file identities.

See small-evidence.receipt.json for every tar member and its verification.
See upload-receipt.json for all actual successful Git blob uploads and exact
restoration rules. Reconstruct ZIP segments by the original segment_plan.parts
order and offset, then verify every original whole ZIP byte count, SHA256 and
Git blob identity before mapping it to the inventory's original logical paths.
Do not treat the smaller object-byte total as the size of the reconstructed ZIPs.

Restore only to a new isolated directory; reject absolute or traversal paths,
symlinks and overwrites. Restore original modes from inventory.json rather than
the normalized tar mode. No archived script needs to run to restore bytes.
All 798 original records, including failures and pending/partial outcomes,
retain their original byte contents. This custody operation does not grant
measurement coverage or original TODO acceptance.

The prior recoverable-copy cleanup records remain unchanged. No additional
local files, branches, tags, runs or study attempts were removed here.
