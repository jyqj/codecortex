"""Continuation preflight: read existing toolchain/cache; no cargo build or home changes."""
import hashlib
import json
import os
import pathlib
import subprocess
import tomllib

ROOT = pathlib.Path(__file__).resolve().parent
TOOLCHAIN = pathlib.Path("/workspace/.rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu")
CACHE = pathlib.Path("/workspace/.cargo/registry/cache/index.crates.io-1949cf8c6b5b557f")
SOURCE = pathlib.Path("/workspace/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f")
SHAS = {
    "v22": "574f7598662334c63e020da136c87f4f7281554d",
    "v23": "57bedbaf193e28be34271a290d28c90dc662b613",
    "v24": "9ebdb155c64e094b3d774be41dbe7ab8c4e222c1",
}


def main():
    out = dict(status="BLOCKED_MISSING_EXISTING_LOCKED_CACHE", toolchains={}, cache_checks=[],
               home_variables_changed=False, rustup_called=False, build_commands_started=0,
               product_builds=0, product_reopen_runs=0, product_pinned_runs=0,
               new_write_denials=0, alternate_cache_used=False, dependency_downloads=0)
    for name in ("rustc", "cargo"):
        path = TOOLCHAIN / "bin" / name
        result = subprocess.run([str(path), "-Vv"], capture_output=True, text=True, check=True)
        stat = path.stat()
        out["toolchains"][name] = dict(path=str(path), version_output=result.stdout,
                                      exit_code=result.returncode, regular_file=path.is_file(),
                                      sha256=hashlib.sha256(path.read_bytes()).hexdigest(),
                                      bytes=stat.st_size, mtime_ns=stat.st_mtime_ns)
    manifest = TOOLCHAIN / "lib/rustlib/multirust-channel-manifest.toml"
    channel = tomllib.loads(manifest.read_text())
    out["existing_installation"] = dict(manifest_path=str(manifest), manifest_date=channel["date"],
        components=(TOOLCHAIN / "lib/rustlib/components").read_text().splitlines(),
        declared_distribution_urls={name: channel["pkg"][name]["target"]["x86_64-unknown-linux-gnu"]["url"]
                                    for name in ("cargo", "rustc")},
        downloaded_or_authenticated_here=False)
    out["observed_home_state"] = {key: os.environ.get(key) for key in ("HOME", "CARGO_HOME", "RUSTUP_HOME")}
    out["existing_cache_roots"] = dict(archive=str(CACHE), extracted_source=str(SOURCE))
    for version, sha in SHAS.items():
        source = ROOT / "runtime" / version / "source"
        lock_path = source / "Cargo.lock"
        lock = tomllib.loads(lock_path.read_text())
        dependencies = [p for p in lock["package"] if p.get("source", "").startswith("registry+")]
        missing = []
        for p in dependencies:
            identity = p["name"] + "-" + p["version"]
            archive = CACHE / (identity + ".crate")
            extracted = SOURCE / identity
            if not archive.is_file() or not extracted.is_dir():
                missing.append(dict(package=p["name"], version=p["version"],
                                    archive_exists=archive.is_file(), source_exists=extracted.is_dir()))
        server = tomllib.loads((source / "crates/cc-server/Cargo.toml").read_text())
        mandatory = {p["name"] for p in dependencies if p["name"] in server["dependencies"]
                     and not server["dependencies"][p["name"]].get("optional", False)}
        mandatory_missing = [p for p in missing if p["package"] in mandatory]
        assert any(p["package"] == "lru" and p["version"] == "0.18.2" for p in mandatory_missing)
        assert any(p["package"] == "notify" and p["version"] == "8.2.0" for p in mandatory_missing)
        out["cache_checks"].append(dict(source_sha=sha, version=version,
            lock_sha256=hashlib.sha256(lock_path.read_bytes()).hexdigest(),
            registry_packages=len(dependencies), missing_count=len(missing),
            all_locked_missing=missing, default_server_direct_missing=mandatory_missing,
            classification="full lock includes optional and non-host packages; direct mandatory blockers are separate"))
    (ROOT / "continuation-inventory.json").write_text(json.dumps(out, ensure_ascii=False, indent=2) + "\n")
    print(json.dumps(dict(status=out["status"], versions={k: v["version_output"].splitlines()[0]
        for k, v in out["toolchains"].items()},
        mandatory_missing={r["version"]: [p["package"] + "-" + p["version"]
            for p in r["default_server_direct_missing"]] for r in out["cache_checks"]}, product_runs=0)))


if __name__ == "__main__":
    main()
