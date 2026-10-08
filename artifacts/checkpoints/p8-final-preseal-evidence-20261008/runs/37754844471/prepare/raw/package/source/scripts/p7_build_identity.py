"""Fixed crate/Cargo identity for P7 build receipts; no source admission or tests."""
import hashlib
import json
from pathlib import Path
import re
import stat
import subprocess


def json_bytes(value):
    return (json.dumps(value, ensure_ascii=False, sort_keys=True, indent=2) + "\n").encode()


def file_sha256(path):
    digest = hashlib.sha256()
    with Path(path).open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def git(root, *args, data=None):
    return subprocess.check_output(["git", *args], cwd=root, input=data,
                                   stderr=subprocess.PIPE)


def source_snapshot(root):
    """Reject uncommitted/missing/extra/symlink inputs and hash the fixed source.

    The complete input inventory is the repository's existing crate/Cargo
    inventory. Compiler, profile and artifact identity are recorded separately;
    this is not a claim of a hermetic build or approved behavior.
    """
    root = Path(root).resolve(strict=True)
    if not stat.S_ISDIR((root / "crates").lstat().st_mode):
        raise ValueError("non-regular crates inventory root")
    commit = git(root, "rev-parse", "--verify", "HEAD^{commit}").decode().strip()
    if not re.fullmatch(r"[0-9a-f]{40}", commit):
        raise ValueError("source commit is not an immutable full SHA")
    raw_entries = git(root, "ls-tree", "-r", "-z", commit, "--",
                      "crates", "Cargo.toml", "Cargo.lock")
    blobs = {}
    for entry in raw_entries.split(b"\0"):
        if not entry:
            continue
        metadata, path = entry.split(b"\t", 1)
        mode, kind, blob = metadata.decode().split()
        if kind != "blob" or mode not in ("100644", "100755"):
            raise ValueError("non-regular committed build input")
        blobs[path.decode()] = blob
    if not {"Cargo.toml", "Cargo.lock"}.issubset(blobs) or not any(
            p.startswith("crates/") for p in blobs):
        raise ValueError("missing crate/Cargo input inventory")
    tracked = {p.decode() for p in git(root, "ls-files", "-z", "--",
                                       "crates", "Cargo.toml", "Cargo.lock").split(b"\0") if p}
    disk = {p.relative_to(root).as_posix() for p in (root / "crates").rglob("*")
            if not p.is_dir() or p.is_symlink()}
    if tracked != set(blobs) or disk != {p for p in blobs if p.startswith("crates/")}:
        raise ValueError("current crate/Cargo input inventory differs from the fixed commit")

    # One batch avoids spawning a git process for every source file. Git supplies
    # each blob's exact size; embedded newlines and Unicode are not delimiters.
    objects = list(dict.fromkeys(blobs.values()))
    raw = git(root, "cat-file", "--batch", data=("\n".join(objects) + "\n").encode())
    offset, expected_hashes = 0, {}
    for expected in objects:
        end = raw.index(b"\n", offset)
        oid, kind, size = raw[offset:end].decode().split()
        if oid != expected or kind != "blob":
            raise ValueError("unexpected git blob response")
        size, offset = int(size), end + 1
        expected_hashes[oid] = hashlib.sha256(raw[offset:offset + size]).hexdigest()
        offset += size
        if raw[offset:offset + 1] != b"\n":
            raise ValueError("truncated git blob response")
        offset += 1
    if offset != len(raw):
        raise ValueError("unexpected trailing git blob data")

    manifest = {}
    for path, blob in sorted(blobs.items()):
        target = root / path
        if not stat.S_ISREG(target.lstat().st_mode):
            raise ValueError("non-regular live build input: " + path)
        digest = file_sha256(target)
        if digest != expected_hashes[blob]:
            raise ValueError("uncommitted build input: " + path)
        manifest[path] = digest
    return {
        "source_commit": commit,
        "source_tree": git(root, "rev-parse", commit + "^{tree}").decode().strip(),
        "input_count": len(manifest),
        "manifest_sha256": hashlib.sha256(json_bytes(manifest)).hexdigest(),
        "inputs": manifest,
    }


def verify_cargo_artifact(root, artifact, package_kind):
    root = Path(root).resolve(strict=True)
    expected = {"default": [], "semantic": ["semantic"]}[package_kind]
    target = artifact.get("target", {})
    package, separator, version = artifact.get("package_id", "").rpartition("#")
    if (artifact.get("reason") != "compiler-artifact"
            or target.get("name") != "codecortex" or target.get("kind") != ["bin"]
            or sorted(artifact.get("features", [])) != expected
            or not separator or not version or not package.endswith("/cc-server")
            or artifact.get("profile", {}).get("test") is not False):
        raise ValueError("Cargo artifact does not identify the requested production package")
    paths = {
        artifact.get("manifest_path"): root / "crates/cc-server/Cargo.toml",
        target.get("src_path"): root / "crates/cc-server/src/main.rs",
    }
    if len(paths) != 2 or any(not actual or Path(actual).resolve(strict=True) != wanted
                              for actual, wanted in paths.items()):
        raise ValueError("Cargo artifact belongs to a different source checkout")
    executable = artifact.get("executable")
    if not executable or not Path(executable).is_file():
        raise ValueError("Cargo artifact has no production executable")
    return Path(executable).resolve(strict=True)


def verify_release_profile(profile):
    """Require actual optimized release fields; the argv label is insufficient."""
    if (not isinstance(profile, dict)
            or profile.get("opt_level") not in ("1", "2", "3", "s", "z")
            or profile.get("debug_assertions") is not False
            or profile.get("test") is not False):
        raise ValueError("actual Cargo release profile is not optimized")
