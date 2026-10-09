import sys
sys.dont_write_bytecode=True
import pathlib,json
base=pathlib.Path.cwd()
source=(base/"candidate-combined").resolve()
sys.path.insert(0,str(source/"scripts"))
from p8_cold_build import source_identity,selected_inventory,verify_portable_cell,validate_smoke,digest
commit="9f7b16f0758eb79f306cf44605b84550f02de441"
identity,manifest=source_identity(source,commit)
pairs=[[11601940021, 11601038247, 'faa67ef973e03dbb4eb4066125aff30ca98c93d0a3957ebea9c507421b678c93', '86ca807e3699cf79c26d76f64d3534690141e942284ff32615b8c1dbaa70239c', 8777671, 8865485], [11601772363, 11601224614, 'e0b5e8bfe4f21d95f5ca03fdb37eb15481d3661bd45f9afa3d6a62590fed0518', 'fd5644cac08ad63a8981fe57937f069d2b6e8e39b57b337a16a523f88bfc241f', 8613427, 8701054], [11601099403, 11601224188, '63ca3eb19e3b390918ca0fa713d90f124356101b5f0bec44e40ba8b55845a956', '97a8a9695ad10be1e756eb3701c150fa62e1181198609747eba6e61d8f1b09ca', 8977967, 9065321], [11602171596, 11602017005, '297c04798780ccd8be726323d4bee963a4489edc890ada4c2c18e210352a87e2', 'a8db7eb7af32c92de4beec437fa4f42b5671fb8e0571d2f23e1e7c24989f3cb1', 8278646, 8366777], [11601916417, 11601513575, '02bd358039719c01686994c1686b92e783f5f30235a44d211a2ecac6f2b19a73', '19968fbfb2d0ec7c2b5f12785fcb1caa6fbde78a90e8bd918468aa25cc27c2c1', 8728085, 8815414]]
out=[]
for cell_id,raw_id,cellsha,rawsha,cellbytes,rawbytes in pairs:
    dirs=[base/"raw-pr178-9f7b16f"/str(aid) for aid in [cell_id,raw_id]]
    if not all((d/"transport-receipt.json").is_file() for d in dirs):
        out.append({"cell_artifact_id":cell_id,"raw_artifact_id":raw_id,"status":"download_pending"})
        continue
    for d,aid,sha,size in [(dirs[0],cell_id,cellsha,cellbytes),(dirs[1],raw_id,rawsha,rawbytes)]:
        meta=json.loads((d/"github-metadata.json").read_text())
        assert meta["id"]==aid and meta["digest"]=="sha256:"+sha and meta["size_in_bytes"]==size
        assert meta["workflow_run"]["id"]==37896539450 and meta["workflow_run"]["head_sha"]==commit
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
    (base/"review-platform-pr178-9f7b16f"/("cell-"+label+"-audit.json")).write_text(json.dumps(r,indent=2)+"\n")
    out.append(r)
print(json.dumps(out,indent=2))
