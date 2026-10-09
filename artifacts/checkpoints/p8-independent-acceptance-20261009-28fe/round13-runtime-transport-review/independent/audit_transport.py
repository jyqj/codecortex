#!/usr/bin/env python3
"""Independent actual restoration and transport negatives; no archive unpack/native run."""
import copy, datetime, hashlib, json, os, shutil, stat, subprocess, sys, time
from pathlib import Path
OUT=Path(__file__).resolve().parent;ROOT=OUT.parent.parent
PREFIX='artifacts/checkpoints/p8-independent-acceptance-20261009-28fe/round12-M6-and-F5-originals'
V1=ROOT/'integration-validation/round13-runtime-delivery';V2=ROOT/'integration-validation/round13-runtime-delivery-v2'
PACKAGE=V2/'delivery'/PREFIX
def jb(v):return (json.dumps(v,ensure_ascii=False,sort_keys=True,indent=2)+'\n').encode()
def sha(data):return hashlib.sha256(data).hexdigest()
def ident(p):
    s=p.lstat();assert stat.S_ISREG(s.st_mode)
    h=hashlib.sha256();g=hashlib.sha1();g.update(('blob '+str(s.st_size)+'\0').encode());n=0
    with p.open('rb') as f:
        for b in iter(lambda:f.read(1048576),b''):h.update(b);g.update(b);n+=len(b)
    assert n==s.st_size
    return {'bytes':n,'sha256':h.hexdigest(),'git_blob':g.hexdigest(),'mode':'100755' if s.st_mode&0o111 else '100644'}
checks=[];executions=[]
def check(name,v,details=None):checks.append({'name':name,'passed':bool(v),'details':details});assert v,name
def write(name,v):(OUT/name).write_bytes(jb(v))
def actual(root,output,name):
    argv=[sys.executable,'-B',str(root/'restore_originals.py'),'--output',str(output)]
    t=time.monotonic();p=subprocess.run(argv,stdout=subprocess.PIPE,stderr=subprocess.PIPE,timeout=120)
    (OUT/(name+'.stdout')).write_bytes(p.stdout);(OUT/(name+'.stderr')).write_bytes(p.stderr)
    sidecar=output.with_name(output.name+'.restore-receipt.json')
    record={'name':name,'argv':argv,'exit_code':p.returncode,'elapsed_seconds':time.monotonic()-t,'stdout_sha256':sha(p.stdout),'stderr_sha256':sha(p.stderr),'receipt_present':sidecar.is_file(),'output_exists':output.exists(),'output_symlink':output.is_symlink()}
    if sidecar.is_file():
        data=sidecar.read_bytes();(OUT/(name+'.original-restore-receipt.json')).write_bytes(data)
        try:record['receipt_status']=json.loads(data).get('status')
        except ValueError:record['receipt_status']='preexisting_non_json'
    executions.append(record);write('executions-in-progress.json',executions)
    return p,record
def files(root):
    result={}
    for p in sorted(root.rglob('*')):
        assert not p.is_symlink()
        if p.is_file():result[str(p.relative_to(root))]=ident(p)
    return result
