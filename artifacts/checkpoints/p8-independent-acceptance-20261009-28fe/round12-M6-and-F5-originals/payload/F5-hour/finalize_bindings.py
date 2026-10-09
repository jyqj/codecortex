#!/usr/bin/env python3
"""Narrow final custody checks on frozen original bytes; no native execution."""
import datetime, hashlib, json, re, zipfile
from collections import Counter
from pathlib import Path, PurePosixPath
OUT=Path(__file__).resolve().parent
HEAD='c2ad27b2b189cbc98775f20a550d718dd4913038'
def sha(data):return hashlib.sha256(data).hexdigest()
def jb(obj):return (json.dumps(obj,ensure_ascii=False,sort_keys=True,indent=2)+'\n').encode()
def fi(p):d=p.read_bytes();return {'path':str(p),'bytes':len(d),'sha256':sha(d)}
checks=[]
def check(name,condition,detail=None):
    checks.append({'name':name,'passed':bool(condition),'detail':detail})
    assert condition,name
rawlog=(OUT/'official-soak-job.log').read_bytes()
text=rawlog.decode('utf-8-sig')
lines=[re.sub(r'^\d{4}-\d\d-\d\dT[0-9:.]+Z ','',line) for line in text.splitlines()]
stripped='\n'.join(lines);decoder=json.JSONDecoder();objects=[]
for match in re.finditer(r'(?m)^\{',stripped):
    try:obj,n=decoder.raw_decode(stripped[match.start():]);objects.append(obj)
    except ValueError:pass
with zipfile.ZipFile(OUT/'11588677565-F5-soak.zip') as z:
    receipt=json.loads(z.read('p8-build/build-receipt.json'));report=json.loads(z.read('p8-runtime/report.json'));plan=json.loads(z.read('p8-runtime/plan.json'))
    check('original stdout report exactly matches artifact JSON',[v for v in objects if v.get('offered')==3601 and v.get('profile')=='soak']==[report])
    verify={'status':'sealed_artifacts_verified','files':2110,'observation_status':'passed_observation','observation_exit_code':0}
    check('original verify CLI stdout contains exact successful receipt',verify in objects)
    check('actual checkout requested and completed F5',f'git checkout --progress --force {HEAD}' in text and any(line==HEAD for line in lines))
    check('original upload binds exact artifact and size','Artifact ID 11588677565' in text and 'Final size is 51012345 bytes' in text)
    target=receipt['target_dir'];events=[json.loads(line) for line in z.read('p8-build/product-build.jsonl').splitlines()]
    check('original build target and command',receipt['build_dir']==target and receipt['target_initially_absent'] is True and receipt['build_command']==['cargo','build','--release','--locked','--offline','--no-default-features','-p','cc-server','--bin','codecortex','-p','cc-eval','--bin','p8-oracle','--bin','p8-runtime-statistics','--message-format=json-render-diagnostics','--target-dir',target])
    source_roots=[];binaries={}
    for name,package,source in [('codecortex','cc-server','src/main.rs'),('p8-oracle','cc-eval','src/bin/p8-oracle.rs'),('p8-runtime-statistics','cc-eval','src/bin/p8-runtime-statistics.rs')]:
        a=receipt['artifacts'][name]['cargo_artifact'];matches=[e for e in events if e.get('reason')=='compiler-artifact' and e.get('target',{}).get('name')==name and e.get('target',{}).get('kind')==['bin']]
        root=a['manifest_path'].removesuffix('/crates/'+package+'/Cargo.toml');source_roots.append(root)
        check('unique exact original optimized target '+name,matches==[a] and a['features']==[] and a['fresh'] is False and a['profile']['test'] is False and a['profile']['opt_level']=='3' and a['profile']['debug_assertions'] is False and a['target']['src_path']==root+'/crates/'+package+'/'+source and a['executable']==target+'/release/'+name)
        body=z.read('p8-build/'+name);binary=receipt['artifacts'][name]
        check('original binary source and preserved copy '+name,len(body)==binary['binary_bytes']==binary['copy_source']['bytes'] and sha(body)==binary['binary_sha256']==binary['copy_source']['sha256'] and binary['copy_source']['path']==a['executable'])
        binaries[name]={'bytes':len(body),'sha256':sha(body),'cargo_copy_source':a['executable'],'executed_by_reviewer':False}
    check('one original source checkout',len(set(source_roots))==1)
    check('actual toolchain identities repeat through final runtime',receipt['toolchain_before']==receipt['toolchain_after']==plan['build_identity']['toolchain']==report['final_build_verification']['toolchain'])
    for name in ['cargo','rustc']:
        t=receipt['toolchain_before'][name]
        check('original recorded compiler invocation '+name,t['command']==[t['invocation'],'--version','--verbose'] and t['executable'].startswith('/') and len(t['executable_sha256'])==64 and '1.95.0' in t['version'])
