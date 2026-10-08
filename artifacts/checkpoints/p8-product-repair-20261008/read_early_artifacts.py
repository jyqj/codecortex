#!/usr/bin/env python3
import argparse,base64,hashlib,json,pathlib,stat,zipfile
p=argparse.ArgumentParser()
p.add_argument("--input",type=pathlib.Path,required=True)
a=p.parse_args()
pins=json.loads(pathlib.Path("artifacts/checkpoints/p8-product-repair-20261008/early-artifacts.json").read_text())
records=[]
for pin in pins:
    meta=json.loads((a.input/(pin["kind"]+".json")).read_bytes())
    assert meta["id"]==pin["id"] and meta["name"]==pin["name"] and not meta["expired"]
    assert meta["size_in_bytes"]==pin["bytes"] and meta["digest"]=="sha256:"+pin["sha256"]
    assert meta["workflow_run"]["id"]==pin["run"] and meta["workflow_run"]["head_sha"]==pin["head"]
    raw=(a.input/(pin["kind"]+".zip")).read_bytes()
    assert len(raw)==pin["bytes"] and hashlib.sha256(raw).hexdigest()==pin["sha256"]
    with zipfile.ZipFile(a.input/(pin["kind"]+".zip")) as z:
        infos=z.infolist()
        assert len(infos)==pin["files"] and len({i.filename for i in infos})==len(infos)
        assert sum(i.file_size for i in infos)<3000000
        for i in sorted(infos,key=lambda v:v.filename):
            pp=pathlib.PurePosixPath(i.filename)
            assert i.filename and not pp.is_absolute() and "\\" not in i.filename
            assert all(x not in ("",".","..") for x in i.filename.split("/"))
            assert not i.is_dir() and not stat.S_ISLNK(i.external_attr>>16)
            b=z.read(i)
            assert len(b)==i.file_size
            target="artifacts/checkpoints/p8-product-repair-20261008/early-runs/"+str(pin["run"])+"/"+pin["kind"]+"/raw/"+i.filename
            encoded=base64.b64encode(b).decode("ascii")
            rec={"path":target,"artifact":pin["id"],"member":i.filename,"bytes":len(b),"sha256":hashlib.sha256(b).hexdigest(),"chunks":(len(encoded)+7999)//8000}
            records.append(rec)
            print("P8_EARLY_FILE "+json.dumps(rec,separators=(",",":")),flush=True)
            for j in range(0,len(encoded),8000):
                print("P8_EARLY_CHUNK "+json.dumps({"path":target,"part":j//8000,"base64":encoded[j:j+8000]},separators=(",",":")),flush=True)
assert len(records)==29 and len({r["path"] for r in records})==29
print("P8_EARLY_MANIFEST "+json.dumps({"schema_version":1,"pins":pins,"files":records,"file_count":len(records),"bytes":sum(r["bytes"] for r in records),"scope":"All exact original early artifacts, including compile failures. No test rerun or acceptance."},separators=(",",":")),flush=True)
