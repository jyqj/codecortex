#!/usr/bin/env python3
"""Creation-identity metadata controls; no native or CURRENT generation."""
import copy,importlib.util,json,sys
from pathlib import Path
sys.dont_write_bytecode=True
B=Path(__file__).resolve().parent
sp=importlib.util.spec_from_file_location('currentv2',B/'prepare_current_delivery_v2.py');m=importlib.util.module_from_spec(sp);sp.loader.exec_module(m)
pack=m.m.read_json(B/'actual-8aeb66-official-delivery-tree-pack.json')
pub=m.m.read_json(B.parent/'main-delivery-branch-published.json')
head=pub['commit'];root=pub['tree'];branch=pub['branch'];cases=[]
def check(name,p,q,accept,branch_arg=branch):
 try:m.validate_creation(p,q,head,root,branch_arg);actual=True;error=None
 except AssertionError as e:actual=False;error=str(e)
 assert actual==accept,(name,actual,accept)
 cases.append({'name':name,'expected_accept':accept,'actual_accept':actual,'error':error})
check('actual_create_raw_ref_and_absence_binding',pack,pub,True)
check('wrong_explicit_branch',pack,pub,False,'task/not-authorized')
p=copy.deepcopy(pack);p['ref']['object']['sha']='a'*40
check('fresh_ref_wrong_head',p,pub,False)
p=copy.deepcopy(pack);p['commit']['tree']['sha']='b'*40
check('actual_commit_wrong_root',p,pub,False)
q=copy.deepcopy(pub);q['operation']='update_ref'
check('update_ref_not_create',pack,q,False)
q=copy.deepcopy(pub);q['request']['sha']='c'*40
check('create_request_wrong_head',pack,q,False)
p=copy.deepcopy(pack);q=copy.deepcopy(pub);p['raw_create_branch_result']['isError']=True;q['actual_create_branch_result']=p['raw_create_branch_result']
check('create_tool_error',p,q,False)
p=copy.deepcopy(pack);p['raw_prior_ref_absence']['structuredContent']['error_data']['status']='403'
check('forbidden_is_not_absent',p,pub,False)
q=copy.deepcopy(pub);q['prior_ref_absence']['sha256']='0'*64
check('absence_capture_hash_mismatch',pack,q,False)
print(json.dumps({'schema':'current-creation-identity-metadata-controls-v1','helper':m.m.metadata(B/'prepare_current_delivery_v2.py'),'source':m.m.metadata(__file__),'cases':cases,'passed':len(cases),'current_generated':False,'native_or_original_zip_reads':0},indent=2,sort_keys=True))
