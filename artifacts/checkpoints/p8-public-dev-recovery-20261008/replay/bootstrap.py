#!/usr/bin/env python3
import hashlib,json,os,pathlib,subprocess,zipfile
root=pathlib.Path.cwd()
out=pathlib.Path(os.environ["P8_REPLAY_OUT"])
out.mkdir(parents=True,exist_ok=False)
downloads=pathlib.Path(os.environ["P8_REPLAY_DOWNLOADS"])
manifest=json.loads((root/"artifacts/benchmarks/p8-external-recovery-20261008/manifest.json").read_bytes())
expected={"artifact_id":11529447454,"repository":"jyqj/codecortex","bytes":141787552,"sha256":"21e15185fc7e0fe917d1253a283026b20e69266fcd75768cff0f2012632aef68"}
assert manifest["archive"]==expected
meta=json.loads((downloads/"artifact.json").read_bytes())
assert meta["id"]==expected["artifact_id"] and meta["size_in_bytes"]==expected["bytes"]
assert meta["digest"]=="sha256:"+expected["sha256"] and not meta["expired"]
assert meta["workflow_run"]["head_sha"]==manifest["product_source"]=="ffdc6f0f97db78cc25a6c026904e7c2adde05d14"
zip_path=downloads/"artifact.zip"
assert zip_path.stat().st_size==expected["bytes"]
h=hashlib.sha256()
with zip_path.open("rb") as stream:
    for block in iter(lambda:stream.read(1048576),b""):
        h.update(block)
assert h.hexdigest()==expected["sha256"]
validator=out/"validator"
validator.mkdir()
records=[]
with zipfile.ZipFile(zip_path) as z:
    assert len(z.namelist())==len(set(z.namelist()))
    for role,target in (("evaluator","cc-eval"),("evaluator_receipt","build-receipt.json")):
        pin=manifest["tools"][role]
        info=z.getinfo(pin["member"])
        assert info.file_size==pin["bytes"]
        raw=z.read(info)
        assert len(raw)==pin["bytes"] and hashlib.sha256(raw).hexdigest()==pin["sha256"]
        dest=validator/target
        dest.write_bytes(raw)
        if role=="evaluator":
            dest.chmod(0o755)
        records.append({"role":role,"path":target,**pin})
receipt=json.loads((validator/"build-receipt.json").read_bytes())
assert receipt["source_before"]["source_commit"]==manifest["product_source"]
assert receipt["source_before"]==receipt["source_after"]
assert receipt["binary_sha256"]==manifest["tools"]["evaluator"]["sha256"]
assert receipt["build_exit_code"]==0
(validator/"original-artifact-metadata.json").write_bytes((downloads/"artifact.json").read_bytes())
(validator/"provenance.json").write_text(json.dumps({"schema_version":1,"archive":expected,"source_commit":manifest["product_source"],"files":records,"scope":"Previously accepted fixed original evaluator used only for original schema/locks and corpus projection. No retrieval or new candidate build is claimed."},indent=2)+"\n")
commands=json.loads((root/"artifacts/checkpoints/p8-public-dev-recovery-20261008/replay/commands.json").read_bytes())
fetches=[]
for ref in commands["history_objects_required"]:
    assert len(ref)==40 and all(c in "0123456789abcdef" for c in ref)
    have=subprocess.run(["git","cat-file","-e",ref+"^{commit}"],stdout=subprocess.PIPE,stderr=subprocess.PIPE)
    if have.returncode:
        r=subprocess.run(["git","fetch","--no-tags","origin",ref],stdout=subprocess.PIPE,stderr=subprocess.PIPE,timeout=120)
        (out/("fetch-"+ref+".stdout")).write_bytes(r.stdout)
        (out/("fetch-"+ref+".stderr")).write_bytes(r.stderr)
        fetches.append({"commit":ref,"exit_code":r.returncode})
        assert r.returncode==0
    subprocess.run(["git","cat-file","-e",ref+"^{commit}"],check=True)
(out/"history-objects.json").write_text(json.dumps({"required":commands["history_objects_required"],"actual_fetches":fetches,"all_present":True},indent=2)+"\n")
print("P8_PUBLIC_REPLAY_VALIDATOR "+json.dumps({"sha256":manifest["tools"]["evaluator"]["sha256"],"source":manifest["product_source"],"artifacts":expected,"scope":"Schema and lock validation only; no scoring run"}),flush=True)
