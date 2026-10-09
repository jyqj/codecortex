from pathlib import Path
import hashlib,json,zipfile,datetime
root=Path.cwd();out=root/'native-public-dependency-fix-evidence';files=sorted(p for p in out.iterdir() if p.is_file());rows=[]
for p in files:
 b=p.read_bytes();rows.append({'path':p.name,'bytes':len(b),'sha256':hashlib.sha256(b).hexdigest()})
manifest={'schema':1,'source_G':'3ffcefc3b28ee1a4ed80caecebd7208a45c3e302','created_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'scope':'native actual Rust red/green, first control attempt, and v2 control attempt with original failures retained; no performance claim','source_domain_counts':{'before_and_v1':1242,'v2':1244,'difference':'two original #[path] artifact compile inputs restored for workspace format'},'files':rows}
p=out/'native-evidence-selection.json';p.write_text(json.dumps(manifest,indent=2)+'\n');files.append(p)
zpath=root/'native-public-dependency-fix-red-green-and-controls.zip';assert not zpath.exists()
with zipfile.ZipFile(zpath,'x',compression=zipfile.ZIP_DEFLATED,compresslevel=9) as z:
 for p in files:z.write(p,p.name)
with zipfile.ZipFile(zpath) as z:
 assert z.testzip() is None and len(z.infolist())==len(files)
 for p in files:assert z.read(p.name)==p.read_bytes()
b=zpath.read_bytes();print(json.dumps({'path':str(zpath),'bytes':len(b),'sha256':hashlib.sha256(b).hexdigest(),'members':len(files),'uncompressed_bytes':sum(p.stat().st_size for p in files)}))
