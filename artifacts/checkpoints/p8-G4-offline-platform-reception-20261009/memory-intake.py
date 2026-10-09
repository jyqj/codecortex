import hashlib,io,json,pathlib,stat,subprocess,sys,zipfile
p=json.loads(sys.argv[1]);root=pathlib.Path("/workspace/scratch/2eaa00d0f93a/p8-G4-exact-source")
sys.path.insert(0,str(root/"scripts"));import p8_cold_build as cold
source,manifest=cold.source_identity(root,p["source"]);observer=cold.observer_snapshot(root)
assert len(manifest)==1087 and len(observer["inputs"])==4
collector=p["collector"];assert collector["counts"]==dict(passed=8,failed=0,not_run=0) and collector["status"]=="passed"
src=dict(collector["source"],source_root=str(root));assert src==source
rows=[];seen=set()
for spec in p["specs"]:
 if "url" in spec:
  try:r=subprocess.run(["curl","--fail","--location","--silent","--show-error","--retry","0","--max-time","180",spec["url"]],stdout=subprocess.PIPE,stderr=subprocess.PIPE,timeout=190)
  except subprocess.TimeoutExpired:raise RuntimeError("registered artifact transfer timeout") from None
  if r.returncode:raise RuntimeError("registered artifact curl exit "+str(r.returncode))
  raw=r.stdout
 else:raw=pathlib.Path(spec["path"]).read_bytes()
 assert len(raw)==spec["bytes"] and hashlib.sha256(raw).hexdigest()==spec["sha256"]
 z=zipfile.ZipFile(io.BytesIO(raw));infos=z.infolist();assert len({i.filename for i in infos})==len(infos)
 members={};data={}
 for i in infos:
  q=pathlib.PurePosixPath(i.filename)
  assert not i.is_dir() and not q.is_absolute() and ".." not in q.parts and "\\" not in i.filename and q.as_posix()==i.filename
  assert stat.S_IFMT(i.external_attr>>16) in (0,stat.S_IFREG) and not i.flag_bits&1
  digest=hashlib.sha256();size=0;small=[]
  with z.open(i) as f:
   while b:=f.read(1024*1024):
    size+=len(b);digest.update(b)
    if i.filename!="codecortex":small.append(b)
  assert size==i.file_size
  members[i.filename]={"bytes":size,"sha256":digest.hexdigest(),"crc32":format(i.CRC,"08x")}
  if i.filename!="codecortex":data[i.filename]=b"".join(small)
 bundle=json.loads(data["bundle.json"]);record=json.loads(data["receipt.json"])
 assert bundle["schema_version"]==2 and bundle["status"]=="passed" and record["status"]=="passed"
 assert set(bundle["files"])==set(members)-{"bundle.json"}
 assert all(members[n]["sha256"]==h for n,h in bundle["files"].items())
 assert members["receipt.json"]["sha256"]==bundle["receipt_sha256"]
 assert members["codecortex"]["sha256"]==bundle["binary_sha256"]==record["binary_sha256"]==record["copy_source"]["sha256"]
 assert members["codecortex"]["bytes"]==record["binary_bytes"]==record["copy_source"]["bytes"]
 assert record["source_before"]==record["source_after"]
 assert dict(record["source_before"],source_root=str(root))==source
 assert json.loads(data["source-inputs.json"])==manifest and members["source-inputs.json"]["sha256"]==source["manifest_sha256"]
 before=record["observer_before"];assert before==record["observer_after"]
 assert dict(before,source_root=str(root))==observer
 assert {n.removeprefix("observer-source/") for n in members if n.startswith("observer-source/")}==set(observer["inputs"])
 assert all(members["observer-source/"+n]["sha256"]==v["sha256"] and members["observer-source/"+n]["bytes"]==v["bytes"] for n,v in observer["inputs"].items())
 cell=record["cell"];key=tuple(cell[x] for x in ("platform","toolchain","package"));assert key not in seen;seen.add(key);assert cell==bundle["cell"]
 assert record["profile"]=="release" and record["target_initially_absent"] is True and record["build_exit_code"]==0 and record["stop_reason"] is None
 tools=record["toolchain"];assert tools==record["toolchain_after"]
 assert cold.parse_rustc_verbose(tools["rustc"]["version_verbose"])==(tools["rustc_release"],tools["host"])
 expected={"cell":cell,"status":"passed","rustc_release":tools["rustc_release"],"bundle_sha256":members["bundle.json"]["sha256"],"binary_sha256":record["binary_sha256"],"wall_seconds":record["wall_seconds"]}
 actual=[r for r in collector["cells"] if r["cell"]==cell];assert actual==[expected]
 rows.append({"artifact":{k:v for k,v in spec.items() if k!="url"},"archive_members":len(members),"expanded_bytes":sum(v["bytes"] for v in members.values()),"members":members,"collector_cell_exact_match":expected,"scope":"Full original ZIP CRC/SHA, self-seal, binary-copy, exact source/observer binding and collector-row match only; original path-dependent portable/stdout predicates await unchanged CLI replay."})
 del raw,z,infos,data,members
assert seen=={(a,b,c) for a in cold.PLATFORMS for b in cold.TOOLCHAINS for c in cold.PACKAGES}
after,after_manifest=cold.source_identity(root,p["source"]);assert after==source and after_manifest==manifest and cold.observer_snapshot(root)==observer
out={"schema":"p8-G4-platform-memory-intake-v1","status":"all_eight_archives_transport_and_binding_verified_original_collector_replay_pending","source":source,"observer":observer,"validator_sha256":hashlib.sha256((root/"scripts/p8_cold_build.py").read_bytes()).hexdigest(),"original_collector":collector,"cells":rows,"ZIP_members":sum(r["archive_members"] for r in rows),"ZIP_bytes":sum(r["artifact"]["bytes"] for r in rows),"expanded_bytes":sum(r["expanded_bytes"] for r in rows),"source_before_after_equal":True,"local_writes":False,"Cargo_or_product_execution":False,"fresh_downloaded_zip_retention":"Original GitHub Actions artifacts remain authoritative; anonymous RAM bytes were not persisted locally. Existing four macOS ZIPs were read without modification.","TODO_closed":0,"TODO_remaining":29}
print(json.dumps(out))

