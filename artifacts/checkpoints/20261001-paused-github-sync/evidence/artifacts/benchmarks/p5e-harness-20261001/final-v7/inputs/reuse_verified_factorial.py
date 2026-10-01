#!/usr/bin/env python3
"""Reuse immutable REAL fresh-per-cell compilation, not stale shared artifacts.
Copy proven source/reference/binaries/receipts into a new versioned run closure;
actual9controlwitnesses still mandatory before any measurement.
"""
import argparse,json,pathlib,hashlib,shutil
p=argparse.ArgumentParser();p.add_argument('--compiled',type=pathlib.Path,required=True);p.add_argument('--output',type=pathlib.Path,required=True);a=p.parse_args();assert not a.output.exists();plan=json.load(open(a.compiled/'plan.json'));reference=json.load(open(a.compiled/'reference-source.json'));targets=set();checks=[];sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
for cell in plan['variants']:
 r=json.load(open(a.compiled/cell['build_receipt']));assert r['exit_code']==0;t=r['build_options']['CARGO_TARGET_DIR'];assert t not in targets;targets.add(t);src=a.compiled/cell['source_root'];actual={p.relative_to(src).as_posix():sha(p) for p in src.rglob('*') if p.is_file()};assert actual==r['source_files'] and set(actual)==set(reference);expected={k:(a.compiled/'reference'/k).read_bytes() for k in reference}
 for c in plan['controls']:
  text=expected[c['path']].decode();assert text.count(c['on_text'])==1
  if c['id'] not in cell['enabled']:expected[c['path']]=text.replace(c['on_text'],c['off_text'],1).encode()
 assert all(actual[k]==hashlib.sha256(v).hexdigest() for k,v in expected.items());assert sha(a.compiled/cell['binary'])==r['binary_sha256'];opts=r['build_options'];assert opts['binding'].startswith('fresh unique per-celltarget') and opts['profile']=='release' and opts['features']=='default' and opts['jobs']==2;assert all(flag in opts['command'] for flag in ['+stable','--offline','--locked','--release']);checks.append({'cell':cell['id'],'target':t,'original_build_receipt':str(a.compiled/cell['build_receipt']),'original_build_receipt_sha256':sha(a.compiled/cell['build_receipt']),'binary_sha256':r['binary_sha256'],'source_sha256':actual,'options':opts})
a.output.mkdir()
for folder in ['reference','sources','binaries','receipts']:shutil.copytree(a.compiled/folder,a.output/folder)
for filename in ['plan.json','reference-source.json','original-51-input-lock.json','preparation.json']:shutil.copy2(a.compiled/filename,a.output/filename)
(a.output/'REUSE-COMPILE-PROVENANCE.json').write_text(json.dumps({'status':'reuse_only8actualfreshindependenttarget_source_build_binary_closures_verified_no_newcompile','checks':checks,'runtime_controls':'notyetmandatorynextstage9actualwitness/Full111equivalence','oldfailure':'oldstage1emptyNoMatch and profile untouched;no measurement occurred'},indent=2)+'\n')
