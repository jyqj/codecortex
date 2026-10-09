import base64,hashlib,json,pathlib,subprocess,sys
p=json.load(sys.stdin)
root=pathlib.Path.cwd().resolve()
expected=pathlib.Path("/Users/jin/Desktop/codecortex-rust")
if root!=expected:raise ValueError("wrong project root")
owned=root/p["owned_root"]
if owned.exists() or not owned.parent.is_dir() or owned.parent.resolve()!=owned.parent:
    raise ValueError("owned control directory must be fresh below fixed parent")
names=[r["name"] for r in p["files"]]
if len(names)!=8 or len(set(names))!=8 or any(pathlib.PurePosixPath(n).name!=n for n in names):
    raise ValueError("invalid fixed control list")
owned.mkdir()
receipt={"schema":"p8-original-controls-materialization-v1","plan_blob":p["plan_blob"],"files":[],
         "source_or_ref_mutation":False,"executed_controls":False,"status":"partial"}
try:
    for row in p["files"]:
        argv=["/opt/homebrew/bin/gh","api","repos/jyqj/codecortex/git/blobs/"+row["blob"]]
        q=subprocess.run(argv,stdout=subprocess.PIPE,stderr=subprocess.PIPE,timeout=120)
        if q.returncode:raise ValueError("fixed Git blob transfer failed")
        value=json.loads(q.stdout)
        if value["sha"]!=row["blob"] or value["encoding"]!="base64":raise ValueError("Git blob packet identity")
        body=base64.b64decode(value["content"])
        if len(body)!=row["bytes"] or value["size"]!=row["bytes"] or hashlib.sha256(body).hexdigest()!=row["sha256"]:
            raise ValueError("fixed full control SHA")
        if hashlib.sha1(("blob "+str(len(body))).encode()+bytes([0])+body).hexdigest()!=row["blob"]:
            raise ValueError("fixed Git content identity")
        with (owned/row["name"]).open("xb") as f:f.write(body)
        (owned/row["name"]).chmod(0o644)
        if (owned/row["name"]).read_bytes()!=body:raise ValueError("materialized control differs")
        receipt["files"].append(row)
    receipt["status"]="fixed_controls_materialized_not_executed"
except BaseException as error:
    receipt["error_type"]=type(error).__name__
    raise
finally:
    (owned/"materialization.json").write_text(json.dumps(receipt,sort_keys=True,indent=2)+chr(10))
print(json.dumps({"status":receipt["status"],"file_count":len(receipt["files"]),"bytes":sum(x["bytes"]for x in receipt["files"]),
                  "receipt_sha256":hashlib.sha256((owned/"materialization.json").read_bytes()).hexdigest()}))
