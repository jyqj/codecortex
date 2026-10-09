# Fixed C3ff scale original ZIP custody

This sibling collection preserves one original release build and four already accepted repetition-0 shards from run 37910924354 at 3ffcefc3b28ee1a4ed80caecebd7208a45c3e302. The originals are split into ordered 2 MiB chunks with exact whole and chunk hashes; no ZIP is repacked. The existing 26-artifact custody tree and catalog remain unchanged.

The input copies were recovered by repeated transport of the same immutable Actions artifact IDs after scratch loss. No workload or already accepted shard was rerun. The accepted scope remains 4/150 shards and 41/1500 samples. The original 100k job and complete matrix remain pending; N30, all budgets and the original validator are unchanged.

Use the unchanged restore_original_zip.py with one manifests/<artifact-id>.json and a previously absent --output path. It verifies every chunk and the full ZIP, does not extract archives, and refuses an existing output.