started=datetime.datetime.now(datetime.timezone.utc).isoformat()
r1=json.loads((V1/'delivery-receipt.json').read_bytes());r2=json.loads((V2/'delivery-receipt.json').read_bytes());m=json.loads((PACKAGE/'logical-package-manifest.json').read_bytes())
check('fixed v2 receipt',sha((V2/'delivery-receipt.json').read_bytes())=='efdaeabaf323c8aafb5c2885c3f880f15b92ef06d7230806b90da7a7c2d06ff8')
check('exact fixed restorer',sha((PACKAGE/'restore_originals.py').read_bytes())=='b590ea763f458d0090b6ac5fc8216784db1c6533f610aedd2b9e0628d7f1bf71')
physical=files(PACKAGE);recorded={x['path'].removeprefix(PREFIX+'/'):{k:x[k] for k in ['bytes','sha256','git_blob','mode']} for x in r2['files']}
check('all106 physical files and receipt',physical==recorded and len(physical)==r2['file_count']==106 and sum(v['bytes'] for v in physical.values())==r2['total_bytes']==95810650)
pi=json.loads((PACKAGE/'package-index.json').read_bytes());check('physical package index exact105 nonself files',{x['path']:{k:x[k] for k in ['bytes','sha256','git_blob','mode']} for x in pi['files']}=={k:v for k,v in physical.items() if k!='package-index.json'} and len(pi['files'])==105)
logical={x['path'].removeprefix(PREFIX+'/'):{k:x[k] for k in ['bytes','sha256','git_blob','mode']} for x in r1['files']}
check('all90 fixed reviewed v1 logical identities',{x['path']:{k:x[k] for k in ['bytes','sha256','git_blob','mode']} for x in m['files']}==logical and len(m['files'])==90 and sum(x['bytes'] for x in m['files'])==m['logical_bytes']==95282009)
check('v1 independent proof and receipt retained',sha((PACKAGE/'logical-v1-independent-review.json').read_bytes())==m['v1_independent_review_sha256']=='5deb7fb09bdb75970901da012363332356a8054bd5f2ac0576c9b82854ef58e9' and sha((PACKAGE/'logical-v1-delivery-receipt.json').read_bytes())==m['logical_v1_delivery_receipt_sha256']=='2e2722b20088d52e24e3839ca1b0488c8eccbd73fe53950ea33c90f44d43c93e')
parts=[p for x in m['files'] for p in x.get('segments',[])]
check('two original files fixed eleven bounded parts',sum('segments'in x for x in m['files'])==2 and len(parts)==11 and m['fixed_maximum_segment_bytes']==8388608 and all(0<p['bytes']<=8388608 and all(physical[p['path']][k]==p[k] for k in ['bytes','sha256','git_blob']) for p in parts))
for entry in m['files']:
    if 'segments' in entry:
        check('contiguous part ordinals '+entry['path'],[p['ordinal'] for p in entry['segments']]==list(range(len(entry['segments']))))
        h=hashlib.sha256();n=0
        for part in entry['segments']:
            with (PACKAGE/part['path']).open('rb') as f:
                for b in iter(lambda:f.read(1048576),b''):h.update(b);n+=len(b)
        check('parts reconstruct original whole SHA '+entry['path'],n==entry['bytes'] and h.hexdigest()==entry['sha256'])
    else:check('unchanged small payload '+entry['path'],physical[entry['payload']]==logical[entry['path']])
check('88 exact ordinary payload files',sum('payload'in x for x in m['files'])==88)

positive=OUT/'restored-positive'
p,success=actual(PACKAGE,positive,'positive')
check('actual restoration command exit0',p.returncode==0 and success.get('receipt_status')=='exact_reviewed_logical_package_restored')
restored=files(positive);check('all90 restored files exact SHA Git blob mode and no extras',restored==logical and len(restored)==90 and sum(x['bytes'] for x in restored.values())==95282009)
write('independent-restored90-identities.json',restored)
side=positive.with_name(positive.name+'.restore-receipt.json');side_before=side.read_bytes()
p,record=actual(PACKAGE,positive,'existing_positive_output')
check('existing completed output rejected without any modification',p.returncode!=0 and files(positive)==restored and side.read_bytes()==side_before)
shutil.rmtree(positive);side.unlink()

