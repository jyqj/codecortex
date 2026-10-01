from pathlib import Path
import os,re,json,hashlib,shutil
R=Path.cwd();OUT=R/'artifacts/checkpoints/20261001-paused-github-sync';B=R/'artifacts/benchmarks';copied=[];omitted=[]
SKIP={'target','targets','source','sources','reference','binaries','__pycache__','.git','.codecortex','raw','observations'}
SUFFIX={'.json','.py','.rs','.c','.h','.md','.log','.toml','.lock','.txt','.sh'}
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def copy(p,why):
 assert p.is_file() and not p.is_symlink(),p
 relative=p.relative_to(R);dest=OUT/'evidence'/relative;dest.parent.mkdir(parents=True,exist_ok=True)
 if dest.exists():assert sha(dest)==sha(p);return
 shutil.copyfile(p,dest);copied.append({'original_path':str(relative),'uploaded_path':str(dest.relative_to(R)),'bytes':p.stat().st_size,'sha256':sha(p),'reason':why})
for root in sorted(B.iterdir()):
 if not root.is_dir() or not(root.name.startswith('p5e-') or root.name=='p5d-20260930-resume'):continue
 for cur,ds,fs in os.walk(root):
  for name in list(ds):
   if name in SKIP:omitted.append({'path':str((Path(cur)/name).relative_to(R)),'scope':'directory','status':'local_not_uploaded','reason':'compiled targets/binaries/duplicate source snapshots or bulk observations; authoritative source manifests/receipts/hashes and selected raw retained'});ds.remove(name)
  for f in sorted(fs):
   p=Path(cur)/f
   if p.suffix not in SUFFIX or p.stat().st_size>2500000 or (f in ['observations.json','whole-query-observations.json'] and 'control-binding' not in p.parts) or re.match(r'^\d+\.json$',f):
    if p.is_file():omitted.append({'path':str(p.relative_to(R)),'scope':'file','status':'local_not_uploaded','bytes':p.stat().st_size,'reason':'bulk raw/normalized corpus, archive/binary or non-reviewable export; no acceptance inferred from omission'})
    continue
   copy(p,'complete progress/scripts/inputs/receipts/logs/manifests; rawcontrol binding retained')
# Essential real source/body/packing failure and regression raw observations.
for rel in ['p5e-priority-pressure-20261001/final-raw-v2','p5e-sourcev3-pressure-diagnostic-20261001/raw','p5e-path-policy-packing-20261001/final-pressure-raw','p5e-path-domain-20261001/red-raw','p5e-path-domain-20261001/final-raw','p5e-path-domain-20261001/green-attempt8-raw']:
 folder=B/rel
 if folder.exists():
  for p in sorted(folder.rglob('*')):
   if p.is_file() and p.suffix in ['.json','.log'] and p.stat().st_size<2500000:copy(p,'necessary actual immutable source/body/budget regression raw, including failed cases')
# Actual original fixed 51-query source/gold/config closure (not current relock).
for rel in ['artifacts/benchmarks/p0-g0-20260927/frozen-inputs/p0-smoke/suite.json','artifacts/benchmarks/p0-g0-20260927/frozen-inputs/p0-codecortex-subset/suite.json','crates/cc-eval/benchmarks/manifests/p1b-exact.json','crates/cc-eval/benchmarks/manifests/p1c-intents.json']:
 p=R/rel;s=json.loads(p.read_text());copy(p,'original immutable51 suite')
 q=(p.parent/s['queries']).resolve();copy(q,'original immutable51 gold queries unchanged')
 root=(p.parent/s['source']['root']).resolve()
 for name in s['source']['files']:copy(root/name,'original immutable51 admitted source raw bytes unchanged')
manifest={'status':'curated_complete_engineering_progress_not_full_raw_upload_not_G5','copied':copied,'excluded_local':omitted,'total_uploaded_files':len(copied),'total_uploaded_bytes':sum(x['bytes'] for x in copied),'bulk_original_location':str(B),'source_of_truth':'docs/roadmap/code-index-v2/tasks.json','no_deleted_local_originals':True}
(OUT/'evidence-index.json').write_text(json.dumps(manifest,ensure_ascii=False,indent=2)+'\n');print(json.dumps({'files':len(copied),'bytes':manifest['total_uploaded_bytes'],'excluded_groups_or_files':len(omitted)}))
