#!/usr/bin/env python3
"""Prepare exact upload metadata, or bind original raw to an observed published tree.

This tool reads small prepared files and saved official Git metadata only. It never
reads original ZIP payloads, executes native tools, edits repositories or publishes.
"""
import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath

WS = Path('/workspace/scratch/a217aaae3bde')
BASE = WS/'checkpoint-round19-prep/benchmark-runtime-delivery'
INDEX = BASE/'prepared-file-index-v2.json'
INDEX_SHA = 'cf23009411f87ad31a160832cc852c1276d4b51a522a5a7b1fc7b1276e041b20'
E = 'a23bb72d3c954f385b99fe81ce9189885c208557'
REPO = 'jyqj/codecortex'
PEER = Path('/dev/shm/a217aaae3bde/platform-review/round19-benchmark-runtime-delivery-independent-review.json')
PEER_SHA = '3121571864fc5010497314fa283fa9cedac49786b12f361a674a586ca51b8b80'
PREFIX = 'artifacts/benchmarks/37871838957'
MODE_TYPE = {'040000':'tree','100644':'blob','100755':'blob','120000':'blob','160000':'commit'}

def sha256(b): return hashlib.sha256(b).hexdigest()
def oid(kind,b): return hashlib.sha1(kind.encode()+b' '+str(len(b)).encode()+b'\0'+b).hexdigest()
def metadata(p):
    p=Path(p);b=p.read_bytes()
    return {'path':str(p),'bytes':len(b),'sha256':sha256(b),'git_blob_sha1':oid('blob',b)}
def read_json(p): return json.loads(Path(p).read_bytes())
def dump_new(p,value):
    p=Path(p);p.parent.mkdir(parents=True,exist_ok=True)
    with p.open('xb') as f:f.write((json.dumps(value,ensure_ascii=False,sort_keys=True,indent=2)+'\n').encode())
    return metadata(p)
def valid_oid(v):
    return isinstance(v,str) and len(v)==40 and all(c in '0123456789abcdef' for c in v)
def safe_relative(path):
    p=PurePosixPath(path)
    assert isinstance(path,str) and path and not p.is_absolute()
    assert str(p)==path and all(c not in ('','.', '..') for c in path.split('/'))
    assert '\0' not in path and '\\' not in path
    return p

def verified_tree(obj):
    """Reconstruct a complete nonrecursive Git tree; never trust a flat path map."""
    assert valid_oid(obj['sha']) and obj.get('truncated') is False
    entries=obj['tree']; assert isinstance(entries,list)
    names=set(); normalized=[]
    for ent in entries:
        path=ent['path'];safe_relative(path);assert '/' not in path
        assert path not in names;names.add(path)
        mode=ent['mode'];kind=ent['type'];assert MODE_TYPE.get(mode)==kind
        assert valid_oid(ent['sha'])
        normalized.append((path,mode,kind,ent['sha']))
    # Git compares a tree directory as its name followed by slash.
    normalized.sort(key=lambda x:(x[0]+('/' if x[2]=='tree' else '')).encode())
    raw=b''.join(str(int(mode)).encode()+b' '+name.encode()+b'\0'+bytes.fromhex(sha)
                 for name,mode,kind,sha in normalized)
    assert oid('tree',raw)==obj['sha'], 'Git tree contents do not match reported SHA'
    return {name:{'mode':mode,'type':kind,'sha':sha} for name,mode,kind,sha in normalized}

