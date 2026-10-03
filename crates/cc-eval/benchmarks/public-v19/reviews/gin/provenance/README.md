# Independent upstream admission supplement

The authorized shallow fetch from `https://github.com/gin-gonic/gin.git` returned exact commit `43fe48e8a0f44af783116cdb010725e6bb50255f`. Its commit object hash and tree `895f98dfe0d3f21c3a55c0814995eedaf4989c99` were verified independently. The upstream tree inventory matches all 130 manifest paths, and the actual Go module identifies `github.com/gin-gonic/gin`.

All 53 admitted handwritten, non-test Go files match upstream both byte for byte and by Git blob identity; each retains its MIT notice. The original root MIT license matches the snapshot, declared lock, SHA256 and size. Admission for the provenance and license of these 53 files is accepted. The 77 excluded files remain excluded; their bodies were not reviewed. Dependencies or excluded files do not inherit this admission.

Receipt SHA256: `9c64ebcfdf6879a5f209e1ec48e7c7292a48c744d8e8c2200d4366b060823427`.

This supplement resolves only `U_UPSTREAM_GIT_ORIGIN_NOT_INDEPENDENTLY_REPLAYED` from the frozen dev-only review. It does not rewrite that historical receipt or its 63 accept / 4 needschange decisions. Global components and the 33 custody-blocked holdout cases remain open. This supplement read no benchmark query bodies and performed no ranking. No permission rejection occurred, and no rejected source channel was retried.

For replay, initialize a fresh bare Git repository, add the above URL as `origin`, and fetch the exact SHA with `--depth=1`. Then run from the Codecortex root:

```sh
python3 crates/cc-eval/benchmarks/public-v19/reviews/gin/provenance/verify_upstream.py --upstream-git /path/to/upstream.git --check
```

The script accepts only the fixed author manifest, source lock, root license and admitted source paths. It reads no query, holdout, historical gold or product files. It never performs a network request itself.
