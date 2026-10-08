#!/usr/bin/env python3
"""Run the original public corpus checks using the actual fixed joint release package."""
import argparse,hashlib,json,os,re,stat,subprocess,sys,time,zipfile
sys.dont_write_bytecode=True
from pathlib import Path
SOURCE="2cd04485b0e0b23483f64671d56538bbaa47f442"
PREFIX="artifacts/checkpoints/p8-public-dev-recovery-20261008/replay/"
def need(value,message):
    if not value: raise ValueError(message)
def sha(raw): return hashlib.sha256(raw).hexdigest()
def write(path,value): path.write_text(json.dumps(value,indent=2,sort_keys=True)+"\n")
def read(path,maximum=512*1024*1024):
    need(not path.is_symlink() and path.is_file() and path.stat().st_size<=maximum,"regular bounded input")
    return path.read_bytes()
def git(root,*args):
    return subprocess.check_output(["git","-C",str(root),*args],stderr=subprocess.PIPE,timeout=120)
def command(out,label,argv,cwd,timeout):
    started=time.time(); timed_out=False
    try:
        p=subprocess.run([str(x) for x in argv],cwd=cwd,stdin=subprocess.DEVNULL,stdout=subprocess.PIPE,stderr=subprocess.PIPE,
            timeout=timeout,env=dict(os.environ,PYTHONDONTWRITEBYTECODE="1",GIT_NO_LAZY_FETCH="1",GIT_TERMINAL_PROMPT="0"))
        code=p.returncode; stdout=p.stdout; stderr=p.stderr
    except subprocess.TimeoutExpired as e:
        code=None; timed_out=True; stdout=e.stdout or b""; stderr=e.stderr or b""
    (out/(label+".stdout")).write_bytes(stdout); (out/(label+".stderr")).write_bytes(stderr)
    receipt={"argv":[str(x) for x in argv],"cwd":str(cwd),"started_unix":started,"finished_unix":time.time(),
        "exit_code":code,"timed_out":timed_out,"timeout_seconds":timeout,"stdout_sha256":sha(stdout),"stderr_sha256":sha(stderr)}
    write(out/(label+".json"),receipt)
    need(code==0 and not timed_out,"original command failed: "+label)
    return receipt
