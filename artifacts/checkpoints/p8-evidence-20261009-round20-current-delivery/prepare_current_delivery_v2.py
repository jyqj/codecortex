#!/usr/bin/env python3
"""Append scoped delivery status only after a published parent contains all inputs."""
import argparse,hashlib,importlib.util,json,sys
from pathlib import Path
sys.dont_write_bytecode=True
HERE=Path(__file__).resolve().parent
HELPER=HERE/'prepare_publication_binding.py'
assert hashlib.sha256(HELPER.read_bytes()).hexdigest()=='b362f5626ac518634fbcec4d329bebfb91be5bcb98fe4e90f58d619eb4e1b7aa'
sp=importlib.util.spec_from_file_location('frozen_binding',HELPER);m=importlib.util.module_from_spec(sp);sp.loader.exec_module(m)
PLAN=HERE/'bound-780296/benchmark-75-upload-plan.json'
PLAN_SHA='bbef7d7b5661c284caf4614c6b6dd244c2e4b679be0b268eff0d2a900e3822d2'
WORKSPACE=Path('/workspace/scratch/a217aaae3bde')

def actual_trees(pack):
    trees={};receipts=[]
    for obj in pack['trees']:
        assert obj['url']=='https://api.github.com/repos/'+m.REPO+'/git/trees/'+obj['sha']
        assert obj['sha'] not in trees
        trees[obj['sha']]=m.verified_tree(obj)
        receipts.append({'sha':obj['sha'],'entries':len(obj['tree']),'capture_kind':'official_nonrecursive'})
    for obj in pack.get('recursive_trees',[]):
        assert obj['url']=='https://api.github.com/repos/'+m.REPO+'/git/trees/'+obj['sha']
        for relative,derived,entries in m.expand_recursive(obj):
            sha=derived['sha'];assert sha not in trees or trees[sha]==entries
            trees[sha]=entries;receipts.append({'sha':sha,'entries':len(entries),'capture_kind':'derived_from_complete_official_recursive_response','official_recursive_root':obj['sha'],'relative_path':relative})
    return trees,receipts

def validate_creation(pack,pub,head,root,expected_branch):
    assert m.valid_oid(head) and m.valid_oid(root)
    assert expected_branch=='task/p8-evidence-delivery-a217-20261009'
    commit=pack['commit'];ref=pack['ref']
    assert commit['sha']==head and commit['tree']['sha']==root
    assert commit['url']=='https://api.github.com/repos/'+m.REPO+'/git/commits/'+head
    assert pub['operation']=='create_branch' and 'force' not in pub
    assert pub['request']=={'repository_full_name':m.REPO,'branch_name':expected_branch,'sha':head}
    assert pub['branch']==expected_branch and pub['commit']==head and pub['tree']==root
    assert [p['sha'] for p in commit['parents']]==[pub['parent']]
    assert pub['parent']=='b21cce4c8661589267ad5719f850accbec088d2f'
    assert ref['ref']=='refs/heads/'+expected_branch
    assert ref['url']=='https://api.github.com/repos/'+m.REPO+'/git/refs/heads/'+expected_branch
    assert ref['object']['type']=='commit' and ref['object']['sha']==head
    assert pub['actual_ref_readback']==ref
    create=pack['raw_create_branch_result']
    assert create==pub['actual_create_branch_result'] and create['isError'] is False
    assert create['structuredContent']=={'branch':expected_branch}
    before=pack['raw_prior_ref_absence']
    assert pub['prior_ref_absence']['official_status']==404 and before['isError'] is True
    assert before['structuredContent']['error_code']=='NOT_FOUND'
    assert before['structuredContent']['error_data']['status']=='404'
    assert pack['prior_ref_absence_capture']['sha256']==pub['prior_ref_absence']['sha256']
    assert m.metadata(pack['prior_ref_absence_capture']['path'])==pack['prior_ref_absence_capture']
    return commit,ref

