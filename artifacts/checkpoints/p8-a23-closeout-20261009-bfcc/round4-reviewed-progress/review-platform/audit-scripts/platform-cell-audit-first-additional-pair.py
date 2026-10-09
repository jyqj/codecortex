import sys
sys.dont_write_bytecode=True
import pathlib,json
base=pathlib.Path.cwd()
source=(base/"source").resolve()
sys.path.insert(0,str(source/"scripts"))
from p8_cold_build import source_identity,selected_inventory,verify_portable_cell,validate_smoke,digest
commit="a23bb72d3c954f385b99fe81ce9189885c208557"
identity,manifest=source_identity(source,commit)
pairs=[
(11591159319,11591239060,"b3ea19fa9254adaa4128616eaad557b3e55a36de3ab1563216e96ebd63623d44","978b8a942e755de19940ca4261bd9890d801df97859c7b87b4afccc6c93c9590",8351887,8439233),
(11592465349,11591809454,"83ffc0a9fbb2c8d4acac780ee427022e5999f2bf89734b539342e03ba602e174","e622e79505c4dcd4eece153daa01ca1e884a1e3570b29d32958ff2a24d6b91c1",8605542,8692936)]
out=[]
for cell_id,raw_id,cellsha,rawsha,cellbytes,rawbytes in pairs:
    dirs=[base/"raw"/str(aid) for aid in [cell_id,raw_id]]
    if not all((d/"transport-receipt.json").is_file() for d in dirs):
        out.append({"cell_artifact_id":cell_id,"raw_artifact_id":raw_id,"status":"download_pending"})
        continue
    for d,aid,sha,size in [(dirs[0],cell_id,cellsha,cellbytes),(dirs[1],raw_id,rawsha,rawbytes)]:
        meta=json.loads((d/"github-metadata.json").read_text())
        assert meta["id"]==aid and meta["digest"]=="sha256:"+sha and meta["size_in_bytes"]==size
        assert meta["workflow_run"]["id"]==37871838952 and meta["workflow_run"]["head_sha"]==commit
        assert (d/"original.zip").stat().st_size==size and digest(d/"original.zip")==sha
    home=dirs[0]/"extracted"
    bundle=json.loads((home/"bundle.json").read_text())
    record=json.loads((home/"receipt.json").read_text())
    cell=record["cell"]
    matrix=json.loads((home/"matrix.json").read_text())
    actual={p.relative_to(home).as_posix() for p in home.rglob("*") if p.is_file() and p!=home/"bundle.json"}
    assert actual==set(bundle["files"]) and not any(p.is_symlink() for p in home.rglob("*"))
    assert all(digest(home/n)==h for n,h in bundle["files"].items())
    assert bundle["schema_version"]==2 and bundle["status"]=="passed" and bundle["live_strict_receipt_and_stdio_replay"] is True
    assert record["cell"]==bundle["cell"]
    selected=selected_inventory(matrix,cell)
    assert digest(home/"receipt.json")==selected["receipt_sha256"]==bundle["receipt_sha256"]
    assert pathlib.PurePosixPath(selected["receipt"]).name=="receipt.json"
    assert str(pathlib.PurePosixPath(selected["receipt"]).parent/"target")==record["target_directory"]
    assert matrix["source"]==record["source_before"]
    assert dict(record["source_before"],source_root=identity["source_root"])==identity
    assert json.loads((home/"source-inputs.json").read_text())==manifest
    assert digest(home/"source-inputs.json")==identity["manifest_sha256"]
    assert digest(home/"codecortex")==bundle["binary_sha256"]==record["binary_sha256"]
    verify_portable_cell(record,home,source,home/"codecortex")
    validate_smoke(home,record)
    label="-".join(cell[k] for k in ["platform","toolchain","package"])
    raw=dirs[1]/"extracted"/"cold"/label
    overlap=0
    for p in raw.rglob("*"):
        if p.is_file():
            rel=p.relative_to(raw)
            assert (home/rel).is_file() and digest(p)==digest(home/rel),str(rel)
            overlap+=1
    assert overlap==17
    r={"schema_version":1,"status":"accepted_original_portable_cell_and_stdio_replay","source_commit":commit,"source_tree":identity["source_tree"],"source_input_count":identity["input_count"],"source_manifest_sha256":identity["manifest_sha256"],"cell":cell,"rustc_release":record["toolchain"]["rustc_release"],"host":record["toolchain"]["host"],"binary_sha256":record["binary_sha256"],"binary_bytes":record["binary_bytes"],"cell_artifact_id":cell_id,"raw_artifact_id":raw_id,"cell_zip_sha256":cellsha,"raw_zip_sha256":rawsha,"receipt_sha256":digest(home/"receipt.json"),"bundle_sha256":digest(home/"bundle.json"),"bundle_files":len(actual),"raw_overlap_identical":overlap,"wall_seconds":record["wall_seconds"],"todo_status_change":False}
    (base/"review-platform"/("cell-"+label+"-audit.json")).write_text(json.dumps(r,indent=2)+"\n")
    out.append(r)
print(json.dumps(out,indent=2))