def expand_recursive(obj):
    """Reconstruct every subtree from one complete official recursive response.

    Child views are derived from the captured response, not claimed as separate GETs.
    """
    assert valid_oid(obj['sha']) and obj.get('truncated') is False
    tree_oids={'':obj['sha']}; children={}; seen=set()
    for ent in obj['tree']:
        path=ent['path'];safe_relative(path);assert path not in seen;seen.add(path)
        assert MODE_TYPE.get(ent['mode'])==ent['type'] and valid_oid(ent['sha'])
        if ent['type']=='tree': tree_oids[path]=ent['sha']
        parent,_,name=path.rpartition('/')
        children.setdefault(parent,[]).append({**ent,'path':name})
    assert set(children)<=set(tree_oids), 'Recursive response is missing a parent tree'
    result=[]
    for path,sha in sorted(tree_oids.items()):
        derived={'sha':sha,'tree':children.get(path,[]),'truncated':False}
        result.append((path,derived,verified_tree(derived)))
    return result

def validate_identity(pack,pub,expected_commit,expected_root):
    assert valid_oid(expected_commit) and valid_oid(expected_root)
    commit=pack['commit'];ref=pack['ref']
    assert commit['sha']==expected_commit and commit['tree']['sha']==expected_root
    assert ref['object']['type']=='commit' and ref['object']['sha']==expected_commit
    assert pub['commit']==expected_commit and pub['tree']==expected_root
    assert pub['actual_ref_readback']['object']==ref['object']
    assert pub.get('force') is False, 'Require recorded normal ref update'
    assert ref['ref']=='refs/heads/task/p8-evidence-checkpoint-a217-20261008'
    assert ref['url'].startswith('https://api.github.com/repos/'+REPO+'/git/refs/')
    assert commit['url']=='https://api.github.com/repos/'+REPO+'/git/commits/'+expected_commit
    return commit,ref

def resolve(trees,root,path):
    components=safe_relative(path).parts;current=root
    traversed=[]
    for i,name in enumerate(components):
        assert current in trees, 'Missing complete ancestor tree: '+current
        entry=trees[current].get(name);assert entry is not None,'Missing published path: '+path
        traversed.append({'tree_sha':current,'name':name,**entry})
        if i<len(components)-1:
            assert entry['type']=='tree' and entry['mode']=='040000'
            current=entry['sha']
    return entry,traversed

def frozen_inputs():
    assert metadata(INDEX)['sha256']==INDEX_SHA
    assert metadata(PEER)['sha256']==PEER_SHA
    index=read_json(INDEX);assert index['file_count']==74 and len(index['files'])==74
    records=index['files'];seen=set()
    for row in records:
        safe_relative(row['repository_path']);assert row['repository_path'] not in seen
        seen.add(row['repository_path']);actual=metadata(row['local_path'])
        assert all(actual[k]==row[k] for k in ['bytes','sha256','git_blob_sha1'])
        assert row['git_mode']=='100644'
    manifest_path=Path(index['manifest']['path']);manifest=read_json(manifest_path)
    assert manifest['execution_head']==E and len(manifest['artifacts'])==7
    assert len(manifest['original_members'])==55
    expected={};artifacts=[]
    for a in manifest['artifacts']:
        pos=0
        for number,part in enumerate(a['parts']):
            assert part['ordinal']==number and part['offset_start']==pos
            assert part['offset_end_exclusive']==pos+part['bytes']
            assert 0<part['bytes']<=4*1024*1024
            assert part['mode']=='100644' and part['type']=='blob'
            path=part['repository_path'];safe_relative(path);assert path not in expected
            expected[path]={'role':'original_zip_part','artifact_id':a['original_artifact_id'],
                'mode':'100644','type':'blob','sha':part['git_blob_sha1'],'bytes':part['bytes'],
                'sha256':part['sha256'],'ordinal':number,'offset_start':pos,'offset_end_exclusive':part['offset_end_exclusive']}
            pos=part['offset_end_exclusive']
        assert pos==a['original_zip']['size']
        artifacts.append({'artifact_id':a['original_artifact_id'],'label':a['label'],
            'run_id':a['run_id'],'whole_zip':a['original_zip'],
            'parts':[part['repository_path'] for part in a['parts']]})
    assert len(expected)==44
    locator_path=Path(index['additional_locator']['path']);locators=read_json(locator_path)
    for row in locators['artifacts']:
        path=row['manifest_repository_path'];safe_relative(path)
        val={'role':'original_zip_manifest','mode':'100644','type':'blob',
             'sha':row['manifest_git_blob_sha1'],'bytes':row['manifest_bytes'],'sha256':row['manifest_sha256']}
        assert path not in expected or expected[path]==val;expected[path]=val
    assert len(expected)==46
    return index,records,manifest,artifacts,expected

