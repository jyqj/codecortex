# Original local P8 history: text transport

The original history archive is stored as `p8-local-history.bundle.b64` so it can be delivered through text-only Git tree writes. This is a transport encoding of the original bundle. No commits or Git pack data were regenerated.

## Restore and verify

Run the dependency-free Python script from any directory:

```sh
python3 artifacts/checkpoints/p8-local-history-20261007/restore_bundle.py
```

By default it creates `p8-local-history.bundle` beside the script. The output must not already exist; the script uses exclusive creation and refuses to overwrite. An optional `--output /path/to/new.bundle` selects a different new file. Input reads are bounded by the fixed transport size, Base64 formatting is checked, and both encoded and restored bytes are verified against fixed SHA256 values.

The restored bundle is **560,244 bytes**, with SHA256 `0db458b30244f383c1658db83cb97728b15b340289011d2c83d328ef69694bc4`. Its Base64 uses the standard alphabet, 76 characters per line, LF line endings and a final LF. Exact encoded size and SHA256 are in `transport-manifest.json`.

Use an existing repository containing the excluded baseline and its ancestors:

```sh
git cat-file -e 6d02d77f018a5965a6f289b0b43558ed4b9f8322^{commit}
git bundle verify /path/to/restored/p8-local-history.bundle
git fetch --no-tags /path/to/restored/p8-local-history.bundle \
  'refs/heads/work/*:refs/archive/p8-local-history-20261007/*'
```

The archive contains the original six fixed heads and 47 commits. It requires the excluded baseline `6d02d77f018a5965a6f289b0b43558ed4b9f8322` and does not contain its own later evidence or transport commits. Importing into archive refs preserves original identities without rewriting work branches. CI and source admission continue to require their actual source/review pins; neither decoding nor verifying this bundle grants admission.

## Original receipts remain immutable

`manifest.json`, `SHA256SUMS`, create/verify/list-heads stdout and stderr, the bundle header, before/after heads and the commit inventory are unchanged from commit `03581be10fb27b3c4cdad0a7e47676659bd7b47a`. Their bundle filename refers to the restored original bytes. They describe the original creation event, not this text transport.

The original `README.md` is preserved byte-for-byte as `README.original.md`, including its original six-head table. Accordingly, the old `SHA256SUMS` remains an original-snapshot receipt: its `README.md` entry checks `README.original.md`, and its bundle entry checks the restored archive. Do not interpret a whole-directory check against that historical list as a checksum of this updated README. Current transport files have a separate `transport-SHA256SUMS` list.

`transport-validation.json` records actual default restoration in a temporary directory, byte-for-byte comparison with the retained original binary, fixed SHA256 checks, a second invocation refusing the existing output, and successful `git bundle verify`. Temporary test copies were removed afterward; the original binary was retained separately in this session's scratch workspace.
