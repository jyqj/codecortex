from pathlib import Path,PurePosixPath
import json,hashlib,zipfile,stat,re,datetime
ROOT=Path('/workspace/scratch/bfccb8494ba0/root-round14/native-P-transfer')
OUT=Path('/dev/shm/pr-triage-round14-native-P-review')
EX=ROOT/'extracted'
P='c92eb5ac7ece70d1f62271d7e2dacac513c285b5'
TREE='9ae248f8e0c42a8a7389e26e3f44710dfb029640'
PARENTS=['4652cad11dde4b41126544a38fddf25eb2fb7474','55aa2bcf355441585bcf980e1d6f4fab8eebe59d']
def sha(b):return hashlib.sha256(b).hexdigest()
def git(b,t='blob'):return hashlib.sha1(t.encode()+b' '+str(len(b)).encode()+b'\0'+b).hexdigest()
def meta(p):
 b=p.read_bytes();return {'path':str(p),'bytes':len(b),'sha256':sha(b),'git_blob':git(b)}
def read(n):return json.loads((EX/n).read_bytes())
def checkref(r):
 b=(EX/r['path']).read_bytes();assert len(b)==r['bytes'] and sha(b)==r['sha256'],r['path']
 return b
zpath=ROOT/'native-public-api-merge-P-c92e-controls.zip';zb=zpath.read_bytes()
assert (len(zb),sha(zb),git(zb))==(1336504,'3addc07bdf6c58cd57e5545b8a3942c3a86931c8037c0a1ef2db23e2f034c983','4d36d1836ccb958f6b29ce11cdab096e96690f36')
manifest=read('archive-manifest.json');assert manifest['source_P']==P and len(manifest['files'])==51
files={x['path']:x for x in manifest['files']};assert len(files)==51
members=[];seen=set()
with zipfile.ZipFile(zpath) as z:
 for zi in z.infolist():
  n=zi.orig_filename;pp=PurePosixPath(n)
  assert n==zi.filename and '\0' not in n and '\\' not in n and not pp.is_absolute() and '..' not in pp.parts and str(pp)==n and n not in seen and not zi.is_dir() and not stat.S_ISLNK(zi.external_attr>>16),n
  seen.add(n);b=z.read(zi);assert b==(EX/n).read_bytes(),n
  members.append({'path':n,'bytes':len(b),'sha256':sha(b),'crc32':f'{zi.CRC:08x}'})
  if n!='archive-manifest.json':assert len(b)==files[n]['bytes'] and sha(b)==files[n]['sha256'],n
assert seen==set(files)|{'archive-manifest.json'} and len(seen)==52
before=read('before-domain-receipt.json');assert before['source_P']==P and before['tree']==TREE and before['ordered_parents']==PARENTS
entries={r['path']:r for r in before['entries']};assert len(entries)==len(before['entries'])==1245
for n,e in entries.items():
 assert set(e)=={'path','bytes','sha256','git_blob','mode'} and e['mode'] in ['100644','100755'] and type(e['bytes']) is int and e['bytes']>=0
 assert re.fullmatch('[0-9a-f]{64}',e['sha256']) and re.fullmatch('[0-9a-f]{40}',e['git_blob'])
rows={}
for row in (EX/'official-domain-ls-tree.stdout').read_bytes().split(b'\0'):
 if not row:continue
 head,path=row.split(b'\t',1);mode,kind,blob=head.decode().split();name=path.decode();assert kind=='blob' and name not in rows
 rows[name]={'mode':mode,'git_blob':blob}
assert len(rows)==1245 and set(rows)==set(entries)
assert all(all(entries[n][k]==v for k,v in r.items()) for n,r in rows.items())
commit=(EX/'actual-P-object.stdout').read_bytes();assert git(commit,'commit')==P
headers=commit.decode().split('\n\n',1)[0].splitlines();assert headers[:3]==['tree '+TREE]+['parent '+p for p in PARENTS]
prep=read('preparation-receipts.json');assert len(prep)==6
for r in prep:
 assert type(r['exit_code']) is int and r['exit_code']==0
 checkref(r['stdout']);checkref(r['stderr'])