def run(a):
    root=a.source_root.resolve(strict=True); output=a.output.absolute()
    need(not output.exists() and not output.is_symlink(),"fresh output required")
    need(not output.is_relative_to(root) and not root.is_relative_to(output),"output separate from source")
    need(git(root,"rev-parse","HEAD").decode().strip()==SOURCE,"fixed joint source")
    need(not git(root,"status","--porcelain=v1","--untracked-files=all").strip(),"clean source")
    output.mkdir(parents=True); result={"status":"started","source":SOURCE,"ranking_runs":0,"protected_body_reads":0,"task_acceptance":"pending"}
    try:
        manifest_raw=read(a.manifest); need(sha(manifest_raw)==a.manifest_sha256 and re.fullmatch("[0-9a-f]{64}",a.manifest_sha256),"fixed package manifest")
        m=json.loads(manifest_raw); need(m["product_source"]==m["evidence_source"]==SOURCE,"package source")
        need(m["candidate_build_validation_status"]=="actual_same_source_release_builds_verified","actual release build validation")
        archive=read(a.archive); need(len(archive)==m["archive"]["bytes"] and sha(archive)==m["archive"]["sha256"],"original release package bytes")
        validator=output/"validator"; validator.mkdir()
        selected={}
        with zipfile.ZipFile(a.archive) as z:
            need(len(z.namelist())==len(set(z.namelist())),"unique package inventory")
            for role in ("evaluator","evaluator_receipt","evaluator_raw_build","evaluator_source_inputs","candidate_build_validation"):
                pin=m["tools"][role]; info=z.getinfo(pin["member"])
                need(not info.is_dir() and not stat.S_ISLNK(info.external_attr>>16),"regular package member")
                raw=z.read(info); need(len(raw)==pin["bytes"] and sha(raw)==pin["sha256"],"fixed package member")
                names={"evaluator":"cc-eval","evaluator_receipt":"build-receipt.json","evaluator_raw_build":"cargo-build.jsonl",
                    "evaluator_source_inputs":"source-inputs.json","candidate_build_validation":"candidate-build-validation.json"}
                destination=validator/names[role]; destination.write_bytes(raw)
                destination.chmod(0o755 if role=="evaluator" else 0o644)
                selected[role]={"path":str(destination),**pin}
        receipt=json.loads(read(validator/"build-receipt.json"))
        need(receipt["source_before"]==receipt["source_after"] and receipt["source_before"]["source_commit"]==SOURCE,"actual evaluator source")
        need(receipt["binary_sha256"]==m["tools"]["evaluator"]["sha256"] and receipt["build_exit_code"]==0,"actual evaluator build")
        # Reuse the original fixed build proof, including actual Cargo/source-map/profile checks.
        for path,item in m["helpers"].items():
            need(sha(read(root/path))==item["sha256"],"fixed package helper differs from candidate")
        sys.path.insert(0,str(root/"scripts"))
        import p8_candidate_execution as candidate
        need(Path(candidate.__file__).resolve()==root/"scripts/p8_candidate_execution.py","fixed original build checker")
        expected=candidate.source_summary(root)
        build=candidate.validate_build(validator/"cc-eval",validator/"build-receipt.json",root,expected,"runner")
        need(build["receipt"]["build_profile"]=="release","actual release evaluator required")
        validation=json.loads(read(validator/"candidate-build-validation.json"))
        need(validation["status"]=="actual_same_source_release_builds_verified" and validation["source"]==expected,
            "original same-source candidate validation")
        for key,value in build.items():
            if key!="receipt":
                need(validation["builds"]["runner"][key]==value,"original build proof receipt differs")
        write(validator/"actual-build-revalidation.json",{"source":expected,"validator":"scripts/p8_candidate_execution.py:validate_build",
            "proof":{k:v for k,v in build.items() if k!="receipt"},"actual_release_profile":build["receipt"]["actual_cargo_profile"],
            "no_rebuild":True})
        (validator/"manifest.json").write_bytes(manifest_raw)
        write(validator/"provenance.json",{"source":SOURCE,"manifest_sha256":sha(manifest_raw),"archive":m["archive"],"selected":selected,
            "scope":"Previously executed joint release binary, used here only for original corpus validation; no new ranking or build."})
        commands=json.loads(read(root/(PREFIX+"commands.json")))
        fetches=[]
        for ref in commands["history_objects_required"]:
            need(re.fullmatch("[0-9a-f]{40}",ref),"immutable history input")
            have=subprocess.run(["git","-C",str(root),"cat-file","-e",ref+"^{commit}"],capture_output=True)
            if have.returncode:
                fetches.append(command(output,"fetch-"+ref,["git","-C",root,"fetch","--no-tags","origin",ref],root,120))
            git(root,"cat-file","-e",ref+"^{commit}")
        write(output/"history-objects.json",{"required":commands["history_objects_required"],"actual_fetches":fetches,"all_present":True})
        evaluator=validator/"cc-eval"; evaluator_sha=m["tools"]["evaluator"]["sha256"]
        original=command(output,"original-command",[sys.executable,root/(PREFIX+"replay_original_public_dev.py"),
            "--repo-root",root,"--evaluator",evaluator,"--evaluator-sha256",evaluator_sha,"--output",output/"original"],root,1800)
        projection=command(output,"projection-command",[sys.executable,root/(PREFIX+"project_reviewed_public_dev.py"),
            "--repo-root",root,"--evaluator",evaluator,"--evaluator-sha256",evaluator_sha,
            "--original-replay-receipt",output/"original/receipt.json","--output",output/"reviewed"],root,1800)
        old=json.loads(read(output/"original/receipt.json")); new=json.loads(read(output/"reviewed/receipt.json"))
        need(old["status"]=="original_public_dev_replay_passed" and len(old["suites_validated"])==16,"all sixteen original V02 checks")
        need(new["status"]=="reviewed_six_repository_public_dev_projection_validated","original projection verdict")
        inventory=new["canonical_product_inventory"]; need(len(inventory)==182,"complete canonical inventory")
        compared=[]
        for record in inventory:
            path=record["path"]
            need(path.startswith("crates/cc-eval/benchmarks/public-v19/") and ".." not in Path(path).parts and not Path(path).is_absolute(),"canonical path")
            actual=root/path; produced=output/"reviewed"/path; original_raw=read(actual); fresh_raw=read(produced)
            need(original_raw==fresh_raw and len(fresh_raw)==record["bytes"] and sha(fresh_raw)==record["sha256"],"canonical bytes differ")
            need(oct(actual.stat().st_mode&0o777)==record["mode"],"canonical mode differs")
            compared.append({"path":path,"bytes":len(original_raw),"sha256":sha(original_raw),"mode":record["mode"]})
        need(sha(read(evaluator))==evaluator_sha,"evaluator changed")
        need(git(root,"rev-parse","HEAD").decode().strip()==SOURCE and not git(root,"status","--porcelain=v1","--untracked-files=all").strip(),"source changed")
        result.update(status="joint_original_and_canonical_public_validation_passed",evaluator_sha256=evaluator_sha,
            source_tree=git(root,"rev-parse","HEAD^{tree}").decode().strip(),original_v02_count=16,canonical_v02_count=12,
            original_receipt_sha256=sha(read(output/"original/receipt.json")),projection_receipt_sha256=sha(read(output/"reviewed/receipt.json")),
            schema_receipt_sha256=sha(read(output/"reviewed/original-schema-and-v02.json")),totals=new["totals"],canonical_byte_mode_comparison=compared,
            original_command=original,projection_command=projection,scope="Public DEV source/gold projection and original V02 checks only; ranking, clean heldout, G8 and release quality are not inferred.")
    except Exception as exc:
        result.update(status="failed",error_type=type(exc).__name__,error=str(exc))
        write(output/"failure.json",result); raise
    finally:
        write(output/"result.json",result)
    print(json.dumps({k:result[k] for k in ("status","source","original_v02_count","canonical_v02_count","totals")}))
def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument("--source-root",type=Path,required=True); p.add_argument("--manifest",type=Path,required=True)
    p.add_argument("--manifest-sha256",required=True); p.add_argument("--archive",type=Path,required=True)
    p.add_argument("--output",type=Path,required=True)
    run(p.parse_args())
if __name__=="__main__": main()
