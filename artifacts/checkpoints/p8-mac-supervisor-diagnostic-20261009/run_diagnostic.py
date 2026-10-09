#!/usr/bin/env python3
"""One exact E test, then one separately identified Mac Command diagnostic.
No scale/soak execution, retries, source edits, changed thresholds, or TODO credit.
"""
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import signal
import stat
import subprocess
import sys
import time
import traceback

E = "a23bb72d3c954f385b99fe81ce9189885c208557"
TREE = "58147c952505c44da1f41eb4b9c31643f2303b96"
TEST = "subprocess_descendant_cannot_hold_stderr_past_worker_deadline"
HELPER_SHA = "0e366f70d7deb927f196f6a0aeaedf6eda8665ff0a141df03148fab81fba31ae"
EXCLUDED = {
    "scripts/verify_reviewed_source_v15.py",
    "scripts/reviewed-source-registry-v15.json",
    ".github/workflows/ci.yml",
}
REMOVED = (
    "CODECORTEX_WRITE_REAL_BENCHMARK", "CODECORTEX_WRITE_BENCHMARK",
    "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER", "RUST_TEST_THREADS",
)

def now():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()

def write(path, data):
    path.write_text(json.dumps(data, indent=2, sort_keys=True) + "\n")

def digest(path):
    h = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()

def git(root, *args):
    return subprocess.check_output(["git", "-C", str(root), *args])

def snapshot(root):
    assert git(root, "rev-parse", "HEAD").decode().strip() == E
    assert git(root, "rev-parse", "HEAD^{tree}").decode().strip() == TREE
    assert not git(root, "status", "--porcelain", "--untracked-files=no")
    data = git(root, "ls-tree", "-r", "-z", E, "--",
               "Cargo.toml", "Cargo.lock", "crates", "scripts",
               ".github/workflows", "tests/source_integrity")
    rows = []
    products = validations = 0
    for row in data.split(b"\0"):
        if not row:
            continue
        fields, raw_path = row.split(b"\t", 1)
        mode, kind, oid = fields.decode().split()
        path = raw_path.decode()
        assert kind == "blob" and mode in ("100644", "100755"), path
        p = root / path
        assert not p.is_symlink() and p.is_file(), path
        body = p.read_bytes()
        actual_oid = hashlib.sha1(b"blob " + str(len(body)).encode() + b"\0" + body).hexdigest()
        assert actual_oid == oid, path
        executable = bool(p.stat().st_mode & stat.S_IXUSR)
        assert executable == (mode == "100755"), path
        products += path.startswith("crates/") or path in ("Cargo.toml", "Cargo.lock")
        validations += (path.startswith(("scripts/", ".github/workflows/", "tests/source_integrity/"))
                        and path not in EXCLUDED)
        rows.append({"path": path, "mode": mode, "git_blob": oid,
                     "bytes": len(body), "sha256": hashlib.sha256(body).hexdigest()})
    assert products == 1087 and validations == 138, (products, validations)
    return {"source_commit": E, "source_tree": TREE, "product_count": products,
            "validation_count_excluding_original_three_trust_root_paths": validations,
            "inputs_including_those_three_paths": rows}

def run(name, argv, out, cwd, env, timeout):
    command = {"argv": [str(v) for v in argv], "cwd": str(cwd),
               "started_at": now(), "outer_timeout_seconds": timeout,
               "outer_timeout_is_infrastructure_bound_not_test_deadline": True,
               "new_session_for_owned_outer_process": True}
    write(out / (name + ".command.json"), command)
    begin = time.monotonic_ns()
    receipt = {"command": command, "exit_code": None, "timed_out": False,
               "launch_error": None}
    with (out / (name + ".stdout.log")).open("wb") as stdout, \
         (out / (name + ".stderr.log")).open("wb") as stderr:
        try:
            process = subprocess.Popen(command["argv"], cwd=cwd, env=env,
                                       stdout=stdout, stderr=stderr, start_new_session=True)
            receipt["pid"] = process.pid
            try:
                receipt["exit_code"] = process.wait(timeout=timeout)
            except subprocess.TimeoutExpired:
                receipt["timed_out"] = True
                # Kill only the session this driver itself created. The fixed
                # helper's descendant is bounded sleep20; no process-name kill.
                os.killpg(process.pid, signal.SIGKILL)
                receipt["exit_code"] = process.wait()
        except Exception as error:
            receipt["launch_error"] = repr(error)
    receipt.update(finished_at=now(), elapsed_ns=time.monotonic_ns()-begin)
    for stream in ("stdout", "stderr"):
        p = out / (name + "." + stream + ".log")
        receipt[stream] = {"path": p.name, "bytes": p.stat().st_size, "sha256": digest(p)}
    write(out / (name + ".receipt.json"), receipt)
    return receipt

def output_text(out, name):
    return (out / (name + ".stdout.log")).read_text().strip()