def upload_rows(records):
    return [{'ordinal':i,'repository_path':r['repository_path'],'source_path':r['local_path'],
             'type':'blob','mode':r['git_mode'],'git_blob_sha1':r['git_blob_sha1'],
             'bytes':r['bytes'],'sha256':r['sha256']} for i,r in enumerate(records)]

def prepare(output):
    index,records,manifest,artifacts,expected=frozen_inputs()
    output=Path(output);output.mkdir(parents=True,exist_ok=True)
    plan={'schema':'benchmark-74-exact-local-upload-plan-v1','execution_source':E,
          'frozen_index':metadata(INDEX),'independent_original_member_review':metadata(PEER),
          'file_count':74,'total_bytes':sum(r['bytes'] for r in records),'files':upload_rows(records),
          'publication_status':'not_claimed_by_upload_plan','complete_deliverables_ready':False,
          'future_append_only_binding_path':PREFIX+'/publication-bindings/<ACTUAL_PUBLISHED_COMMIT>/original-raw-publication.json',
          'future_binding_has_no_fake_blob_or_placeholder':True,'formal_task_completion':False}
    req={'schema':'required-published-original-raw-paths-v1','execution_source':E,
         'required_artifact_count':7,'required_part_count':44,'required_manifest_count':2,
         'expected':expected,'artifacts':artifacts,'complete_original_raw_publication':False,
         'state':'awaiting_actual_commit_tree_and_ref_metadata',
         'official_tree_pack_schema':{'commit':'Saved official GET /git/commits/<sha> object',
           'ref':'Saved official GET /git/ref/heads/<evidence branch> readback object',
           'trees':'List of complete nonrecursive official GET /git/trees/<sha> objects; include root and every ancestor of46requiredpaths; all objects reconstructed byGitSHA1'},
         'no_execution_or_remote_writes':True,'formal_task_completion':False}
    return [dump_new(output/'benchmark-74-upload-plan.json',plan),dump_new(output/'required-publication-inputs.json',req)]

