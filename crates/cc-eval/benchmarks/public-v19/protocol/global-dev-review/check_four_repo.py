#!/usr/bin/env python3
"""Four-repo current public development admission; no protected bodies or ranking."""
import argparse
from collections import Counter
from itertools import combinations
import json
from pathlib import Path
import subprocess
import tempfile
import unicodedata

import check_admission as base

HERE=Path(__file__).resolve().parent
AUTHOR='4c01f627bd6df73e70bfe1dbdc42b2a9234e797a'
FIRST='d6a41bbcf76b5dc15e4924e54c7375f673d714f7'
REVIEW='704c0fd0901c85c51eaaa265bfc1aab49ccb3341'
AGGREGATE_SHA256='1da27a0dbe1a75d277f60daff2e043ce414f433ef1d03c62bd7e88d4255d05f5'
BLOCKS=['01-migration','02','03','04','05']
PAIR=['v19.typescript.f0021','v19.typescript.f0075']


def audit(evaluator,output,evaluator_build_receipt=None):
    result=base.audit(evaluator,output,evaluator_build_receipt,include_gin=True)
    errors=Counter(result['errors']);proofs=result['inputs_sha256']
    prefix='crates/cc-eval/benchmarks/public-v19/'
    get=lambda path:base.load_blob(AUTHOR,prefix+'typescript/'+path,proofs)
    review=lambda commit,path:base.load_blob(commit,prefix+'reviews/typescript/'+path,proofs)
    manifest_raw=get('source-manifest.json');manifest=json.loads(manifest_raw)
    locked=next(x for x in json.loads((HERE.parent/'source-locks.json').read_text())['candidates'] if x['repository'].lower().endswith('/typescript'))
    if manifest['upstream_sha']!=locked['source_sha']:errors['TS_UPSTREAM_LOCK']+=1
    admitted={r['path']:r for r in manifest['admitted_files']}
    if any(not path.startswith('packages/typescript/src/') or not path.endswith('.ts')
           or '..' in Path(path).parts for path in admitted):raise ValueError('SOURCE_OUTSIDE_ADMITTED_ALLOWLIST')
    source={};rows=[];lines={};queries={};validations=[];entries=[];spans=0;protocol_checks=[]
    with tempfile.TemporaryDirectory(prefix='v19-four-repo-dev-') as td:
        temp=Path(td)
        for block in BLOCKS:
            block_source=None
            for profile in ['native','compat']:
                path='blocks/'+block+'/suite.'+profile+'.candidate.json'
                suite_raw=get(path);suite=json.loads(suite_raw)
                if suite['queries']!='queries.'+profile+'.dev.jsonl':raise ValueError('QUERY_OUTSIDE_CURRENT_DEV_ALLOWLIST')
                if block_source is None:block_source=suite['source']
                elif (block_source['root']!=suite['source']['root'] or block_source['commit']!=suite['source']['commit']
                        or not set(suite['source']['files']).issubset(block_source['files'])):errors['TS_SOURCE_PROFILE_DRIFT']+=1
                target=temp/'typescript'/path;target.parent.mkdir(parents=True,exist_ok=True);target.write_bytes(suite_raw)
                source_root=(target.parent/suite['source']['root']).resolve()
                if not source_root.is_relative_to(temp):raise ValueError('SOURCE_ESCAPE')
                query_raw=get('blocks/'+block+'/'+suite['queries']);query_file=target.parent/suite['queries'];query_file.write_bytes(query_raw)
                queries[(block,profile)]=query_raw
                for line in query_raw.splitlines(keepends=True):
                    q=json.loads(line)
                    if q['split']!='dev':errors['NON_DEV_INTAKE']+=1
                    if profile=='native':
                        if q['query_family'] in lines:errors['TS_DUPLICATE_DEV_ID']+=1
                        rows.append(q);lines[q['query_family']]=base.sha(line)
                for path in suite['source']['files']:
                    if path not in admitted:raise ValueError('SOURCE_OUTSIDE_ADMITTED_ALLOWLIST')
                    raw=get('source/'+path);source[path]=raw
                    if base.sha(raw)!=admitted[path]['sha256'] or len(raw)!=admitted[path]['bytes']:errors['TS_SOURCE_BYTES_DRIFT']+=1
                    dest=source_root/path;dest.parent.mkdir(parents=True,exist_ok=True);dest.write_bytes(raw)
                proc=subprocess.run([str(evaluator),'validate','--suite',str(target)],capture_output=True)
                validations.append({'suite_sha256':base.sha(suite_raw),'exit_code':proc.returncode,'diagnostics_sha256':base.sha(proc.stdout+proc.stderr)})
                if proc.returncode:errors['EVALUATOR_VALIDATION']+=1
                entries.append(AUTHOR+':'+prefix+'typescript/blocks/'+block+'/suite.'+profile+'.candidate.json')
            countfile=temp/'typescript'/'blocks'/block/'protocol.json'
            args=['python3',str(HERE.parent/'check.py'),'--shard','typescript:native='+str(countfile.parent/'queries.native.dev.jsonl'),
                  '--shard','typescript:compat='+str(countfile.parent/'queries.compat.dev.jsonl'),'--output',str(countfile)]
            proc=subprocess.run(args,capture_output=True);p=json.loads(countfile.read_bytes());protocol_checks.append(p['errors'])
            if proc.returncode:errors.update({'PROTOCOL_'+k:v for k,v in p['errors'].items()})
    if len(rows)!=73 or sum(len(b.splitlines()) for (block,profile),b in queries.items() if profile=='compat')!=59:errors['TS_UNEXPECTED_ROWS']+=1
    if set(source)!=set(admitted) or len(source)!=20:errors['TS_SOURCE_INVENTORY']+=1
    for q in rows:
        for alt in [a for group in q['answers'] for a in group['alternatives']]:
            raw=source.get(alt['path']);s=alt.get('span')
            if raw is None or not s or not 0<=s['start']<s['end']<=len(raw):errors['TS_GOLD_SOURCE_BOUNDS']+=1
            else:
                try:raw[:s['start']].decode();raw[s['start']:s['end']].decode();spans+=1
                except UnicodeDecodeError:errors['SPAN_UTF8']+=1
        for e in q['annotations']['v19']['gold_evidence']:
            raw=source.get(e['path']);s,t=e['start_byte'],e['end_byte']
            if (raw is None or not 0<=s<t<=len(raw) or e['source_sha']!=manifest['upstream_sha']
                    or base.sha(raw)!=e['file_sha256'] or base.sha(raw[s:t])!=e['span_sha256']):errors['TS_SOURCE_EVIDENCE_DRIFT']+=1
    first_raw=review(FIRST,'dev-020-review.json');first=json.loads(first_raw)
    if first_raw!=review(REVIEW,'dev-020-review.json'):errors['TS_FIRST_REVIEW_MUTATED']+=1
    official_raw=review(FIRST,'upstream-source-admission.json');official=json.loads(official_raw)
    if official_raw!=review(REVIEW,'upstream-source-admission.json') or base.sha(official_raw)!=first['official_upstream_admission_receipt_sha256']:errors['TS_OFFICIAL_REVIEW_BINDING']+=1
    if (official['source_sha']!=manifest['upstream_sha'] or official['source_files_equal']!=20 or official['license_files_equal']!=3
            or official['permission_rejections'] or first['author_current_sha']!=AUTHOR
            or first['source_manifest_sha256']!=base.sha(manifest_raw) or first['reviewer_id']==first['author_id']):errors['TS_REVIEW_IDENTITY']+=1
    witnesses={r['sha256']:r for r in official['records']}
    licenses=[]
    for entry in manifest['license_artifacts']:
        raw=get(entry['path']);w=witnesses.get(base.sha(raw),{})
        if (base.sha(raw)!=entry['sha256'] or w.get('path_sha256')!=base.sha(entry['path'].encode())
                or w.get('bytes')!=len(raw) or not w.get('snapshot_matches_upstream')):errors['TS_LICENSE_BINDING']+=1
        licenses.append({'path':entry['path'],'sha256':base.sha(raw)})
    if len(licenses)!=3 or licenses[0]['sha256']!=locked['license_files'][0]['sha256']:errors['TS_ROOT_LICENSE_LOCK']+=1
    for path,raw in source.items():
        w=witnesses.get(base.sha(raw),{})
        if w.get('path_sha256')!=base.sha(('source/'+path).encode()) or w.get('bytes')!=len(raw) or not w.get('snapshot_matches_upstream'):errors['TS_OFFICIAL_BYTES_DRIFT']+=1
    aggregate_raw=review(REVIEW,'remaining/aggregate-review.json');aggregate=json.loads(aggregate_raw)
    if base.sha(aggregate_raw)!=AGGREGATE_SHA256:errors['TS_AGGREGATE_LOCK']+=1
    if (aggregate['author_current_sha']!=AUTHOR or aggregate['first20_frozen_sha']!=FIRST or aggregate['first20_receipt_sha256']!=base.sha(first_raw)
            or aggregate['first20_mutated'] or aggregate['reviewer_id']!=first['reviewer_id']):errors['TS_AGGREGATE_BINDING']+=1
    independent=list(first['rows']);review_inputs=list(first['input_dev_files'])
    for index,block in enumerate(['021-040','041-060','061-073']):
        raw=review(REVIEW,'remaining/block-'+block+'/review-receipt.json');r=json.loads(raw)
        if (base.sha(raw)!=aggregate['remaining_block_receipts'][index]['sha256'] or r['author_current_sha']!=AUTHOR
                or r['reviewer_id']!=first['reviewer_id'] or r['first20_receipt_sha256']!=base.sha(first_raw)
                or r['inherited_official_source_admission_receipt_sha256']!=base.sha(official_raw)
                or set(r['scope_error_codes'])-{'G_CROSS_REPOSITORY_GLOBAL_TEMPLATE_EQUIVALENCE_AND_BLOCKED_COMPONENTS_UNREVIEWED','H_CUSTODY_BLOCKED_27'}):errors['TS_BLOCK_REVIEW_BINDING']+=1
        independent.extend(r['rows']);review_inputs.extend(r['input_dev_files'])
    for r in review_inputs:
        for profile in ['native','compat']:
            if r[profile+'_dev_sha256']!=base.sha(queries[(r['block'],profile)]):errors['TS_REVIEW_FILE_BINDING']+=1
    expected={r['family_id_sha256']:r['row_sha256'] for r in independent}
    actual={base.sha(k.encode()):v for k,v in lines.items()}
    if len(independent)!=73 or expected!=actual:errors['TS_ROW_REVIEW_BINDING']+=1
    unresolved=[]
    for r in independent:
        if r['decision']=='needschange':
            unresolved.append(r)
            if (r.get('query_id')!='v19.typescript.f0075.en01' or r.get('source_fact_review')!='accept'
                    or r['error_codes']!=['C_TASK_EQUIVALENCE_REVIEW_NEEDED']):errors['TS_CONTENT_REVIEW_INCOMPLETE']+=1
        elif r['decision']!='accept' or r['error_codes']:errors['TS_CONTENT_REVIEW_INCOMPLETE']+=1
    if len(unresolved)!=1:errors['TS_COMPONENT_DISPUTE_BINDING']+=1
    component_raw=review(REVIEW,'remaining/component-review.json');component=json.loads(component_raw)
    if (base.sha(component_raw)!=aggregate['component_review_sha256'] or component['local_components_accepted']!=71
            or len(component['component_disputes'])!=1):errors['TS_COMPONENT_REVIEW_BINDING']+=1
    decision=json.loads((HERE/'typescript-extension/pair-adjudication.json').read_text())
    by_family={q['query_family']:q for q in rows};members=decision['members']
    if members!=PAIR or decision['decision']!='conservative_shared_required_obligation_one_component':errors['TS_PAIR_UNRESOLVED']+=1
    if decision['native_row_sha256']!=[lines[m] for m in PAIR]:errors['TS_PAIR_ROW_DRIFT']+=1
    obligations=[]
    for member in PAIR:
        obligations.append({(a['path'],a['span']['start'],a['span']['end']) for g in by_family[member]['answers'] for a in g['alternatives']})
    shared=obligations[0]&obligations[1]
    if not shared or not obligations[0].issubset(obligations[1]):errors['TS_PAIR_OBLIGATION_DRIFT']+=1
    shared_hashes=sorted(base.sha(source[path][s:t]) for path,s,t in shared)
    if shared_hashes!=decision['shared_obligation_span_sha256'] or shared_hashes!=component['component_disputes'][0]['shared_specific_contract_evidence_sha256']:errors['TS_PAIR_SOURCE_DRIFT']+=1
    # Preserve local projection of the three frozen public repos; never load other nodes' bodies.
    all_rows=list(rows);local_edges=[PAIR];repo_rows={'typescript':rows}
    for name,spec in {**base.SPECS,'gin':base.GIN_SPEC}.items():
        path=str(Path(spec['suites'][0]).parent/'queries.native.dev.jsonl')
        raw=base.load_blob(spec['author'],prefix+name+'/'+path,proofs)
        native=[json.loads(l) for l in raw.splitlines()];all_rows.extend(native);repo_rows[name]=native
        relations=json.loads(base.load_blob(spec['author'],prefix+name+'/'+spec['relations'],proofs))
        local_edges.extend(c['members'] for c in relations['components'])
    rules=json.loads((HERE/'cross-repo-decisions.json').read_text());gin=json.loads((HERE/'gin-extension/cross-repo-decisions.json').read_text())
    edges=[r['members'] for r in rules['conservative_leakage_edges']+gin['conservative_leakage_edges']]
    comparisons=json.loads((HERE/'typescript-extension/cross-repo-decisions.json').read_text())
    if comparisons['native_query_file_sha256']!={b:base.sha(queries[(b,'native')]) for b in BLOCKS}:errors['TS_CROSS_REVIEW_FILE_BINDING']+=1
    all_by_family={q['query_family']:q for q in all_rows}
    for d in comparisons['pair_decisions']:
        if d['decision']!='no_task_equivalence_union' or d['question_record_sha256']!=[base.sha(base.canon(all_by_family[m])) for m in d['members']]:errors['TS_CROSS_DECISION_BINDING']+=1
    norm=lambda s:' '.join(unicodedata.normalize('NFKC',s).casefold().split())
    pairs=[(a,b) for left,right in combinations(repo_rows.values(),2) for a in left for b in right]
    identical=sum(norm(a['query'])==norm(b['query']) for a,b in pairs)
    if identical:errors['CROSS_REPO_IDENTICAL_QUERY_UNADJUDICATED']+=identical
    registry=base.components(all_rows,local_edges+edges)
    result['repo_results']['typescript']={'author_sha':AUTHOR,'review_sha':REVIEW,'first20_review_sha':FIRST,'upstream_sha':manifest['upstream_sha'],
        'native_dev_rows':73,'compat_dev_rows':59,'source_fact_hash_accept':73,'independent_content_accept_before_pair_adjudication':72,
        'independent_component_needschange_preserved':1,'current_content_hash_accept':73 if not errors else 0,
        'local_components':len(base.components(rows,[PAIR])),'conservative_local_pair_adjudications':1,
        'source_files_verified':len(source),'spans_verified':spans,'license_files_verified':licenses,
        'snapshot_only_not_git_cleanliness':True,'actual_evaluator_validations':validations,'actual_protocol_checks':protocol_checks,'dev_suite_entries':entries,
        'aggregate_review_sha256':base.sha(aggregate_raw),'pair_adjudication_sha256':base.sha((HERE/'typescript-extension/pair-adjudication.json').read_bytes())}
    result['global_review'].update({'current_native_dev_rows_read':len(all_rows),'cross_pairs_automatically_checked':len(pairs),
        'exact_normalized_cross_repo_duplicates':identical,'manual_pair_decisions':len(rules['pair_decisions'])+len(gin['pair_decisions'])+len(comparisons['pair_decisions']),
        'local_components_before_cross_repo':sum(r['local_components'] for r in result['repo_results'].values()),
        'conservative_global_correlation_components':len(registry),'registry_sha256':base.sha(base.canon(registry))})
    result.update({'status':'development_admitted_snapshot_scope_only' if not errors else 'development_admission_blocked','errors':dict(errors),
        'development_admitted_native_rows':len(all_rows) if not errors else 0,
        'remaining':['custody_blocked','heldout_cleanliness','serde_vite_source_blocked','six_repo_target','formal_facet_graph_statistics']})
    output.mkdir(parents=True,exist_ok=True)
    (output/'admission.json').write_text(json.dumps(result,indent=2)+'\n')
    (output/'global-components.json').write_text(json.dumps({'scope':'current_public_dev_conservative_correlation_not_confirmatory','components':registry},indent=2)+'\n')
    return result


def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--evaluator',type=Path,required=True)
    parser.add_argument('--output',type=Path,required=True);parser.add_argument('--evaluator-build-receipt',type=Path);args=parser.parse_args()
    try:r=audit(args.evaluator.resolve(),args.output,args.evaluator_build_receipt)
    except (OSError,subprocess.SubprocessError,KeyError,ValueError,TypeError):
        r={'status':'development_admission_blocked','errors':{'INPUT_OR_EVALUATOR_BLOCKER':1}}
        args.output.mkdir(parents=True,exist_ok=True);(args.output/'admission.json').write_text(json.dumps(r,indent=2)+'\n')
    print(json.dumps({'status':r['status'],'errors':r['errors'],'receipt_sha256':base.sha((args.output/'admission.json').read_bytes())}))
    return bool(r['errors'])


if __name__=='__main__':raise SystemExit(main())
