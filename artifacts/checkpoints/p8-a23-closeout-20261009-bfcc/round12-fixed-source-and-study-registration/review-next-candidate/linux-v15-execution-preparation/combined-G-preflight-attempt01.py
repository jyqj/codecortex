from pathlib import Path
import json,hashlib,subprocess,os,datetime
base=Path('.').resolve();root=base/'candidate-combined'
expected={'head':'9f7b16f0758eb79f306cf44605b84550f02de441','tree':'9003d1383b4dbf148ac3aca30d00b5e8c06aaf48','review':'6f11ae6904fd8d08fec4b4abd83f510cedcba6c8','product':'9d13cb30dd618bc5d106186711c1d98e01b47995','guard_sha256':'ca57e744c5e6b3f2989bd097b37e68c360d89025b5833c83588eeed8c89157ef','registry_sha256':'852ea42fc66c4375b9566fc5530780774bcd78bd03f665ef207b5fc21794b46c','review_sha256':'b7cb663259960d263f8e21d1de7762b33cd3ea5c03445251408a0615588c16c6','review_path':'artifacts/checkpoints/p8-combined-engineering-bfcc-20261009/independent-source-review.json'}
env=dict(os.environ,GIT_NO_LAZY_FETCH='1',GIT_TERMINAL_PROMPT='0',GIT_OPTIONAL_LOCKS='0')
def git(*args):return subprocess.check_output(['git',*args],cwd=root,env=env)
def sha(raw):return hashlib.sha256(raw).hexdigest()
def tree(ref):
 rows={}
 for line in git('ls-tree','-r','-z',ref).split(b'\0'):
  if line:
   info,p=line.decode().split('\t',1);mode,kind,oid=info.split();rows[p]={'mode':mode,'kind':kind,'blob':oid}
 return rows
assert git('rev-parse','HEAD').decode().strip()==expected['head']
assert git('rev-parse','HEAD^{tree}').decode().strip()==expected['tree']
assert git('show','-s','--format=%P','HEAD').decode().strip()==expected['review']
assert git('show','-s','--format=%P',expected['review']).decode().strip()==expected['product']
assert git('status','--porcelain=v1','--untracked-files=all')==b''
current=tree(expected['head']);product=tree(expected['product'])
for p,key,blob in [('scripts/verify_reviewed_source_v15.py','guard_sha256','1d0ef393a39839bb3d7c23f16a5202bf18bfb895'),('scripts/reviewed-source-registry-v15.json','registry_sha256','591c46132065cd1f1055169ff12e70df6bd47196'),(expected['review_path'],'review_sha256',None)]:
 assert sha((root/p).read_bytes())==expected[key]
 if blob:assert current[p]['blob']==blob
reg=json.loads((root/'scripts/reviewed-source-registry-v15.json').read_text())
scope={p for p in product if p.startswith('crates/') or p in ['Cargo.lock','Cargo.toml']}
assert set(reg['complete_inputs'])==scope
assert len(reg['validation_inputs'])==138
mapping={}
for p,digest in {**reg['complete_inputs'],**reg['validation_inputs']}.items():
 f=root/p;assert f.is_file() and not f.is_symlink()
 raw=f.read_bytes();assert sha(raw)==digest
 assert hashlib.sha1(b'blob '+str(len(raw)).encode()+b'\0'+raw).hexdigest()==current[p]['blob']
 assert current[p]==product[p]
 mode='100755' if f.stat().st_mode&0o111 else '100644';assert mode==current[p]['mode']
 mapping[p]={'bytes':len(raw),'sha256':digest,**current[p]}
d=base/'review-next-candidate/linux-v15-execution-preparation'
for p,want in [(d/'observer-template/observer.py','11fee8917b294bfa7b47e0b31c8b3d1fea8d935fdb5296f319f284d241620c45'),(d/'observer-template/launch.py','54f855b8e090e48f18a15a9ef27203a4a756d5d9f1a3b9e0057e53234025caa5')]:assert sha(p.read_bytes())==want
execution=base/'review-ci/combined-linux-source-admission-attempt01'
assert not execution.exists() and root not in execution.parents
common=Path('/Users/jin/Desktop/codecortex-rust/.git')
assert execution!=common and common not in execution.parents
assert Path(git('rev-parse','--git-common-dir').decode().strip()).resolve()==common
binding={**expected,'source_dir':str(root),'execution_dir':str(execution),'container_name':'p8-v15-combined-bfcc-9f7b16f0758e-attempt01','additional_inputs':{}}
for p in ['docs/roadmap/code-index-v2/tasks.json','docs/roadmap/code-index-v2/05-TODO.md']:
 binding['additional_inputs'][p]=sha((root/p).read_bytes())
p=d/'combined-G-binding.json';assert not p.exists();p.write_text(json.dumps(binding,indent=2,sort_keys=True)+'\n')
inputs=d/'combined-G-preflight-inputs.json';assert not inputs.exists();inputs.write_text(json.dumps(mapping,indent=2,sort_keys=True)+'\n')
report={'observed_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'status':'preflight_passed_not_cli_execution','actual_identity':expected,'actual_source_dir':str(root),'source_clean':True,'fresh_output_absent':True,'execution_outside_source_and_shared_git':True,'complete_product_count':len(reg['complete_inputs']),'validation_count':len(reg['validation_inputs']),'every_manifest_byte_and_git_mode_blob_verified':True,'template_bytes_unchanged':True,'binding':{'path':str(p.relative_to(base)),'bytes':len(p.read_bytes()),'sha256':sha(p.read_bytes())},'inputs':{'path':str(inputs.relative_to(base)),'bytes':len(inputs.read_bytes()),'sha256':sha(inputs.read_bytes())},'no_container_started_yet':True}
out=d/'combined-G-binding-preflight.json';assert not out.exists();out.write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps({**report,'report_sha256':sha(out.read_bytes())}))