def bind(pack_path,publication_path,expected_commit,expected_root,output):
    assert valid_oid(expected_commit) and valid_oid(expected_root)
    index,records,manifest,artifacts,expected=frozen_inputs()
    pack=read_json(pack_path);pub=read_json(publication_path)
    commit,ref=validate_identity(pack,pub,expected_commit,expected_root)
    trees={};tree_records=[]
    for obj in pack['trees']:
        assert obj['sha'] not in trees,'Duplicate tree object'
        assert obj['url']=='https://api.github.com/repos/'+REPO+'/git/trees/'+obj['sha']
        trees[obj['sha']]=verified_tree(obj)
        tree_records.append({'sha':obj['sha'],'entries':len(obj['tree']),'capture_kind':'official_nonrecursive'})
    for obj in pack.get('recursive_trees',[]):
        assert obj['url']=='https://api.github.com/repos/'+REPO+'/git/trees/'+obj['sha']
        for relative,derived,entries in expand_recursive(obj):
            sha=derived['sha']
            assert sha not in trees or trees[sha]==entries
            trees[sha]=entries
            tree_records.append({'sha':sha,'entries':len(entries),'capture_kind':'derived_from_complete_official_recursive_response',
                                 'official_recursive_root':obj['sha'],'relative_path':relative})
    bindings=[]
    for path,wanted in sorted(expected.items()):
        entry,chain=resolve(trees,expected_root,path)
        assert all(entry[k]==wanted[k] for k in ['mode','type','sha']),'Wrong published object: '+path
        bindings.append({'repository_path':path,**wanted,'actual_tree_chain':chain,
                         'public_url':'https://github.com/'+REPO+'/blob/'+expected_commit+'/'+path})
    assert len([r for r in bindings if r['role']=='original_zip_part'])==44
    output=Path(output);output.mkdir(parents=True,exist_ok=True)
    repository_path=PREFIX+'/publication-bindings/'+expected_commit+'/original-raw-publication.json'
    proof={'schema':'actual-published-original-runtime-lifecycle-raw-v1','execution_source':E,
           'actual_publication_commit':expected_commit,'actual_publication_tree':expected_root,
           'publication_receipt':metadata(publication_path),'saved_official_metadata_pack':metadata(pack_path),
           'official_objects':{'commit':commit,'ref':ref,'reconstructed_trees':tree_records},
           'frozen_delivery_index':metadata(INDEX),'original_member_peer_review':metadata(PEER),
           'required_parts':44,'actual_parts_bound':44,'required_manifests':2,'actual_manifests_bound':2,
           'complete_original_raw_publication':True,'original_artifacts':artifacts,'bindings':bindings,
           'whole_and_member_byte_proofs':'Reused unchanged original custody and peer-reviewed7ZIP/55member proofs pinned byfrozen delivery index; no largeZIP rehash or newnative run.',
           'benchmark_report_publication':'These74prepared benchmark files are a separate upload; thisreceipt doesnotcertify theirfuturepublication.',
           'complete_deliverables_ready':False,'old_pending_guard_preserved':index['publication_guard'],
           'restore':'Concatenate listedparts in ordinal order without added bytes; verify eachsize/SHA256/Gitblob andwhole officialZIP digest before extraction. Use original frozen replay recipe; no originalDB repair.',
           'remote_or_source_or_tasks_modified':False,'native_or_tests_rerun':0,
           'formal_task_completion':False,'counts':{'done':163,'remaining':29}}
    proof_file=output/'repository-files'/repository_path
    proof_meta=dump_new(proof_file,proof)
    combined=upload_rows(records)+[{'ordinal':74,'repository_path':repository_path,'source_path':str(proof_file),
        'type':'blob','mode':'100644','git_blob_sha1':proof_meta['git_blob_sha1'],'bytes':proof_meta['bytes'],'sha256':proof_meta['sha256']}]
    plan={'schema':'benchmark-75-exact-local-upload-plan-v1','execution_source':E,'original74_index':metadata(INDEX),
          'additional_raw_publication_proof':proof_meta,'file_count':75,'total_bytes':sum(r['bytes'] for r in combined),
          'files':combined,'all_original74_bytes_unchanged':True,'publication_status':'upload_pending_for_these_benchmark_files',
          'complete_deliverables_ready':False,'formal_task_completion':False}
    plan_meta=dump_new(output/'benchmark-75-upload-plan.json',plan)
    frozen_inputs() # bounded small-file recheck; never touches original ZIPs.
    return [proof_meta,plan_meta]

def main():
    ap=argparse.ArgumentParser();sub=ap.add_subparsers(dest='command',required=True)
    prep=sub.add_parser('prepare');prep.add_argument('--output',required=True)
    actual=sub.add_parser('bind');actual.add_argument('--official-tree-pack',required=True)
    actual.add_argument('--publication-receipt',required=True);actual.add_argument('--expected-commit',required=True)
    actual.add_argument('--expected-root-tree',required=True);actual.add_argument('--output',required=True)
    args=ap.parse_args()
    if args.command=='prepare':result=prepare(args.output)
    else:result=bind(args.official_tree_pack,args.publication_receipt,args.expected_commit,args.expected_root_tree,args.output)
    print(json.dumps(result,ensure_ascii=False))
if __name__=='__main__':main()
