#!/usr/bin/env python3
"""Synthetic metadata controls only; never creates a publication proof."""
import copy, importlib.util, json, subprocess, sys
from pathlib import Path
sys.dont_write_bytecode=True
HERE=Path(__file__).resolve().parent
sp=importlib.util.spec_from_file_location('binding',HERE/'prepare_publication_binding.py');m=importlib.util.module_from_spec(sp);sp.loader.exec_module(m)
results=[]
def check(name,fn,accept):
    try:fn();actual=True;detail=None
    except AssertionError as e:actual=False;detail=str(e)
    assert actual==accept,(name,actual,accept)
    results.append({'name':name,'expected_accept':accept,'actual_accept':actual,'detail':detail})
def tree(entries):
    entries=sorted(entries,key=lambda e:(e['path']+('/' if e['type']=='tree' else '')).encode())
    raw=b''.join(str(int(e['mode'])).encode()+b' '+e['path'].encode()+b'\0'+bytes.fromhex(e['sha']) for e in entries)
    # Independent Git implementation, no -w or repository mutation.
    got=subprocess.run(['git','hash-object','-t','tree','--stdin'],input=raw,stdout=subprocess.PIPE,stderr=subprocess.PIPE,check=True).stdout.decode().strip()
    assert got==m.oid('tree',raw)
    return {'sha':got,'tree':entries,'truncated':False}
def blob(name,b=b'fixture'):
    return {'path':name,'mode':'100644','type':'blob','sha':m.oid('blob',b)}
leaf=tree([blob('part-000'),blob('part-001',b'other')]);root=tree([blob('a.c'),{'path':'a','mode':'040000','type':'tree','sha':leaf['sha']}])
check('complete_git_tree_with_directory_sort',lambda:m.verified_tree(root),True)
bad=copy.deepcopy(root);bad['tree'][0]['sha']='f'*40
check('changed_oid_without_changed_tree_rejected',lambda:m.verified_tree(bad),False)
bad=copy.deepcopy(root);bad['truncated']=True
check('truncated_capture_rejected',lambda:m.verified_tree(bad),False)
bad=copy.deepcopy(root);bad['tree'].append(bad['tree'][0])
check('duplicate_entry_rejected',lambda:m.verified_tree(bad),False)
bad=copy.deepcopy(root);bad['tree'][0]['mode']='100755'
check('changed_mode_rejected',lambda:m.verified_tree(bad),False)
trees={root['sha']:m.verified_tree(root),leaf['sha']:m.verified_tree(leaf)}
check('existing_path_exact_oid',lambda:exec("assert m.resolve(trees,root['sha'],'a/part-001')[0]['sha']==leaf['tree'][1]['sha']",globals()),True)
check('missing_part_rejected',lambda:m.resolve(trees,root['sha'],'a/part-002'),False)
check('missing_ancestor_capture_rejected',lambda:m.resolve({root['sha']:trees[root['sha']]},root['sha'],'a/part-000'),False)
check('parent_traversal_rejected',lambda:m.resolve(trees,root['sha'],'a/../part-000'),False)
recursive={'sha':root['sha'],'tree':root['tree']+[{**e,'path':'a/'+e['path']} for e in leaf['tree']],'truncated':False}
check('complete_recursive_response_reconstructs_children',lambda:exec("assert len(m.expand_recursive(recursive))==2",globals()),True)
bad=copy.deepcopy(recursive);bad['tree']=bad['tree'][:-1]
check('recursive_missing_part_rejected_by_child_oid',lambda:m.expand_recursive(bad),False)
C='a'*40;T='b'*40
ref={'ref':'refs/heads/task/p8-evidence-checkpoint-a217-20261008','url':'https://api.github.com/repos/jyqj/codecortex/git/refs/heads/task/x','object':{'type':'commit','sha':C}}
pack={'commit':{'sha':C,'tree':{'sha':T},'url':'https://api.github.com/repos/jyqj/codecortex/git/commits/'+C},'ref':ref}
pub={'commit':C,'tree':T,'actual_ref_readback':ref,'force':False}
check('exact_commit_ref_root_binding',lambda:m.validate_identity(pack,pub,C,T),True)
bad=copy.deepcopy(pack);bad['ref']['object']['sha']='c'*40
check('ref_mismatch_rejected',lambda:m.validate_identity(bad,pub,C,T),False)
bad=copy.deepcopy(pack);bad['commit']['tree']['sha']='c'*40
check('root_mismatch_rejected',lambda:m.validate_identity(bad,pub,C,T),False)
bad=copy.deepcopy(pub);bad['force']=True
check('non_normal_update_rejected',lambda:m.validate_identity(pack,bad,C,T),False)
bad=copy.deepcopy(pack);bad['ref']['ref']='refs/heads/main'
check('wrong_branch_rejected',lambda:m.validate_identity(bad,pub,C,T),False)
out={'schema':'synthetic-publication-binding-metadata-controls-v1','helper':m.metadata(HERE/'prepare_publication_binding.py'),'control_source':m.metadata(__file__),'cases':results,'passed':len(results),'native_or_workload_or_original_zip_replay':False,'publication_proof_created':False}
print(json.dumps(out,sort_keys=True,indent=2))
