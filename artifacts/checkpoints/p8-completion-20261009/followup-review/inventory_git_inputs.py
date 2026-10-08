#!/usr/bin/env python3
"""Create a read-only, unapproved inventory from fixed codecortex Git blobs.

The output contains hashes, modes and path metadata, never corpus file bodies.
Final source approval is a separate human/agent review after actual validation.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess


BASE = "7354db236c9d9850a75f31672697ae9eab44565e"
SOURCE_ROOTS = ("crates", "Cargo.toml", "Cargo.lock")
VALIDATION_ROOTS = ("scripts", ".github/workflows", "tests/source_integrity")
VALIDATION_EXCLUSIONS = frozenset({
    "scripts/verify_reviewed_source_v15.py",
    "scripts/reviewed-source-registry-v15.json",
    ".github/workflows/ci.yml",
})


def inventory(repository, source):
    def git(*argv, data=None):
        return subprocess.check_output(["git", "--no-pager", "-C", str(repository), *argv], input=data)

    def entries(ref, roots):
        result = {}
        for row in git("ls-tree", "-r", "-z", ref, "--", *roots).split(b"\0"):
            if not row:
                continue
            metadata, path = row.decode().split("\t", 1)
            mode, kind, oid = metadata.split()
            if kind != "blob" or mode not in ("100644", "100755"):
                raise ValueError("nonregular input: " + path)
            result[path] = {"mode": mode, "blob": oid}
        return result

    if re.fullmatch(r"[0-9a-f]{40}", source) is None:
        raise ValueError("source must be an immutable full SHA")
    current = entries(source, SOURCE_ROOTS)
    inherited = entries(BASE, SOURCE_ROOTS)
    # This path-only check runs before cat-file reads any source body.
    forbidden = [p for p in {*current, *inherited}
                 if re.search(r"hold[-_]?out|held[-_]?out", p, re.I)]
    if forbidden:
        raise ValueError("refusing to read a heldout body path: " + repr(forbidden))
    validation = {p: row for p, row in entries(source, VALIDATION_ROOTS).items()
                  if p not in VALIDATION_EXCLUSIONS
                  and not ("__pycache__" in Path(p).parts and p.endswith(".pyc"))}
    oids = sorted({row["blob"] for domain in (current, inherited, validation) for row in domain.values()})
    raw = git("cat-file", "--batch", data="".join(oid + "\n" for oid in oids).encode())
    cursor, hashes, sizes = 0, {}, {}
    for requested in oids:
        end = raw.index(b"\n", cursor)
        actual, kind, size_text = raw[cursor:end].decode().split()
        size = int(size_text)
        if actual != requested or kind != "blob" or size < 0:
            raise ValueError("unexpected batched Git blob response")
        body = raw[end + 1:end + 1 + size]
        cursor = end + 1 + size
        if len(body) != size or raw[cursor:cursor + 1] != b"\n":
            raise ValueError("truncated Git blob")
        cursor += 1
        hashes[requested], sizes[requested] = hashlib.sha256(body).hexdigest(), size
    if cursor != len(raw):
        raise ValueError("unparsed Git blob bytes")
    complete = {p: hashes[row["blob"]] for p, row in sorted(current.items())}
    before = {p: hashes[row["blob"]] for p, row in sorted(inherited.items())}
    changes = set(git("diff", "--name-only", BASE, source, "--", *SOURCE_ROOTS).decode().splitlines())
    if not set(inherited) <= set(current):
        raise ValueError("unexpected inherited source deletion")
    delta = {p: {"before_sha256": before.get(p), "sha256": complete[p]} for p in sorted(changes)}
    return {
        "schema_version": 1,
        "classification": "mechanically verified immutable input inventory; not a source approval or execution attestation",
        "source": source,
        "source_tree": git("rev-parse", source + "^{tree}").decode().strip(),
        "base": BASE,
        "verdict": "pending_review_and_actual_validation",
        "complete_inputs": complete,
        "paths": delta,
        "validation_inputs": {p: hashes[row["blob"]] for p, row in sorted(validation.items())},
        "validation_exclusions": sorted(VALIDATION_EXCLUSIONS),
        "source_modes": {p: row["mode"] for p, row in sorted(current.items())},
        "validation_modes": {p: row["mode"] for p, row in sorted(validation.items())},
        "inherited_input_count": len(inherited),
        "complete_input_count": len(current),
        "changed_source_input_count": len(changes),
        "validation_input_count": len(validation),
        "source_bytes": sum(sizes[row["blob"]] for row in current.values()),
        "validation_bytes": sum(sizes[row["blob"]] for row in validation.values()),
        "heldout_corpus_bodies_read": False,
        "method": "independent ls-tree mode/path inventory; fixed Git blob bytes hashed with SHA-256; source delta BASE..source; no worktree mutation or contents copied",
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repository", required=True, type=Path)
    parser.add_argument("--source", required=True)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    result = inventory(args.repository, args.source)
    with args.output.open("x", encoding="utf-8") as stream:
        json.dump(result, stream, ensure_ascii=False, sort_keys=True, indent=2)
        stream.write("\n")
    print(json.dumps({k: result[k] for k in ("source", "source_tree", "verdict", "complete_input_count", "changed_source_input_count", "validation_input_count", "heldout_corpus_bodies_read")}, sort_keys=True))


if __name__ == "__main__":
    main()
