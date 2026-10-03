#!/usr/bin/env python3
"""Frozen two-repo development admission; allowlisted current dev only, no ranking."""
import argparse
from collections import Counter
import hashlib
from itertools import combinations
import json
from pathlib import Path
import subprocess
import tempfile
import unicodedata

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[5]
DECISIONS = HERE/'cross-repo-decisions.json'
SPECS = {
 'express': {'author':'465e9e0bd435e2e30c08de8702f78a0d10c49c8e', 'review':'0c0ec9e8906493373faf800ca37b78c589f38324',
             'suites':['intake/dev-repair-v1/suite.native.dev.json','intake/dev-repair-v1/suite.compat.dev.json'],
             'relations':'relations.dev-repair-v1.json', 'expected_native':70, 'expected_compat':59},
 'requests': {'author':'e49f9ada5826b4206d2128f3bfb8d31603ff42fa', 'review':'3aae8bf2a690213426af91427dc837fedbe50c83',
              'prior_review':'c2026ebad0b6037f1c30b7d2b34055e211849b70',
              'suites':['suite-native-dev.json','suite-compat-dev.json'], 'relations':'relations.json',
              'expected_native':91, 'expected_compat':83}}
GIN_SPEC = {'author':'949a9471f552d496e85b27c460c7b76536f13803',
            'review':'5aafcda4a5098c8407d9373eafc3185e59932550',
            'source_review':'ba79bcfc3f2bcc622297ef53b5de37ce205a5c89',
            'suites':['suite-native-dev.json','suite-compat-dev.json'],
            'relations':'relations.json', 'expected_native':67, 'expected_compat':55}


def sha(raw):return hashlib.sha256(raw).hexdigest()

def canon(obj):return json.dumps(obj,sort_keys=True,separators=(',',':'),ensure_ascii=False).encode()


def load_blob(commit, path, proofs):
    raw = subprocess.check_output(['git','show',commit+':'+path],cwd=REPO,stderr=subprocess.DEVNULL)
    proofs[commit+':'+path] = sha(raw)
    return raw


def components(rows, edges):
    parent={q['query_family']:q['query_family'] for q in rows}
    def find(x):
        while parent[x]!=x:
            parent[x]=parent[parent[x]];x=parent[x]
        return x
    def join(a,b):
        a,b=find(a),find(b)
        if a!=b:parent[max(a,b)]=min(a,b)
    known=set(parent)
    for edge in edges:
        members=[m for m in edge if m in known]
        for m in members[1:]:join(members[0],m)
    result={}
    for m in sorted(parent):result.setdefault(find(m),[]).append(m)
    return [{'global_component':k,'members':v} for k,v in sorted(result.items())]