def require_success(receipt):
    assert receipt["exit_code"] == 0 and not receipt["timed_out"] and receipt["launch_error"] is None, receipt

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--observer", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--execution-sha", required=True)
    args = parser.parse_args()
    source = args.source.resolve()
    observer_source = args.observer.resolve()
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    result = {"schema_version": 1, "status": "infrastructure_error",
              "diagnostic_only": True, "source_commit": E, "source_tree": TREE,
              "workflow_execution_commit": args.execution_sha,
              "original_test": {"status": "not_run"},
              "observer": {"status": "not_run"},
              "original_mac_failures_superseded": False,
              "full_workspace_pass": False, "scale_or_soak_started": False,
              "formal_todo_completion": False, "done": 163, "remaining": 29,
              "scope": "Hosted macOS 15 arm64 comparative diagnosis; original macOS 26.6.2 failures remain unchanged."}
    final_exit = 2
    env = os.environ.copy()
    before = None
    toolchain_before = None
    try:
        assert re.fullmatch(r"[0-9a-f]{40}", args.execution_sha)
        assert platform.system() == "Darwin" and platform.machine() == "arm64"
        before = snapshot(source)
        write(out / "source-before.json", before)
        # Only the process-local Xcode/SDK selection changes; no xcode-select.
        developer = Path("/Applications/Xcode_16.3.app/Contents/Developer")
        assert developer.is_dir(), "Required Xcode16.3 absent; do not silently substitute SDK."
        env["DEVELOPER_DIR"] = str(developer)
        require_success(run("sdk-path", ["xcrun","--sdk","macosx15.4","--show-sdk-path"],out,source,env,30))
        sdk = Path(output_text(out,"sdk-path"))
        assert sdk.is_dir()
        env["SDKROOT"] = str(sdk)
        require_success(run("sdk-version", ["xcrun","--sdk","macosx15.4","--show-sdk-version"],out,source,env,30))
        assert output_text(out,"sdk-version") == "15.4"
        for name, argv in [
            ("uname", ["uname","-a"]), ("sw-vers", ["sw_vers"]),
            ("xcode", ["xcrun","xcodebuild","-version"]),
            ("rustc-path", ["rustup","which","--toolchain","1.95.0","rustc"]),
            ("cargo-path", ["rustup","which","--toolchain","1.95.0","cargo"]),
        ]:
            require_success(run(name,argv,out,source,env,30))
        rustc = Path(output_text(out,"rustc-path"))
        cargo = Path(output_text(out,"cargo-path"))
        require_success(run("rustc-version",[rustc,"-vV"],out,source,env,30))
        require_success(run("cargo-version",[cargo,"-vV"],out,source,env,30))
        rv = output_text(out,"rustc-version")
        assert "\nrelease: 1.95.0\n" in "\n"+rv+"\n" and "host: aarch64-apple-darwin" in rv
        for key in REMOVED:
            env.pop(key, None)
        env["CARGO_BUILD_JOBS"] = "4"
        env["RUSTC"] = str(rustc)
        toolchain_before = {str(p): {"bytes": p.stat().st_size, "sha256": digest(p)}
                            for p in (rustc, cargo)}
        target = out.parent / (out.name + "-private-cargo-target")
        assert not target.exists(), "Diagnostic requires a fresh target."
        env["CARGO_TARGET_DIR"] = str(target)
        write(out / "environment.json", {
            "process_local": {"DEVELOPER_DIR":str(developer),"SDKROOT":str(sdk),
                              "CARGO_TARGET_DIR":str(target),"CARGO_BUILD_JOBS":"4","RUSTC":str(rustc)},
            "removed_names":list(REMOVED), "sdk_version":"15.4",
            "runner_image":{k:os.environ.get(k) for k in ("ImageOS","ImageVersion","RUNNER_OS","RUNNER_ARCH")},
            "toolchain_binaries":toolchain_before,
            "sdk_settings":{p.name:{"bytes":p.stat().st_size,"sha256":digest(p)}
                            for p in (sdk/"SDKSettings.json",sdk/"SDKSettings.plist") if p.is_file()},
            "does_not_establish_original_host_equivalence":True})
        test_source = (source/"crates/cc-eval/tests/p8_scale.rs").read_text()
        block = test_source.split("fn "+TEST+"()",1)[1].split("\n#[test]",1)[0]
        match = re.search(r'std::fs::write\(\s*&helper,\s*("(?:\\.|[^"\\])*")\s*,\s*\)',block)
        assert match, "Cannot locate exact original helper literal."
        helper_bytes = json.loads(match.group(1)).encode()
        assert len(helper_bytes)==159 and hashlib.sha256(helper_bytes).hexdigest()==HELPER_SHA
        assert "deadline_ms: 250" in block and "Duration::from_secs(2)" in block
        assert 'assert_eq!(report["exit_code"], 0' in block and 'assert_eq!(report["stderr_complete"], true)' in block
        build = run("original-build",[cargo,"test","-p","cc-eval","--test","p8_scale","--locked",
                                     "--no-run","--message-format=json-render-diagnostics"],out,source,env,1200)
        result["original_build"] = build
        if build["exit_code"] == 0 and not build["timed_out"] and build["launch_error"] is None:
            events = [json.loads(line) for line in (out/"original-build.stdout.log").read_text().splitlines()]
            finishes = [v for v in events if v.get("reason")=="build-finished"]
            assert len(finishes)==1 and finishes[0]["success"] is True and events[-1]==finishes[0]
            products = [v for v in events if v.get("reason")=="compiler-artifact"
                        and v.get("target",{}).get("name")=="p8_scale"
                        and v["target"].get("kind")==["test"] and v.get("executable")]
            assert len(products)==1
            product = products[0]
            assert product["fresh"] is False, "Private-target test artifact was unexpectedly fresh."
            binary = Path(product["executable"]).resolve()
            assert binary.is_relative_to(target.resolve()) and product["profile"]["test"] is True
            assert product["profile"]["opt_level"]=="0"
            assert Path(product["manifest_path"]).resolve()==source/"crates/cc-eval/Cargo.toml"
            assert Path(product["target"]["src_path"]).resolve()==source/"crates/cc-eval/tests/p8_scale.rs"
            copied = out/"original-p8-scale-test"
            shutil.copy2(binary,copied)
            assert digest(binary)==digest(copied)
            write(out/"original-binary-receipt.json",{
                "cargo_artifact":product,"actual_binary":str(binary),"copy":copied.name,
                "sha256":digest(binary),"bytes":binary.stat().st_size,
                "source_commit":E,"source_tree":TREE})
            original = run("original-exact-test",[binary,TEST,"--exact","--nocapture"],out,source,env,40)
            result["original_test"] = {"status":"executed",**original}
            assert digest(binary)==digest(copied), "Original test binary changed."
        # Run once even after a real original-test failure. No retry or success selection.
        copied_rs = out/"observer.rs"
        shutil.copy2(observer_source,copied_rs)
        observer_binary = out/"command-observer"
        compile_result = run("observer-build",[rustc,"--edition=2021",copied_rs,"-o",observer_binary],out,source,env,120)
        result["observer_build"] = compile_result
        require_success(compile_result)
        write(out/"observer-binary-receipt.json",{
            "diagnostic_only":True,"source_sha256":digest(copied_rs),
            "binary_sha256":digest(observer_binary),"bytes":observer_binary.stat().st_size})
        helper = out/"stderr-holder.sh"
        helper.write_bytes(helper_bytes)
        helper.chmod(0o700)
        observed = run("command-observer",[observer_binary,helper,out/"observer-run",out/"observer-workspace"],
                       out,source,env,40)
        result["observer"] = {"status":"executed",**observed}
        result["observer_outer_two_second_observation"] = {
            "elapsed_ns": observed["elapsed_ns"],
            "within_two_seconds": observed["elapsed_ns"] < 2_000_000_000,
            "scope": "Driver wall time includes process startup, observer report I/O and wait; not the original test assertion.",
            "used_as_original_test_result": False}
        assert digest(helper)==HELPER_SHA
        report_path = out/"observer-run/report.json"
        if report_path.is_file():
            result["observer_report"]={"path":"observer-run/report.json","sha256":digest(report_path),
                                       "bytes":report_path.stat().st_size}
        result["status"]="diagnostic_commands_finished"
        # A captured original failure remains a failing workflow.
        original_ok = result["original_test"].get("exit_code")==0
        observer_ok = observed["exit_code"]==0 and not observed["timed_out"]
        final_exit=0 if original_ok and observer_ok else 1
    except BaseException as error:
        result["infrastructure_error"]=repr(error)
        (out/"driver-error.log").write_text(traceback.format_exc())
    finally:
        try:
            after = snapshot(source)
            write(out/"source-after.json",after)
            result["source_before_after_equal"]=before is not None and before==after
            if not result["source_before_after_equal"]:
                final_exit=2
        except BaseException as error:
            result["source_after_error"]=repr(error)
            final_exit=2
        if toolchain_before is not None:
            try:
                toolchain_after = {name: {"bytes": Path(name).stat().st_size, "sha256": digest(Path(name))}
                                   for name in toolchain_before}
                write(out/"toolchain-after.json", toolchain_after)
                result["toolchain_before_after_equal"] = toolchain_before == toolchain_after
                if not result["toolchain_before_after_equal"]:
                    final_exit=2
            except BaseException as error:
                result["toolchain_after_error"]=repr(error)
                final_exit=2
        result["finished_at"]=now()
        result["driver_exit_code"]=final_exit
        write(out/"result.json",result)
        inventory=[]
        for p in sorted(out.rglob("*")):
            if p.is_file() and not p.is_symlink():
                inventory.append({"path":p.relative_to(out).as_posix(),"bytes":p.stat().st_size,
                                  "sha256":digest(p),"mode":stat.S_IMODE(p.stat().st_mode)})
        write(out/"inventory.json",{"scope":"All output regular files except this self-manifest; no private Cargo target.",
                                     "files":inventory})
    return final_exit

if __name__=="__main__":
    sys.exit(main())
