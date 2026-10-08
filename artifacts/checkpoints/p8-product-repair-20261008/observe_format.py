#!/usr/bin/env python3
import base64, hashlib, json, pathlib, subprocess, sys
root = pathlib.Path.cwd()
out = root / "artifacts/checkpoints/p8-product-repair-20261008/format-observation"
out.mkdir(parents=True, exist_ok=True)
paths = json.loads((root / "artifacts/checkpoints/p8-product-repair-20261008/source-paths.json").read_text())
def digest(b):
    return {"bytes":len(b),"sha256":hashlib.sha256(b).hexdigest()}
records=[]
for p in paths:
    b=(root/p).read_bytes()
    d=out/"before"/p
    d.parent.mkdir(parents=True,exist_ok=True)
    d.write_bytes(b)
    records.append({"path":p,"before":digest(b)})
commands=[]
for name,cmd in [("check-before",["cargo","fmt","--all","--","--check"]),("format",["cargo","fmt","--all"]),("check-after",["cargo","fmt","--all","--","--check"])]:
    r=subprocess.run(cmd,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,timeout=120)
    (out/(name+".log")).write_bytes(r.stdout)
    commands.append({"name":name,"argv":cmd,"returncode":r.returncode,**digest(r.stdout)})
    if name!="check-before" and r.returncode!=0:
        raise RuntimeError(name+" failed")
changed=subprocess.check_output(["git","diff","--name-only"]).decode().splitlines()
if set(changed)-set(paths):
    raise RuntimeError("Formatter changed unapproved path: "+str(set(changed)-set(paths)))
diff=subprocess.check_output(["git","diff","--binary","--",*paths])
(out/"format.diff").write_bytes(diff)
for record in records:
    p=record["path"]
    b=(root/p).read_bytes()
    d=out/"after"/p
    d.parent.mkdir(parents=True,exist_ok=True)
    d.write_bytes(b)
    record["after"]=digest(b)
    record["changed"]=record["before"]!=record["after"]
    encoded=base64.b64encode(b).decode("ascii")
    chunks=[encoded[i:i+16000] for i in range(0,len(encoded),16000)]
    for i,chunk in enumerate(chunks):
        print("P8_FORMAT_FILE "+json.dumps({"path":p,**record["after"],"part":i,"parts":len(chunks),"base64":chunk},separators=(",",":")),flush=True)
receipt={"schema_version":1,"purpose":"Actual rustfmt on the unaccepted P8 candidate; not source approval, test acceptance or TODO transition.","git_head":subprocess.check_output(["git","rev-parse","HEAD"]).decode().strip(),"source_before_commit":"3e53704c2878562a1c6e9ef1d019cf156104545d","commands":commands,"changed_paths":changed,"format_diff":digest(diff),"files":records}
(out/"receipt.json").write_text(json.dumps(receipt,ensure_ascii=False,indent=2)+"\n")
print("P8_FORMAT_RECEIPT "+json.dumps(receipt,separators=(",",":")),flush=True)
