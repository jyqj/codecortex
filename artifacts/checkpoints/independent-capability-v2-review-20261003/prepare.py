from pathlib import Path
import shutil,subprocess,json,hashlib
R=Path(__file__).resolve().parent; S=R/'source'; H=R/'harness'
H.mkdir(exist_ok=True)
# No eval corpus/heldout copying or evaluation.
for n in ['cc-model','cc-db','cc-parsers','cc-index','cc-search','cc-server','cc-semantic']:
 shutil.copytree(S/'crates'/n,H/'crates'/n,dirs_exist_ok=True)
for n in ['Cargo.lock','Cargo.toml']:
 shutil.copy2(S/n,H/n)
p=H/'Cargo.toml';p.write_text(p.read_text().replace('    "crates/cc-eval",\n',''))
# Appended modules only. Original file bytes remain the complete exact prefix.
for crate,file,test in [('cc-db','capability_read.rs','db_checks.rs'),('cc-server','capability_status.rs','server_checks.rs')]:
 original=(S/'crates'/crate/'src'/file).read_bytes()
 (H/'crates'/crate/'src'/file).write_bytes(original+b'\n#[cfg(test)]\n#[path = "../../../'+test.encode()+b'"]\nmod independent_v2;\n')
 shutil.copy2(R/test,H/test)
def git(*args):return subprocess.check_output(['git',*args],cwd=S).decode()
base='6db4d396e5d994388ada1e97c3d28947ffdb9c81'; prod='11af963c33cfa68cc9497e355464c1d6d058adac'; final='29b03a0fec6bfde92aac9c41dce996ebb2d08cc7'
ident=[]
for p in sorted((S/'crates').rglob('*')):
 if not p.is_file() or 'cc-eval' in p.parts:continue
 rel=p.relative_to(S); b=p.read_bytes(); hb=(H/rel).read_bytes()
 assert hb==b or (str(rel) in ['crates/cc-db/src/capability_read.rs','crates/cc-server/src/capability_status.rs'] and hb.startswith(b) and hb[len(b):].startswith(b'\n#[cfg(test)]'))
 ident.append({'path':str(rel),'sha256':hashlib.sha256(b).hexdigest(),'original_bytes':len(b),'harness_sha256':hashlib.sha256(hb).hexdigest()})
original=(S/'crates/cc-server/src/capability_status.rs').read_bytes().split(b'#[cfg(test)]\nmod tests {')[0]
copy=(S/'artifacts/checkpoints/capability-snapshot-optimization-20261003/current-status-under-test.rs').read_bytes()
# Only EOF ASCII whitespace accepted; no source-prefix normalization.
assert copy.rstrip(b'\r\n\t ')==original.rstrip(b'\r\n\t ')
changed=git('diff','--name-only',base,prod,'--','crates').splitlines()
prod_changed=[p for p in changed if '/src/' in p]
assert prod_changed==['crates/cc-db/src/capability_read.rs','crates/cc-db/src/freshness_store.rs','crates/cc-db/src/lib.rs','crates/cc-server/src/capability_status.rs']
assert not git('diff',prod,final,'--','crates')
query_files=['crates/cc-server/src/query_handle.rs','crates/cc-server/src/query_execution.rs','crates/cc-db/src/read_generation.rs','crates/cc-db/src/semantic_coverage.rs','crates/cc-db/src/semantic_publish.rs','crates/cc-db/src/index_db.rs','crates/cc-db/src/index_db_rebuild.rs','crates/cc-server/tests/p7_v11_ready_epoch_independent_review.rs']
q=[]
for p in query_files:
 if not (S/p).exists():continue
 assert subprocess.check_output(['git','show',base+':'+p],cwd=S)==(S/p).read_bytes()
 q.append(p)
E=S/'artifacts/checkpoints/capability-snapshot-optimization-20261003'
ev=[]
for p in sorted(E.rglob('*')):
 if not p.is_file():continue
 b=p.read_bytes(); row={'path':str(p.relative_to(S)),'bytes':len(b),'sha256':hashlib.sha256(b).hexdigest()}
 if p.suffix=='.json':
  obj=json.loads(b);row['json_type']=type(obj).__name__
 ev.append(row)
(R/'source-identity.json').write_text(json.dumps({'base':base,'production':prod,'final':final,'local_initial_head':'ff458bc591b4e7e444af4464d6eef2513cdb335c','AGENTS_at_fixed_tree':False,'source_prefix_EOF_only_equal':True,'production_changed':prod_changed,'query_byte_identical':q,'crate_files':ident,'author_evidence_inventory':ev},indent=2)+'\n')
print('identity verified:',len(ident),'crate files;',len(ev),'author evidence files; query fence unchanged')
