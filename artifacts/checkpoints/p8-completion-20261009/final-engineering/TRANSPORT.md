# Byte-preserving transfer wrapper

This layout changes only how the original engineering archive is transported. It preserves the original eleven files byte for byte. The original `README.md`, `DELIVERABLES.json`, manifests, source identities, validation outcomes and verifier are unchanged.

The two original tar files and `sources.bundle` are stored as thirteen ordered chunks under `chunks/`. Every nonfinal chunk is exactly **4,194,303 bytes** (4 MiB minus one), divisible by three so that complete base64 encodings can be split at compatible boundaries. Final chunks may be smaller. `transport-manifest.json` records each original file's complete SHA256 and byte length, and every chunk's order, offset, length and SHA256. The other eight original files remain directly available in this directory.

Reconstruct into a new directory before invoking the unchanged original archive verifier:

```sh
python3 -B assemble_transport.py --output /path/to/new-reassembled-archive
python3 -B /path/to/new-reassembled-archive/verify_archive.py --runtime --repo /path/to/codecortex
```

The second command verifies both runtime build/observation pairs with their retained original offline verifiers and verifies the incremental bundle against a repository containing prerequisite `7354db236c9d9850a75f31672697ae9eab44565e`. It does not start a product measurement. Omit `--repo` when the prerequisite repository is unavailable; all archive bytes and runtime pairs can still be verified.

Do not run the original verifier against this transport directory: the original three complete binary files intentionally exist only after reassembly. The assembler refuses an existing output directory, verifies every ordered chunk and complete-file hash, and restores only the original eleven files. It does not modify any source evidence or the input transport layout.

This transfer wrapper provides no new benchmark, task-completion, platform, release or P2 approval. The original archive records all limitations and failures unchanged.
