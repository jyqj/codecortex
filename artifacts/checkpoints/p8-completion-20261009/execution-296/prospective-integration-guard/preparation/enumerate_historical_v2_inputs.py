#!/usr/bin/env python3
"""Statically enumerate the unchanged v2 verifier's data-dependent read closure."""
import ast, hashlib, json, pathlib, subprocess

RAM=pathlib.Path('/dev/shm/a217aaae3bde')
ROOT=RAM/'codecortex'
OUT=pathlib.Path('/workspace/scratch/a217aaae3bde/prospective-combined-tree-prep')
GIT=RAM/'prospective-combined-tree-prep/git'
def git(*args):return subprocess.check_output(['git','--git-dir='+str(GIT),*args])
def raw(ref,path):return git('show',ref+':'+path)
def sha(b):return hashlib.sha256(b).hexdigest()
guard=ROOT/'scripts/verify_historical_integrations_v2.py'
tree=ast.parse(guard.read_text())
const={}
for n in tree.body:
    if isinstance(n,ast.Assign) and isinstance(n.targets[0],ast.Name):
        try:const[n.targets[0].id]=ast.literal_eval(n.value)
        except (ValueError,TypeError):pass
pairs=set(); current={}; inventory=[]
def pair(ref,path,reason):pairs.add((ref,path,reason))
def local(path,reason):current.setdefault(path,[]).append(reason)
task=const['TASK_PATH']
for path in const['LEGACY_FILES']:
    pair(const['TASK_BASE'],path,'legacy frozen bytes');local(path,'legacy frozen bytes')
manifest_paths=[const['PACKING_REPORT']+'/source-manifest.json',const['E3_REPORT']+'/production-identity.json',const['E3_REPORT']+'/imported-identity.json',const['E3_REPORT']+'/final-tooling-identity.json']
manifest_refs=[const['PACKING_INTEGRATION']]+[const['E3_INTEGRATION']]*3
manifests=[]
for ref,path in zip(manifest_refs,manifest_paths):
    pair(ref,path,'fixed manifest');local(path,'fixed manifest');manifests.append(json.loads(raw(ref,path)))
packing,product,imports,tooling=manifests
for row in packing['crate_inputs']:
    pair(const['PACKING_INTEGRATION'],row['path'],'packing integrated input')
    pair(row['source_commit'],row['path'],'packing original input')
    if row.get('adaptation')=='four_cloned_ref_to_slice_refs':
        local(const['PACKING_REPORT']+'/original-tests/qname_db_independent_review.rs','exact original adapted test')
for row in packing['evidence_files']:
    pair(row['source_commit'],row['path'],'packing original evidence');local(row['path'],'packing original evidence')
for row in packing['unchanged_workflows']:
    for ref in [const['PACKING_INTEGRATION'],const['PACKING_WORKFLOW_SOURCE']]:pair(ref,row['path'],'historical workflow')
pair(const['PACKING_INTEGRATION'],task,'packing preserved states')
for row in product['files']:pair(const['E3_SOURCE'],row['path'],'E3 original input')
for row in imports['files']:
    pair(row['source_commit'],row['path'],'E3 imported original');local(row['path'],'E3 imported original')
pair(const['E3_INTEGRATION'],task,'E3 tooling task snapshot')
pair(const['TASK_BASE'],task,'immutable task definitions')
local(task,'current task definitions and plan normal check')
local(const['GATES_PATH'],'current gate states and absent-path contract')
for path in ['scripts/verify_historical_integrations_v2.py','scripts/code_index_plan.py']:
    local(path,'executed unchanged source')
for path in ['docs/roadmap/code-index-v2/06-VALIDATION.md','docs/roadmap/code-index-v2/05-TODO.md','README.md','docs/roadmap/code-index-v2/README.md','docs/roadmap/code-index-v2/08-HANDOFF.md']:
    local(path,'original code_index_plan.py default validation/read-derived-view')
for ref in [const['PACKING_INTEGRATION'],const['PACKING_SOURCE'],const['E3_SOURCE']]:
    paths=git('ls-tree','-r','--name-only','-z',ref,'--','crates','Cargo.toml','Cargo.lock').split(b'\0')
    inventory.append({'ref':ref,'path_roots':['crates','Cargo.toml','Cargo.lock'],'paths':[x.decode() for x in paths if x]})
prep=json.loads((OUT/'private-base-preparation.json').read_bytes())
prospective=prep['prospective_tree']
files=[]
for path,reasons in sorted(current.items()):
    description=git('ls-tree',prospective,'--',path).decode().strip()
    mode,kind,blob=description.split('\t')[0].split()
    assert kind=='blob'
    actual=(ROOT/path).read_bytes()
    assert hashlib.sha1(b'blob '+str(len(actual)).encode()+b'\0'+actual).hexdigest()==blob
    assert bool((ROOT/path).stat().st_mode&0o111)==(mode=='100755')
    files.append({'path':path,'mode':mode,'git_blob':blob,'bytes':len(actual),'sha256':sha(actual),'reasons':reasons})
inv=json.loads((OUT/'object-and-space-inventory.json').read_bytes())
new_paths={x['path'] for x in inv['additional_official_entries'] if x['type']=='blob'}
intersect=sorted(new_paths & set(current));assert not intersect
forbidden=const['FORBIDDEN']
allpaths=[x['path'] for x in inv['base_files']]+list(new_paths)
assert not any(p==forbidden or p.startswith(forbidden+'/') for p in allpaths)
refs=sorted({p[0] for p in pairs}|{const['PACKING_INTEGRATION'],const['PACKING_SOURCE'],const['PACKING_WORKFLOW_SOURCE'],const['E3_INTEGRATION'],const['E3_SOURCE'],const['TASK_BASE']})
unique=sorted({(ref,path) for ref,path,_ in pairs})
result={'schema':'unchanged-historical-v2-static-read-closure-v1','method':'Static source review plus exact frozen manifest expansion; original verifier and normal-plan CLI are not executed.',
 'guard_source_sha256':sha(guard.read_bytes()),'plan_source_sha256':sha((ROOT/'scripts/code_index_plan.py').read_bytes()),
 'prospective_tree':prospective,'prospective_parents':prep['prospective_parents'],'current_files':files,
 'current_file_count':len(files),'unique_git_blob_reads':[{'ref':r,'path':p} for r,p in unique],
 'unique_git_blob_read_count':len(unique),'git_read_reasons':[{'ref':r,'path':p,'reason':reason} for r,p,reason in sorted(pairs)],
 'git_inventory_reads':inventory,'fixed_provenance_commits':refs,'fixed_provenance_commit_count':len(refs),
 'new_archive_file_count':len(new_paths),'new_archives_intersection_with_current_read_set':intersect,
 'forbidden_path':forbidden,'forbidden_absent_in_complete_prospective_tree_inventory':True,
 'private_checkout_complete_combination':False,'guard_executed':False,'task_statuses_changed':False,
 'limits':['No full combined checkout claim: added archive blobs are not materialized.','Git object closure for the listed fixed reads must be complete before the unchanged verifier can run.','Actual fresh main and actual combined tree must be fixed after PR165 merge before any final local probe.','Full checkout validation belongs to actual normal GitHub CI; this preparation grants no TODO completion.']}
p=OUT/'historical-v2-complete-read-closure.json';p.write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps({'path':str(p),'sha256':sha(p.read_bytes()),'current_files':len(files),'git_reads':len(unique),'refs':len(refs),'intersection':intersect,'forbidden_absent':True}))