def prepare(pack_path,pub_path,head,root,expected_branch,out):
    pack=m.read_json(pack_path);pub=m.read_json(pub_path)
    commit,ref=validate_creation(pack,pub,head,root,expected_branch)
    assert m.metadata(PLAN)['sha256']==PLAN_SHA
    plan=m.read_json(PLAN);assert plan['file_count']==75 and len(plan['files'])==75
    index,old74,manifest,artifacts,raw_expected=m.frozen_inputs()
    assert m.upload_rows(old74)==plan['files'][:74]
    assert len({r['repository_path'] for r in plan['files']})==75
    wanted={}
    for r in plan['files']:
        local=Path(r['source_path']);local=local if local.is_absolute() else WORKSPACE/local
        actual=m.metadata(local)
        assert all(actual[k]==r[k] for k in ['bytes','sha256','git_blob_sha1'])
        assert r['mode']=='100644' and r['type']=='blob'
        wanted[r['repository_path']]={'role':'benchmark_original_report_or_original_transport_index' if r['ordinal']<74 else 'prior_raw_publication_proof','sha':r['git_blob_sha1'],'type':'blob','mode':'100644','bytes':r['bytes'],'sha256':r['sha256']}
    assert not (set(wanted)&set(raw_expected))
    wanted.update(raw_expected);assert len(wanted)==121
    trees,tree_receipts=actual_trees(pack);bindings=[]
    # All target report files exist in this fully captured runtime directory.
    # Require the new status names to be absent rather than overwriting history.
    parent_entry,_=m.resolve(trees,root,m.PREFIX)
    assert parent_entry['type']=='tree' and parent_entry['mode']=='040000'
    assert parent_entry['sha'] in trees, 'Missing complete target directory capture'
    assert not ({'CURRENT-DELIVERY.json','CURRENT-DELIVERY.md'} & set(trees[parent_entry['sha']])), 'Current status already exists; append a versioned successor instead'
    for path,expect in sorted(wanted.items()):
        entry,chain=m.resolve(trees,root,path)
        assert all(entry[k]==expect[k] for k in ['sha','mode','type']), 'Wrong delivered object: '+path
        bindings.append({'repository_path':path,**expect,'actual_tree_chain':chain,'public_url':'https://github.com/'+m.REPO+'/blob/'+head+'/'+path})
    proof={'schema':'current-runtime-lifecycle-delivery-on-published-parent-v1','execution_source':m.E,
        'scope':{'runtime_run':37871838957,'runtime_profiles':['mixed-c1','mixed-c4','mixed-c8','mixed-c16','soak','backfill'],'lifecycle_run':37871838975},
        'actual_published_parent_commit':head,'actual_published_parent_tree':root,'actual_commit_and_ref':{'commit':commit,'ref':ref},
        'publication_operation':'create_branch','created_branch':expected_branch,'publication_receipt':m.metadata(pub_path),'official_metadata_pack':m.metadata(pack_path),'frozen_original75_plan':m.metadata(PLAN),
        'original_member_peer_review':m.metadata(m.PEER),'helper':m.metadata(HELPER),'current_delivery_script':m.metadata(__file__),
        'verified_counts':{'benchmark_original_files':74,'prior_raw_publication_proof':1,'original_zip_parts':44,'original_transport_manifests':2,'all_bound_paths':121,'original_artifacts':7},
        'complete_runtime_lifecycle_delivery':True,'original_artifacts':artifacts,'bindings':bindings,'reconstructed_trees':tree_receipts,
        'history':'Original PUBLICATION-GUARD, README, upload-pending indexes and prior raw-only publication proof retain their original bytes. This append-only record establishes current scoped delivery from the separately published immutable parent above.',
        'restore':'Use each original manifest ordinal/offset order to concatenate parts; verify part/whole size and SHA256 before extraction. The 55 original JSON reports alone are not a full replay input. Follow the unchanged original README replay recipes and preserve original databases.',
        'actual_replay_acceptance':'Inherited unchanged fixed-E original reviews and execution receipts; no native measurement or replay was repeated for this delivery status.',
        'excluded_claims':['No claim this new status file already exists in the parent it describes.','No claim full scale matrix or its aggregate is complete.','No new product or release certification.'],
        'formal_task_completion':False,'task_status_action':'no_change','counts':{'done':163,'remaining':29},'remote_or_source_or_tasks_modified':False,'native_or_original_zip_reads':0}
    out=Path(out);json_path=out/'repository-files'/m.PREFIX/'CURRENT-DELIVERY.json'
    j=m.dump_new(json_path,proof)
    url='https://github.com/'+m.REPO+'/tree/'+head
    md=(f'# 当前运行与生命周期证据交付\n\n'
        f'本范围已在独立、实际公开的父提交 [`{head}`]({url}) 中完整交付。该状态记录随后追加，不把自身当作父提交已存在的文件。\n\n'
        f'- 执行来源：`{m.E}`。运行 `{37871838957}` 的四个 mixed、1 小时 soak 与 backfill，以及生命周期 `{37871838975}`。\n'
        '- 父提交实际包含全部 74 份原报告/导航文件、1 份原始包公开证明、44 个完整原 ZIP 分片和 2 份原传输清单；7 份原 ZIP 均可完整恢复。逐路径 Git OID、模式和原字节哈希见同目录 `CURRENT-DELIVERY.json`。\n'
        '- 原 `PUBLICATION-GUARD.json`、README 和先前 pending 记录原字节保留为历史；本当前记录只更新上述交付范围的事实。\n'
        '- 恢复时按原清单序号拼接分片，核分片及整 ZIP 长度/SHA256，再按原 README 命令验证；55 份摘要 JSON 不能代替完整原始包。原数据库不可修复后冒充原件。\n'
        '- 本次只核交付，不重跑 native 或负载，不改变原验收标准。完整规模矩阵及 aggregate 仍不在此完成声明中。\n\n'
        '**原任务账本未修改：163 已完成、29 待完成；formal task completion = false。**\n')
    md_path=json_path.with_name('CURRENT-DELIVERY.md');md_path.parent.mkdir(parents=True,exist_ok=True)
    with md_path.open('xb') as f:f.write(md.encode())
    d=m.metadata(md_path)
    rows=[]
    for ordinal,record in enumerate([j,d]):
        p=Path(record['path']);rows.append({'ordinal':ordinal,'source_path':str(p),'repository_path':str(p.relative_to(out/'repository-files')),'mode':'100644','type':'blob',**{k:record[k] for k in ['bytes','sha256','git_blob_sha1']}})
    plan_out={'schema':'current-runtime-lifecycle-status-two-file-upload-plan-v1','published_parent':head,'file_count':2,'files':rows,'publication_of_new_two_files':'not_claimed_until_normal_ref_update_and_readback','formal_task_completion':False}
    upload=m.dump_new(out/'current-delivery-two-file-upload-plan.json',plan_out)
    m.frozen_inputs();assert m.metadata(PLAN)['sha256']==PLAN_SHA
    return [j,d,upload]

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--official-tree-pack',required=True);p.add_argument('--creation-receipt',required=True);p.add_argument('--expected-branch',required=True);p.add_argument('--expected-commit',required=True);p.add_argument('--expected-root-tree',required=True);p.add_argument('--output',required=True)
    a=p.parse_args();print(json.dumps(prepare(a.official_tree_pack,a.creation_receipt,a.expected_commit,a.expected_root_tree,a.expected_branch,a.output),ensure_ascii=False))
