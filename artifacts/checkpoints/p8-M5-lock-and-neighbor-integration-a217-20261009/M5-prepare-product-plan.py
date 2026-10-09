import datetime,hashlib,json,pathlib,subprocess,tomllib
B=pathlib.Path('/workspace/scratch/a217aaae3bde');O=B/'p8-db-lock-observation-integration';REPO=B/'codecortex-recovered'
M='832f79b8702c6cdfb567b7cf949508cd4589f286';G2='4d18dcdb34b5d7a277566b57801cc882d8b1eb61'
def git(*a):return subprocess.check_output(['git','-C',str(REPO),*a])
def sha(b):return hashlib.sha256(b).hexdigest()
def oid(b):return hashlib.sha1(b'blob '+str(len(b)).encode()+b'\0'+b).hexdigest()
def serial(x):return (json.dumps(x,ensure_ascii=False,indent=2,sort_keys=True)+'\n').encode()
def save(n,x):b=serial(x);(O/n).open('xb').write(b);return {'path':str(O/n),'bytes':len(b),'sha256':sha(b)}
m=json.loads((O/'M4-reviewed-source-registry-v15.json').read_bytes());g=json.loads((O/'LG2-reviewed-source-registry-v15.json').read_bytes())
rows=json.loads((O/'LG2-three-path-repair-inputs.json').read_bytes())['current15']
prior=json.loads((B/'scale-pr180-C-review/future-M2-LP-Cargo-text-and-map-reconstruction.json').read_bytes())
merged=prior['stdout_utf8'].encode();assert sha(merged)=='7808969f63a3da11106471fea722f715a91b48f35086473df756b25c8576f742'
toml=tomllib.loads(merged.decode());assert toml['dependencies']['toml_edit']=='0.22.27' and toml['features']['p8-db-lock-observation']==['cc-db/p8-db-lock-observation']
tie=B/'preselect-pr3-tie-candidate/crates/cc-search/src/preselect.rs';traw=tie.read_bytes();assert sha(traw)=='a3aea905493d4204cf00209a53918175e2b31b84eed643b76ea19c09bcb011bb'
assert git('show',M+':crates/cc-search/src/preselect.rs')==(B/'preselect-pr3-tie-candidate/original-G4-preselect.rs').read_bytes()
peer=B/'preselect-pr3-tie-candidate/independent-review-pr-audit.json';assert sha(peer.read_bytes())=='255e31d8317b3398950c5aef5b3a944090d6015ea86af75eee191ea8f3124e2e'
controls=B/'original-LG2-runtime/admission-controls-independent-review.json';assert sha(controls.read_bytes())=='b54b64877a3a14fc624eccd75da19def453797f91ec84e6b2a644d7ebba0c8da'
assert json.loads(controls.read_bytes())['source']==G2
pm=dict(m['complete_inputs']);vm=dict(m['validation_inputs']);delta=dict(m['delta']);elements=[];custom=[]
for row in rows:
 p=row['path'];raw=git('show',G2+':'+p);assert oid(raw)==row['git_blob'] and sha(raw)==row['sha256'] and len(raw)==row['bytes']
 if p=='crates/cc-server/Cargo.toml':
  assert oid(git('show',M+':'+p))==prior['input_bindings'][0]['git_blob'];raw=merged
  f=O/'M5-merged-cc-server-Cargo.toml';assert not f.exists() or f.read_bytes()==raw
  if not f.exists():f.open('xb').write(raw)
  custom.append({'source':str(f),'path':p,'content':raw.decode(),'sha':oid(raw)})
 h=sha(raw)
 if p.startswith('crates/'):
  pm[p]=h;delta[p]={'before_sha256':(m['delta'].get(p) or g['delta'][p])['before_sha256'],'sha256':h}
 elif p in g['validation_inputs']:vm[p]=h
 elements.append({'path':p,'mode':'100644','type':'blob','sha':oid(raw),'bytes':len(raw),'sha256':h,'source_identity':G2,'selection':'verified merged Cargo' if p=='crates/cc-server/Cargo.toml' else 'exact G2 bytes'})
p='crates/cc-search/src/preselect.rs';pm[p]=sha(traw);delta[p]={'before_sha256':sha(git('show','7354db236c9d9850a75f31672697ae9eab44565e:'+p)),'sha256':sha(traw)}
elements.append({'path':p,'mode':'100644','type':'blob','sha':oid(traw),'bytes':len(traw),'sha256':sha(traw),'source_identity':'isolated G4-based candidate, independent peer review','selection':'one final comparator hunk and two meaningful SQLite tests'})
custom.append({'source':str(tie),'path':p,'content':traw.decode(),'sha':oid(traw)})
assert len(elements)==16 and len(pm)==1095 and len(delta)==70 and len(vm)==142 and vm==g['validation_inputs']
assert pm['Cargo.lock']==m['complete_inputs']['Cargo.lock']
for p in ['crates/cc-search/src/lanes.rs','crates/cc-search/src/query_policy.rs','crates/cc-eval/src/benchmark/ablation/mechanism_controls.json']:assert pm[p]==m['complete_inputs'][p]
plan={'schema':'M5-fixed-minimal-product-plan-v1','author':'/root/scale_closeout','prepared_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'parents':[M,G2],'base_tree':git('rev-parse',M+'^{tree}').decode().strip(),'elements':elements,'element_count':16,'product_count':len(pm),'original_BASE_delta_count':len(delta),'validation_count':len(vm),'complete_inputs':pm,'delta':delta,'validation_inputs':vm,'complete_inputs_sha256':sha(serial(pm)),'validation_inputs_sha256':sha(serial(vm)),'delta_sha256':sha(serial(delta)),'prior_reviews':{'M4':m['review_source'],'G2':g['review_source'],'G2_controls':{'path':str(controls),'sha256':sha(controls.read_bytes())},'preselect_peer':{'path':str(peer),'sha256':sha(peer.read_bytes())}},'retention':'All G4 tree leaves remain exact except the sixteen listed inputs; G2 whole tree and old guards are not copied. New task definitions/status changes: none. No PR137 changes. G2 workflow branch trigger stays exact.','guard_policy':'Original v15 algorithm with new P/R/review-path/registry digest, no extra acceptance criteria.','execution_scope':'G2 controls apply only to G2. M4 source guard applies only to M4. New preselect fmt/compile/tests and all M5 behavioral execution are not_run.','current_C_study_unchanged':{'head':'3ffcefc3b28ee1a4ed80caecebd7208a45c3e302','run':37910924354,'accepted_shards':4,'accepted_samples':41,'required_shards':150,'required_samples':1500},'formal_completion':False,'remaining_todos':29}
print(json.dumps(save('M5-fixed-product-composition-plan.json',plan)))
save('M5-custom-blob-payloads.json',custom)
