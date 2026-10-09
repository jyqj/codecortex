#!/usr/bin/env python3
"""Combine frozen public selections, retaining manifests and duplicate aliases."""
import hashlib,json,pathlib,stat,datetime
ROOT=pathlib.Path('/workspace/scratch/a217aaae3bde')
OUT=ROOT/'original-C3ff-platform-gates/round32-public-archive'
selections=[
 ('pr-audit','original-C3ff-platform-gates/round32-public-archive/new-public-files-manifest.json'),
 ('pr-audit-CI','original-C3ff-platform-gates/round32-public-archive/post-selection-public-files-manifest.json'),
 ('runtime','pr137-qname-ci-triage/round32-public-files-manifest.json'),
 ('scale','scale-pr180-C-review/round32-public-archive/public-archive-manifest.json'),
]
def fingerprint(p):
 b=p.read_bytes();return {'bytes':len(b),'sha256':hashlib.sha256(b).hexdigest(),'git_blob':hashlib.sha1(b'blob '+str(len(b)).encode()+b'\0'+b).hexdigest(),'mode':'100755' if p.stat().st_mode&0o111 else '100644','source_mode':oct(stat.S_IMODE(p.stat().st_mode))}
files=[];seen={};aliases=[];manifests=[]
for label,source in selections:
 p=ROOT/source;x=json.loads(p.read_text());manifests.append(dict(source=source,**fingerprint(p)))
 for declared in x['files']:
  src=pathlib.Path(declared['source']);src=src.relative_to(ROOT) if src.is_absolute() else src
  actual=fingerprint(ROOT/src)
  for k in ['bytes','sha256','git_blob']:assert declared[k]==actual[k],(str(src),k)
  mode=declared.get('git_mode',declared.get('mode'))
  if mode in ['0644','0755']:mode='100755' if mode=='0755' else '100644'
  assert mode==actual['mode'],str(src)
  key=str(src);target=label+'/'+declared['target']
  if key in seen:
   assert all(seen[key][k]==actual[k] for k in ['bytes','sha256','git_blob','mode'])
   aliases.append({'source':key,'manifest_requested_target':target,'single_member_target':seen[key]['target'],'reason':'Same immutable original source selected by two peers; retain once and preserve both source manifests.'})
  else:
   row=dict(source=key,target=target,**actual);seen[key]=row;files.append(row)
 for_manifest=dict(source=source,target='manifests/'+label+'.json',**fingerprint(p));files.append(for_manifest)
prior=set()
for n in [28,29,30,31]:
 p=ROOT/f'p8-db-lock-observation-integration/round{n}-public-bundle-inputs.json'
 if p.exists():prior.update(z['source'] for z in json.loads(p.read_text())['files'])
assert not (prior & {r['source'] for r in files})
assert len({r['target'] for r in files})==len(files)
cfg={'schema':'round32-public-bundle-inputs-v1','prepared_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'scope':'Frozen original public selections after R31; final round cutoff and raw-custody publication are bound separately in final closeout/navigation. No pending publication is represented as completed.','cutoff_status':'awaiting_root_final_cutoff','raw_custody_status':'awaiting_root_actual_commit','source_C':'3ffcefc3b28ee1a4ed80caecebd7208a45c3e302','diagnostic_G':'4d18dcdb34b5d7a277566b57801cc882d8b1eb61','current_PR_G4':'832f79b8702c6cdfb567b7cf949508cd4589f286','task_status':{'newly_completed':0,'done':163,'remaining':29},'manifests':manifests,'duplicate_selection_aliases':aliases,'prior_round_source_intersection':[],'files':files,'excluded':['All original ZIPs, executables and raw workload data (separate custody branch)','All private transport captures and signed URLs','R28-R31 originals already selected','M5 ongoing source integration','Final benchmark navigation until actual raw-custody commit binding']}
p=OUT/'round32-public-bundle-inputs.json';assert not p.exists();p.write_text(json.dumps(cfg,indent=2,sort_keys=True)+'\n');print(json.dumps({'path':str(p),'files':len(files),'aliases':len(aliases),'payload_bytes':sum(x['bytes'] for x in files),'sha256':fingerprint(p)['sha256']}))