def audit(evaluator, output, evaluator_build_receipt=None, include_gin=False):
    specs={**SPECS, **({'gin':GIN_SPEC} if include_gin else {})}
    errors=Counter();proofs={};results={};all_rows=[];local_edges=[];all_sources={}
    receipt_path=evaluator_build_receipt or REPO/'artifacts/checkpoints/v19-corpus-audit-20261003/validation.json'
    build_receipt=json.loads(Path(receipt_path).read_text())
    source=build_receipt['source_sha']
    if len(source)!=40 or any(c not in '0123456789abcdef' for c in source):raise ValueError('EVALUATOR_SOURCE_SHA')
    if (build_receipt['build_exit_code']!=0 or sha(Path(evaluator).read_bytes())!=build_receipt['binary_sha256']
            or build_receipt['compiler_artifact']['target']['name']!='cc-eval'):errors['EVALUATOR_BUILD_BINDING']+=1
    if subprocess.run(['git','diff','--quiet','83a6b54ab1e71db033264e3b4e8d4f0a1d5319ad',source,'--','crates/cc-eval/src','crates/cc-eval/Cargo.toml','Cargo.lock'],cwd=REPO).returncode:errors['EVALUATOR_SOURCE_COMPATIBILITY']+=1
    rules=json.loads(DECISIONS.read_text())
    if include_gin:
        delta=json.loads((HERE/'gin-extension/cross-repo-decisions.json').read_text())
        rules['native_query_file_sha256'].update(delta['native_query_file_sha256'])
        rules['pair_decisions'].extend(delta['pair_decisions'])
        rules['conservative_leakage_edges'].extend(delta['conservative_leakage_edges'])
    locked={x['repository'].split('/')[-1].lower():x for x in json.loads((HERE.parent/'source-locks.json').read_text())['candidates']}
    with tempfile.TemporaryDirectory(prefix='v19-dev-admission-') as td:
        temp=Path(td)
        for name,spec in specs.items():
            prefix='crates/cc-eval/benchmarks/public-v19/'+name+'/'
            get=lambda path:load_blob(spec['author'],prefix+path,proofs)
            native=[];native_raw=b'';suite_paths=[];source_bytes={};suites=[]
            for profile,sp in zip(['native','compat'],spec['suites']):
                raw=get(sp);suite=json.loads(raw);suites.append(suite)
                target=temp/name/sp;target.parent.mkdir(parents=True,exist_ok=True);target.write_bytes(raw);suite_paths.append(target)
                qp=(Path(sp).parent/suite['queries']).as_posix()
                expected_query=(Path(sp).parent/('queries.'+profile+'.dev.jsonl')).as_posix()
                if qp!=expected_query:raise ValueError('QUERY_OUTSIDE_CURRENT_DEV_ALLOWLIST')
                qraw=get(qp);qtarget=temp/name/qp;qtarget.parent.mkdir(parents=True,exist_ok=True);qtarget.write_bytes(qraw)
                rows=[json.loads(l) for l in qraw.splitlines() if l.strip()]
                if any(q['split']!='dev' for q in rows):errors['NON_DEV_INTAKE']+=1
                if len(rows)!=spec['expected_'+profile]:errors['UNEXPECTED_ROWS']+=1
                if profile=='native':native=rows;native_raw=qraw
                src=(target.parent/suite['source']['root']).resolve()
                if not src.is_relative_to(temp):raise ValueError('SOURCE_ESCAPE')
                for path in suite['source']['files']:
                    raw=get('source/'+path);source_bytes[path]=raw;dest=src/path;dest.parent.mkdir(parents=True,exist_ok=True);dest.write_bytes(raw)
            if suites[0]['source']!=suites[1]['source']:errors['SOURCE_PROFILE_DRIFT']+=1
            manifest=json.loads(get('source-manifest.json'))
            records=manifest.get('files',manifest.get('admitted',[]));by_path={r['path']:r for r in records}
            upstream=manifest.get('upstream_sha',manifest.get('source_sha'))
            if upstream!=locked[name]['source_sha']:errors['UPSTREAM_LOCK_DRIFT']+=1
            for path,raw in source_bytes.items():
                r=by_path.get(path,{})
                if r.get('sha256')!=sha(raw) or r.get('bytes')!=len(raw):errors['SOURCE_BYTES_DRIFT']+=1
                if r.get('git_blob') and r['git_blob']!=hashlib.sha1(b'blob '+str(len(raw)).encode()+b'\0'+raw).hexdigest():errors['GIT_BLOB_DRIFT']+=1
            license_raw=get('license/LICENSE')
            if sha(license_raw)!=locked[name]['license_files'][0]['sha256']:errors['ROOT_LICENSE_DRIFT']+=1
            spans=0
            for q in native:
                for g in q['answers']:
                    for alt in g['alternatives']:
                        raw=source_bytes.get(alt['path']);s=alt.get('span')
                        if raw is None:errors['GOLD_SOURCE_OUTSIDE_DOMAIN']+=1
                        elif s:
                            start,end=s['start'],s['end']
                            if not 0<=start<end<=len(raw):errors['SPAN_BOUNDS']+=1
                            else:
                                try:raw[:start].decode();raw[start:end].decode();spans+=1
                                except UnicodeDecodeError:errors['SPAN_UTF8']+=1
            relraw=get(spec['relations']);rel=json.loads(relraw);relfile=temp/name/'relations.json';relfile.write_bytes(relraw)
            local_edges.extend(c['members'] for c in rel['components'])
            local_count=len(components(native,[c['members'] for c in rel['components']]))
            reviewer=None;content_verified=0;license_aux=0
            rp='crates/cc-eval/benchmarks/public-v19/reviews/'+name+'/'
            if name=='express':
                rev=json.loads(load_blob(spec['review'],rp+'repair-v1/dev-008-repair-review.json',proofs));reviewer=rev['reviewer_id']
                if rev['author_current_sha']!=spec['author'] or rev['native_dev_file_sha256']!=sha(native_raw):errors['REVIEW_BINDING']+=1
                compat_raw=(temp/name/Path(spec['suites'][1]).parent/suites[1]['queries']).read_bytes()
                if rev['compat_dev_file_sha256']!=sha(compat_raw) or rev['license_sha256']!=sha(license_raw):errors['REVIEW_BINDING']+=1
                content_verified=rev['counts']['combined_content_accepted_native_rows']
                if content_verified!=len(native) or rev['counts']['repaired_needschange'] or set(rev['scope_error_codes'])-{'G_CROSS_REPOSITORY_AND_BLOCKED_COMPONENTS_UNREVIEWED','H_CUSTODY_BLOCKED_32','G_TOTAL_99_IS_AUTHOR_CANDIDATE_COUNT_NOT_GLOBAL_CERTIFICATION'}:errors['CONTENT_REVIEW_INCOMPLETE']+=1
                if reviewer==rev['author_id']:errors['SELF_REVIEW']+=1
            elif name=='gin':
                rev=json.loads(load_blob(spec['review'],rp+'repair-v2/dev-004-repair-review.json',proofs));reviewer=rev['reviewer_id']
                compat_raw=(temp/name/suites[1]['queries']).read_bytes()
                if rev['author_current_sha']!=spec['author'] or rev['native_dev_file_sha256']!=sha(native_raw) or rev['compat_dev_file_sha256']!=sha(compat_raw):errors['REVIEW_BINDING']+=1
                content_verified=rev['counts']['combined_content_accepted_native_rows']
                allowed={'G_CROSS_REPOSITORY_AND_BLOCKED_COMPONENTS_UNREVIEWED','H_CUSTODY_BLOCKED_33','G_TOTAL_99_IS_AUTHOR_CANDIDATE_COUNT_NOT_GLOBAL_CERTIFICATION'}
                if (content_verified!=len(native) or rev['counts']['repaired_accept']!=4 or rev['counts']['unchanged_prior_accepted_native_rows']!=63
                        or rev['counts']['repaired_needschange'] or set(rev['scope_error_codes'])-allowed):errors['CONTENT_REVIEW_INCOMPLETE']+=1
                expected_rows={r['family_id_sha256']:r['native_row_sha256'] for r in rev['retained_prior_acceptance']}
                expected_rows.update({r['family_id_sha256']:r['new_native_row_sha256'] for r in rev['rows'] if r['decision']=='accept' and not r['error_codes']})
                actual_rows={sha(q['query_family'].encode()):sha(line) for q,line in zip(native,native_raw.splitlines(keepends=True))}
                if expected_rows!=actual_rows:errors['GIN_ROW_REVIEW_BINDING']+=1
                compat_rows={sha(json.loads(line)['query_family'].encode()):sha(line) for line in compat_raw.splitlines(keepends=True)}
                if any(compat_rows.get(r['family_id_sha256'])!=r['new_compat_row_sha256'] for r in rev['rows']):errors['GIN_COMPAT_REVIEW_BINDING']+=1
                original_raw=load_blob(spec['source_review'],rp+'dev-067-review.json',proofs)
                original=json.loads(original_raw)
                if reviewer!=original['reviewer_id'] or reviewer==original['author_id']:errors['SELF_REVIEW']+=1
                if sha(original_raw)!=rev['previous_review_receipt_sha256']:errors['GIN_PRIOR_REVIEW_BINDING']+=1
                old_accept={r['family_id_sha256']:r['row_sha256'] for r in original['rows'] if r['decision']=='accept'}
                if old_accept!={r['family_id_sha256']:r['native_row_sha256'] for r in rev['retained_prior_acceptance']}:errors['GIN_PRIOR_ACCEPT_BINDING']+=1
                proof_raw=load_blob(spec['source_review'],rp+'provenance/upstream-admission.json',proofs);proof=json.loads(proof_raw)
                if rev['source_admission_review_sha']!=spec['source_review'] or sha(proof_raw)!=rev['source_admission_receipt_sha256']:errors['GIN_SOURCE_REVIEW_BINDING']+=1
                if proof['source_lock_sha256']!=sha(get('provenance/source-lock.json')):errors['GIN_SOURCE_LOCK_BINDING']+=1
                if (proof['decision']!='accept_provenance_and_license_admission_for_53_admitted_files' or proof['source_sha']!=upstream
                        or not proof['upstream_commit_object_sha1_verified'] or not proof['upstream_tree_inventory_matches_manifest']
                        or proof['permission_rejections'] or proof['source_manifest_sha256']!=sha(get('source-manifest.json'))):errors['GIN_UPSTREAM_ADMISSION']+=1
                if (proof['license']['sha256']!=sha(license_raw) or not proof['license']['snapshot_matches_upstream']):errors['GIN_MIT_REVIEW_BINDING']+=1
                inventory={r['path_sha256']:r for r in proof['inventory']}
                if len(inventory)!=53 or len(source_bytes)!=53 or set(inventory)!={sha(p.encode()) for p in source_bytes}:errors['GIN_SOURCE_INVENTORY']+=1
                for path,raw in source_bytes.items():
                    item=inventory.get(sha(path.encode()),{})
                    if (item.get('sha256')!=sha(raw) or item.get('bytes')!=len(raw) or item.get('mit_notice')!='verified'
                            or item.get('upstream_and_snapshot_git_blob')!=hashlib.sha1(b'blob '+str(len(raw)).encode()+b'\0'+raw).hexdigest()):errors['GIN_UPSTREAM_BYTES_DRIFT']+=1
                for q in native:
                    for e in q['annotations']['v19'].get('gold_evidence',[]):
                        raw=source_bytes.get(e['path']);s=e['span']
                        if (raw is None or e['source_sha']!=upstream or not 0<=s['start']<s['end']<=len(raw)
                                or sha(raw[s['start']:s['end']])!=e['sha256']):errors['GIN_SOURCE_EVIDENCE_DRIFT']+=1
                if local_count!=66 or rev['counts']['visible_dev_local_components']!=local_count:errors['GIN_LOCAL_COMPONENT_BINDING']+=1
            else:
                for prior_file in ['dev20-review.json','block02-review.json','block03-review.json','block04-review.json']:
                    if load_blob(spec['prior_review'],rp+prior_file,proofs)!=load_blob(spec['review'],rp+prior_file,proofs):errors['REVIEW_HISTORY_BINDING']+=1
                gold=json.loads(get('gold/dev.json'));gold={r['family_id']:r for r in gold};matched=set()
                for f in ['dev20-review.json','block02-review.json','block03-review.json','block04-review.json','delta-dev-review.json']:
                    rev=json.loads(load_blob(spec['review'],rp+f,proofs))
                    for r in rev.get('rows',[]):
                        q=next((q for q in native if q['id']==r['query_id']),None)
                        if q and r['decision']=='accept' and r['question_sha256']==sha(canon(q)) and r['gold_record_sha256']==sha(canon(gold[q['query_family']])):
                            if r['author_id']==r['reviewer_id']:errors['SELF_REVIEW']+=1
                            matched.add(q['id']);reviewer=r['reviewer_id']
                content_verified=len(matched)
                if content_verified!=len(native):errors['CONTENT_REVIEW_INCOMPLETE']+=1
                lic=json.loads(load_blob(spec['review'],rp+'delta-license-review.json',proofs))
                if lic['author_sha']!=spec['author'] or lic['independent_license_evidence']!='accept_with_full_BSD_notice_retention':errors['BSD_LICENSE_REVIEW']+=1
                for path,expected in lic['read_allowlist_sha256'].items():
                    if not path.startswith(('license/','source/')):continue
                    if sha(get(path))!=expected:errors['BSD_RETAINED_EVIDENCE_DRIFT']+=1
                    license_aux+=1
                if len(lic['normalized_helper_lineage'])!=3 or not all(r['three_way_equal'] for r in lic['normalized_helper_lineage']):errors['BSD_HELPER_LINEAGE']+=1
                # All helper bytes may be indexed with retained BSD notices; helper-specific gold stays excluded.
                inclusion=json.loads(get('license/inclusion-review.json'))
                if any(h.get('source_admission')!='verified_BSD_lineage_full_notices_retained' or h['gold_admission'] for h in inclusion['adapted_third_party_helpers']):errors['HELPER_ADMISSION_DOMAIN']+=1
                for q in native:
                    for alt in [a for g in q['answers'] for a in g['alternatives']]:
                        for h in inclusion['adapted_third_party_helpers']:
                            span=alt.get('span')
                            if alt['path']==h['path'] and span and max(span['start'],h['start_byte'])<min(span['end'],h['end_byte']):errors['HELPER_GOLD_NOT_ADMITTED']+=1
            validations=[]
            for suite_path in suite_paths:
                proc=subprocess.run([str(evaluator),'validate','--suite',str(suite_path)],capture_output=True)
                validations.append({'suite_sha256':sha(suite_path.read_bytes()),'exit_code':proc.returncode,'diagnostics_sha256':sha(proc.stdout+proc.stderr)})
                if proc.returncode:errors['EVALUATOR_VALIDATION']+=1
            # Run own read-only protocol checker in the temporary allowlisted layout.
            countfile=temp/name/'protocol.json'
            args=['python3',str(HERE.parent/'check.py'),'--shard',name+':native='+str(suite_paths[0].parent/suites[0]['queries']),
                  '--shard',name+':compat='+str(suite_paths[1].parent/suites[1]['queries']),'--relations',str(relfile),'--output',str(countfile)]
            proc=subprocess.run(args,capture_output=True);protocol=json.loads(countfile.read_bytes())
            if proc.returncode:errors.update({'PROTOCOL_'+k:v for k,v in protocol['errors'].items()})
            results[name]={'author_sha':spec['author'],'review_sha':spec['review'],'prior_review_sha':spec.get('prior_review'),'upstream_sha':upstream,'native_dev_rows':len(native),
                           'compat_dev_rows':spec['expected_compat'],'local_components':local_count,'current_content_hash_accept':content_verified,
                           'source_files_verified':len(source_bytes),'spans_verified':spans,'root_license_sha256':sha(license_raw),
                           'independent_BSD_retained_files_verified':license_aux,'reviewer_ref_sha256':sha(reviewer.encode()),
                           'snapshot_only_not_git_cleanliness':suites[0]['source']['commit'] is None,
                           'actual_evaluator_validations':validations,'protocol_errors':protocol['errors'],
                           'native_dev_suite_entry':spec['author']+':'+prefix+spec['suites'][0],
                           'compat_dev_suite_entry':spec['author']+':'+prefix+spec['suites'][1]}
            all_rows.extend(native)
            all_sources[name]=source_bytes
            if name=='gin':
                results[name]['source_review_sha']=spec['source_review']
                results[name]['source_admission_receipt_sha256']=sha(proof_raw)
    # Whole observed cross-repository query/fact audit; no retrieval score computation.
    repo_rows={name:[q for q in all_rows if q['annotations']['v19']['repo_id']==name] for name in specs}
    pairs=[(a,b) for left,right in combinations(repo_rows.values(),2) for a in left for b in right]
    norm=lambda s:' '.join(unicodedata.normalize('NFKC',s).casefold().split())
    identical=sum(norm(a['query'])==norm(b['query']) for a,b in pairs)
    if identical:errors['CROSS_REPO_IDENTICAL_QUERY_UNADJUDICATED']+=identical
    if rules['native_query_file_sha256']!={name:proofs[spec['author']+':crates/cc-eval/benchmarks/public-v19/'+name+'/'+(Path(spec['suites'][0]).parent/json.loads(load_blob(spec['author'],'crates/cc-eval/benchmarks/public-v19/'+name+'/'+spec['suites'][0],proofs))['queries']).as_posix()] for name,spec in specs.items()}:errors['GLOBAL_REVIEW_FILE_BINDING']+=1
    edges=[e['members'] for e in rules['conservative_leakage_edges']]
    dev_families={q['query_family'] for q in all_rows}
    if any(m not in dev_families for e in edges for m in e):errors['GLOBAL_EDGE_OUTSIDE_DEV']+=1
    if include_gin:
        by_family={q['query_family']:q for q in all_rows}
        for decision in rules['pair_decisions']:
            if any(m not in by_family for m in decision['members']):
                errors['GLOBAL_DECISION_OUTSIDE_DEV']+=1;continue
            if decision['question_record_sha256']!=[sha(canon(by_family[m])) for m in decision['members']]:errors['GLOBAL_DECISION_ROW_DRIFT']+=1
            for member,span_proofs in zip(decision['members'],decision.get('source_span_proofs',[])):
                name=by_family[member]['annotations']['v19']['repo_id']
                source_index={sha(path.encode()):raw for path,raw in all_sources[name].items()}
                for proof in span_proofs:
                    raw=source_index.get(proof['path_sha256']);start,end=proof['start'],proof['end']
                    if raw is None or not 0<=start<end<=len(raw) or sha(raw[start:end])!=proof['sha256']:errors['GLOBAL_DECISION_SOURCE_DRIFT']+=1
    registry=components(all_rows,local_edges+edges)
    result={'schema_version':1,'status':'development_admitted_snapshot_scope_only' if not errors else 'development_admission_blocked',
            'errors':dict(errors),'inputs_sha256':proofs,'repo_results':results,'global_review':{
                'current_native_dev_rows_read':len(all_rows),'cross_pairs_automatically_checked':len(pairs),
                'exact_normalized_cross_repo_duplicates':identical,'manual_pair_decisions':len(rules['pair_decisions']),
                'proven_cross_repo_task_equivalence_merges':0,'conservative_shared_fact_edges':len(edges),
                'local_components_before_cross_repo':sum(r['local_components'] for r in results.values()),
                'conservative_global_correlation_components':len(registry),'registry_sha256':sha(canon(registry)),
                'full_semantic_independence_proved':False},'development_admitted_native_rows':len(all_rows) if not errors else 0,
            'formal_600_accepted_families':0,'clean_holdout':0,'formal_complete20_blocks':0,
            'ranking_runs':0,'live_provider_calls':0,'protected_body_reads':0,'gold_changes':0,
            'remaining':['custody_blocked','heldout_cleanliness','Gin_TypeScript_not_reviewed_here','six_repo_target','formal_facet_graph_statistics'],
            'evaluator_binary_sha256':sha(Path(evaluator).read_bytes()),'evaluator_build_source_sha':build_receipt['source_sha'],
            'evaluator_build_receipt_sha256':sha(Path(receipt_path).read_bytes())}
    if include_gin:
        result['remaining']=['custody_blocked','heldout_cleanliness','TypeScript_not_admitted_here','six_repo_target','formal_facet_graph_statistics']
    output.mkdir(parents=True,exist_ok=True)
    (output/'admission.json').write_text(json.dumps(result,indent=2)+'\n')
    (output/'global-components.json').write_text(json.dumps({'scope':'current_public_dev_conservative_correlation_not_confirmatory','components':registry},indent=2)+'\n')
    return result


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--evaluator',type=Path,required=True);p.add_argument('--evaluator-build-receipt',type=Path);p.add_argument('--output',type=Path,required=True);p.add_argument('--include-gin',action='store_true');args=p.parse_args()
    try:r=audit(args.evaluator.resolve(),args.output,args.evaluator_build_receipt,args.include_gin)
    except (OSError,subprocess.SubprocessError,KeyError,ValueError,TypeError):
        r={'status':'development_admission_blocked','errors':{'INPUT_OR_EVALUATOR_BLOCKER':1}}
        args.output.mkdir(parents=True,exist_ok=True);(args.output/'admission.json').write_text(json.dumps(r,indent=2)+'\n')
    print(json.dumps({'status':r['status'],'errors':r['errors'],'receipt_sha256':sha((args.output/'admission.json').read_bytes())}))
    return bool(r['errors'])


if __name__=='__main__':raise SystemExit(main())