base=json.loads((OUT/'independent-originals-report.json').read_text())
check('complete original 77-check audit succeeded',base['state']=='scoped_F5_original_onehour_verified' and len(base['checks'])==77 and not base['failed_checks'])
api=OUT/'official-api'
jobs599=json.loads((api/'s599jobs.json').read_text());d0a=json.loads((api/'d0jobs1.json').read_text());d0b=json.loads((api/'d0jobs2.json').read_text());d0jobs=d0a['jobs']+d0b['jobs']
check('complete D0 151-job two-page snapshot',len(d0jobs)==d0a['total_count']==d0b['total_count']==151 and len({j['id'] for j in d0jobs})==151)
measurement=[j for j in d0jobs if j['name'].startswith('measure (')]
coordinates=[tuple(map(int,re.fullmatch(r'measure \((\d+), (\d+)\)',j['name']).groups())) for j in measurement]
check('D0 all 150 unique original coordinates',set(coordinates)=={(n,r) for n in [1000,5000,10000,50000,100000] for r in range(30)} and len(coordinates)==150)
snap={'kind':'single_official_status_snapshot_not_new_product_execution','source_api_receipts':{p.name:fi(p) for p in sorted(api.glob('*.json'))},
      '599':{'run':37835810882,'jobs':jobs599['total_count'],'status_counts':dict(Counter(j['status'] for j in jobs599['jobs'])),'100k':next(j for j in jobs599['jobs'] if j['id']==113519201733),'conclusion':'Still actual in_progress from20:23:41; potential5h clock01:23:41 is not an observed terminal or failure.'},
      'D0':{'run':37854240827,'controller':json.loads((api/'d0run.json').read_text())['head_sha'],'measurement_count':len(measurement),'status_counts':dict(Counter(j['status'] for j in measurement)),'conclusion_counts':dict(Counter(str(j['conclusion']) for j in measurement)),'actual_measurement_step_starts':[{'job':j['id'],'coordinate':j['name'],'step':s} for j in measurement for s in j['steps'] if s['name']=='Execute every original phase for this new registered repetition' and s['started_at']],
           'artifact_count':json.loads((api/'d0artifacts.json').read_text())['total_count'],'artifact_names':[a['name'] for a in json.loads((api/'d0artifacts.json').read_text())['artifacts']],
           'conclusion':'One original rep8 infrastructure-interrupted failure preserved separately; no new completed native measurement shard appears in this snapshot. Capacity artifacts are not completed study measurements. Queued job started_at alone is not evidence of actual execution.'},
      'remaining_original_todos':29,'new_original_completed':0}
(OUT/'concurrent-study-snapshot.json').write_bytes(jb(snap))
summary={'schema_version':1,'created_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'source':HEAD,'run':37858536446,'job':113588467257,'artifact':11588677565,
         'base_audit':fi(OUT/'independent-originals-report.json'),'additional_custody_checks':checks,'additional_checks_passed':len(checks),'binary_identities':binaries,
         'status':'complete_scoped_F5_hour_originals_verified_and_eligible_for_unchanged_M6_success_behavior_bridge',
         'bridge_static_report':fi(OUT.parent/'round12-F5-M6-success-bridge/conditional-success-bridge-review.json'),
         'limits':['Actual execution remains F5; no source relabeling or artifact code execution.',
                   'Remote original target/compiler paths are historical receipt and original job evidence, not present local same-job verify_receipt proof.',
                   'Do not certify M6 new seal-failure handling from the old F5 hour; separate seven original regression controls cover that change.',
                   'No pooling with incomplete local M5/M6 runs or erasure of their raw/failed resource prefixes.',
                   'Shared query-pool counters/cache reuse observed; no individual OS-thread identity or real semantic-provider claim.',
                   'Default profile only, no hard dependency/full TODO/release closure; totals192/163/29 unchanged.'],
         'original_job_log':fi(OUT/'official-soak-job.log'),'download_receipt':fi(OUT/'download-receipt.json')}
(OUT/'final-originals-custody-and-bridge.json').write_bytes(jb(summary))
(OUT/'evidence-manifest.json').write_bytes(jb({str(p.relative_to(OUT)):fi(p) for p in sorted(OUT.rglob('*')) if p.is_file() and p.name!='evidence-manifest.json'}))
print(json.dumps({'additional_checks':len(checks),'failed':0,'final_report':fi(OUT/'final-originals-custody-and-bridge.json'),'status':summary['status']}))