negative_cases=['corrupt_segment','truncated_segment','missing_segment','reordered_bytes_with_valid_part_hashes','duplicate_logical_path','traversal_logical_path','symlink_segment','corrupt_payload','segment_ordinal_gap','existing_receipt','existing_output_symlink','missing_output_parent']
case_results=[]
for kind in negative_cases:
    case=OUT/('case-'+kind);case.mkdir();source=case/'package';source.mkdir();out=case/'output'
    # Read-only hardlinked inputs save space. Every changed test file is unlinked first.
    for relative in physical:
        target=source/relative;target.parent.mkdir(parents=True,exist_ok=True);os.link(PACKAGE/relative,target)
    mm=copy.deepcopy(m);seg=next(x for x in mm['files'] if 'segments'in x);part=seg['segments'][0];target=source/part['path']
    sentinel=case/'outside-sentinel';sentinel.write_bytes(b'never overwrite independent sentinel\n');sentinel_before=sentinel.read_bytes()
    changed_manifest=False
    if kind=='corrupt_segment':
        b=target.read_bytes();target.unlink();target.write_bytes(bytes([b[0]^1])+b[1:])
    elif kind=='truncated_segment':
        b=target.read_bytes();target.unlink();target.write_bytes(b[:-1])
    elif kind=='missing_segment':target.unlink()
    elif kind=='reordered_bytes_with_valid_part_hashes':
        seg['segments'][0],seg['segments'][1]=seg['segments'][1],seg['segments'][0]
        for i,x in enumerate(seg['segments']):x['ordinal']=i
        changed_manifest=True
    elif kind=='duplicate_logical_path':mm['files'][1]['path']=mm['files'][0]['path'];changed_manifest=True
    elif kind=='traversal_logical_path':mm['files'][0]['path']='../outside-sentinel';changed_manifest=True
    elif kind=='symlink_segment':target.unlink();target.symlink_to(PACKAGE/part['path'])
    elif kind=='corrupt_payload':
        entry=next(x for x in mm['files'] if 'payload'in x);f=source/entry['payload'];b=f.read_bytes();f.unlink();f.write_bytes(b+b'corruption')
    elif kind=='segment_ordinal_gap':part['ordinal']=3;changed_manifest=True
    elif kind=='existing_receipt':out.with_name(out.name+'.restore-receipt.json').write_bytes(b'preexisting receipt must survive\n')
    elif kind=='existing_output_symlink':out.symlink_to(case,target_is_directory=True)
    elif kind=='missing_output_parent':out=case/'no-parent'/'output'
    if changed_manifest:
        f=source/'logical-package-manifest.json';f.unlink();f.write_bytes(jb(mm))
    before_receipt=out.with_name(out.name+'.restore-receipt.json');old_receipt=before_receipt.read_bytes() if before_receipt.is_file() else None
    p,record=actual(source,out,kind)
    check('negative rejection '+kind,p.returncode!=0 and record.get('receipt_status')!='exact_reviewed_logical_package_restored' and sentinel.read_bytes()==sentinel_before)
    if old_receipt is not None:check('preexisting receipt preserved '+kind,before_receipt.read_bytes()==old_receipt)
    if kind in ['corrupt_segment','truncated_segment','reordered_bytes_with_valid_part_hashes','corrupt_payload']:
        check('copy failure retains explicit failed receipt '+kind,record.get('receipt_status')=='failed')
    case_results.append(record)
    shutil.rmtree(case)
check('all106 original physical files still unchanged',files(PACKAGE)==physical)
check('fixed v2 receipt remained unchanged',sha((V2/'delivery-receipt.json').read_bytes())=='efdaeabaf323c8aafb5c2885c3f880f15b92ef06d7230806b90da7a7c2d06ff8')
readme=(PACKAGE/'README.md').read_text()
check('transport does not change acceptance scope',all(s in readme for s in ['没有重压','其余 88','正式完成 0/10','未知小时','资源覆盖 false','原规模 150/1500','不打开 SQLite','不解 ZIP/tar']))
result={'schema_version':1,'kind':'non_author_actual_segment_restore_and_negative_controls','started_at':started,'finished_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),
 'verdict':'APPROVE_v2_segment_transport_exact_v1_restoration','source_receipt_sha256':'efdaeabaf323c8aafb5c2885c3f880f15b92ef06d7230806b90da7a7c2d06ff8','restorer_sha256':'b590ea763f458d0090b6ac5fc8216784db1c6533f610aedd2b9e0628d7f1bf71',
 'physical_files':106,'physical_bytes':95810650,'segments':11,'max_segment_bytes':8388608,'logical_restored_files':90,'logical_restored_bytes':95282009,
 'checks_passed':len(checks),'checks_failed':0,'checks':checks,'actual_restore_and_existing_output_controls':executions[:2],'additional_negative_controls':case_results,
 'all_positive_and_negative_restored_workdirs_deleted_after_checks':True,'original_physical_inputs_unchanged':True,
 'limits':['Transport only: no archive decompression, SQLite connection, original payload/native execution, benchmark run or TODO closure.',
 'Original restorer is executed only as the requested transport tool. Its payload files are copied or concatenated, never executed.',
 'Validation failures before output reservation reject nonzero without a new restore receipt. Mid-copy corruption/reordering retains an explicit failed receipt. Preexisting output or receipt is never overwritten.',
 'Original v1 native/source/README conclusions remain exactly the approved90 logical bytes; physical segmentation adds no acceptance or changes to source identities.',
 'This review approves frozen content and transfer behavior. Actual published Git tree/commit and remote part bytes still require normal readback.'],
 'ledger_unchanged':{'total':192,'done':163,'remaining':29,'new_original_completed':0}}
write('independent-v2-transport-review.json',result)
print(json.dumps({'verdict':result['verdict'],'checks':len(checks),'negative_cases':len(case_results)+1,'report_bytes':(OUT/'independent-v2-transport-review.json').stat().st_size,'report_sha256':sha((OUT/'independent-v2-transport-review.json').read_bytes())}))