assert (EX/'parent-head-before.stdout').read_bytes()==(EX/'parent-head-after.stdout').read_bytes()
clone=read('target-clone-receipt.json');assert type(clone['exit_code']) is int and clone['exit_code']==0 and clone['command'][:2]==['/bin/cp','-cR'] and clone['command'][2]!=clone['command'][3]
commands=[('resolution-store',['test','--locked','-p','cc-db','--test','p2b_resolution_store'],7,7),('snapshot-binder-dependency-unit',['test','--locked','-p','cc-db','--lib','index_db_snapshot_insert::'],14,14),('snapshot-leaf-owner-control',['test','--locked','-p','cc-index','--test','snapshot_leaf_batching'],6,6),('workspace-format',['fmt','--all','--','--check'],0,0),('cc-db-strict-clippy',['clippy','--locked','-p','cc-db','--all-targets','--','-D','warnings'],0,0),('integrated-installer-cli',['test','--locked','-p','cc-server','--test','installer_cli'],1,2)]
progress=read('controls-progress.json');assert len(progress)==len(commands)==6
results=[];maps=0
for index,(label,args,minimum,expected_passed) in enumerate(commands):
 r=read(label+'-receipt.json');assert r==progress[index]
 assert r['label']==label and r['source_P']==P and r['tree']==TREE and type(r['exit_code']) is int and r['exit_code']==0 and r['minimum_tests']==minimum
 assert r['command']==['/Users/jin/.cargo/bin/cargo','+1.95.0']+args
 assert r['cwd']=='/Users/jin/Desktop/codecortex-rust/artifacts/benchmarks/p8-bfccb8494ba0/candidate-public-api-merge-P-c92e'
 assert r['environment']['CARGO_TARGET_DIR']==clone['command'][3] and r['environment']['SDKROOT']=='/Library/Developer/CommandLineTools/SDKs/MacOSX15.4.sdk'
 assert type(r['pid']) is int and r['pid']>0 and r['elapsed_seconds']>0 and datetime.datetime.fromisoformat(r['ended_at'])>datetime.datetime.fromisoformat(r['started_at'])
 for phase in ['before','after']:
  d=json.loads(checkref(r[phase]));assert d['label']==label and d['phase']==phase and d['source_P']==P and d['tree']==TREE and r[phase]['entries']==1245
  de={x['path']:x for x in d['entries']};assert len(d['entries'])==len(de)==1245 and de==entries
  maps+=1
 stdout=checkref(r['stdout']).decode();stderr=checkref(r['stderr']).decode()
 parsed=[dict(zip(['passed','failed','ignored','measured','filtered_out'],map(int,t))) for t in re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out;',stdout)]
 passed=sum(t['passed'] for t in parsed);assert passed==expected_passed and all(t['failed']==0 for t in parsed)
 oknames=re.findall(r'^test (.+) \.\.\. ok$',stdout,re.M);assert len(oknames)==passed and len(set(oknames))==passed
 assert r['test_summaries']==[{'passed':x['passed'],'failed':x['failed']} for x in parsed] and r['nonzero_test_check_passed'] is True
 if minimum:assert parsed and passed>=minimum
 else:assert not parsed
 results.append({'label':label,'command':r['command'],'exit_code':r['exit_code'],'started_at':r['started_at'],'ended_at':r['ended_at'],'minimum_executed':minimum,'summaries_from_original_stdout':parsed,'passed_test_names':oknames,'stdout':r['stdout'],'stderr':r['stderr'],'before_map':r['before'],'after_map':r['after']})
assert sum(sum(t['passed'] for t in r['summaries_from_original_stdout']) for r in results)==29
ftsline=next(x for x in (EX/'snapshot-binder-dependency-unit.stderr').read_text().splitlines() if x.startswith('P8_FTS_MAX_ROWID_CONTROL='));fts=json.loads(ftsline.split('=',1)[1]);assert fts['timing_measurement'] is False
assert {(r['table'],r['rows']) for r in fts['observations']}=={(t,n) for t in ['files','literal_index'] for n in [0,1,257]}
assert all(r['max_fullscan_steps']==0 and r['maximum']==r['rows'] and any(x[1]=='Last' for x in r['explain']) for r in fts['observations'])
assert all(r['sum_fullscan_steps']==256 for r in fts['observations'] if r['rows']==257)
report={'schema_version':1,'created_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'reviewer':'/root/pr_triage','review_scope':'Independent read-only review of retained native execution evidence; no source edit, native replay, workload, measurement, or remote mutation.','decision':'accepted_scoped_native_engineering_evidence','source_P':P,'tree':TREE,'ordered_parents':PARENTS,'original_zip':meta(zpath),'archive':{'members':52,'manifest_files':51,'crc_read_all':True,'manifest_all_bytes_and_sha256_match':True,'extracted_all_bytes_equal_zip':True,'safe_unique_regular_paths':True,'manifest':meta(EX/'archive-manifest.json')},'source_binding':{'actual_commit_object_rehashed_to_P':True,'original_ls_tree_rows':1245,'baseline_entries':1245,'each_recorded_mode_and_git_blob_matches_original_P_ls_tree':True,'before_after_maps':maps,'entries_per_map':1245,'all_maps_equal_original_baseline_in_every_path_bytes_sha256_git_blob_mode':True,'native_controller_reads_each_source_file_before_after':True,'selected_scope_paths':before['scope_paths'],'scope':'Selected sparse checkout; no whole-repository execution assertion. Independent primary review compares retained maps and original Git output, not a fresh re-read of unavailable native source files.','baseline':meta(EX/'before-domain-receipt.json'),'original_ls_tree':meta(EX/'official-domain-ls-tree.stdout'),'parent_project_HEAD_unchanged':(EX/'parent-head-before.stdout').read_text().strip()},'preparation':{'six_git_commands_exit_codes':[r['exit_code'] for r in prep],'all_original_stdio_hashes_match':True,'clone':clone},'native_engineering':{'commands':results,'command_exit_codes':[r['exit_code'] for r in progress],'test_total_passed':29,'test_total_failed':0,'test_total_ignored':0,'test_total_filtered_out':193,'full_workspace_tests_claimed':False,'fmt_scope':'cargo +1.95.0 fmt --all -- --check','clippy_scope':'cargo +1.95.0 clippy --locked -p cc-db --all-targets -- -D warnings','explicit_toolchain_selector':'1.95.0','SDKROOT':'/Library/Developer/CommandLineTools/SDKs/MacOSX15.4.sdk','fresh_rustc_binary_hash_or_version_probe_in_package':False,'target':'APFS cloned owned engineering cache; not fresh/cold/performance','fts_max_control':{'six_observations':True,'rows':[0,1,257],'tables':['files','literal_index'],'all_max_fullscan_steps':0,'rows257_sum_control_steps':256,'all_explain_include_Last':True,'timing_measurement':False}},'boundaries':['This accepts these six actual commands on committed P; it does not assert a complete workspace test suite or workspace-wide Clippy.','No historical 3ff RED/GREEN execution is relabeled as c92e.','No original Gc8 primary sample or component result transfers to this new P.','These commands contain no actual mixed-load DB lock acquisition wait measurement; the P8-007 original contract gap remains. This product is the public API merge repair, not the external default-off DB observation implementation.','Root 10:46:04 preparation snapshot remains frozen at its then-pending native-controls state; this terminal review is appended separately.','Original task ledger remains 163 done /29 remaining; this review closes no original TODO.'],'original_controller_hashes':{n:meta(EX/n) for n in ['prepare-controller.py','controls-controller.py','package-controller.py']},'all_original_members':members}
output=OUT/'independent-native-P-evidence-review.json'
with output.open('x') as f:json.dump(report,f,ensure_ascii=False,indent=2);f.write('\n')
print(json.dumps({'decision':report['decision'],'source_P':P,'commands':6,'tests_passed':29,'maps':maps,'entries_per_map':1245,'report':meta(output)},ensure_ascii=False))
