"""Prepare future JSON only; no tasks, generators, tests, studies or refs are changed."""
from pathlib import Path
import hashlib,json
from datetime import datetime,timezone

ROOT=Path('/workspace/scratch/bfccb8494ba0')
OUT=Path(__file__).parent
PREFIX='artifacts/checkpoints/p8-round39-complete-bfcc-20261010/post-nine/'

def info(p, expected=None):
    p=Path(p);b=p.read_bytes()
    r=dict(path=str(p),bytes=len(b),sha256=hashlib.sha256(b).hexdigest(),git_blob=hashlib.sha1(b'blob '+str(len(b)).encode()+b'\0'+b).hexdigest())
    if expected:assert r['git_blob']==expected,(p,r['git_blob'])
    return r
def read(relative):return json.loads((ROOT/relative).read_text())
def put(name,x):
    p=OUT/name;assert not p.exists(),str(p)
    p.write_text(json.dumps(x,ensure_ascii=False,indent=2)+'\n');return info(p)

oldp=ROOT/'round35-post-nine-closeout-preparation/P8-017-019-publication-bridge.applyfalse.json'
oldmeta=info(oldp,'7108a17fab7a7589652896ef654e0383d8cbef30');old=json.loads(oldp.read_text())
old020=info(ROOT/'round35-post-nine-closeout-preparation/P8-020-M4-local-release-input.applyfalse.json','370056a2b2395f7bcb941c38bac04a016ca68e97')
originalmap=info(Path(old['inherited_map']['local_path']),'e863b8b3fe42a5fe3797fac251f48f158362087d')
ledger=ROOT/'round36-006-and-dependency-scope-review/fixed-main-source/docs/roadmap/code-index-v2/tasks.json'
ledgermeta=info(ledger,'adb44ee70a4c129bfb443d4ecf678fb934c8ea6b')
x=json.loads(ledger.read_text());tasks={t['id']:t for t in x['tasks']}
assert sum(t['status']=='done' for t in tasks.values())==164 and len(tasks)==192
for definition in old['original_definitions']:
    assert all(tasks[definition['id']][k]==v for k,v in definition.items())
    assert tasks[definition['id']]['status']=='in_progress'
baseline_path=ROOT/'round39-doc-baseline-original-intake/independent-original-baseline-terminal-review.json'
baseline_meta=info(baseline_path,'565e1c18e24a382c43f18f8bf9ecef8873d4b4d0')
baseline=json.loads(baseline_path.read_text())
assert baseline['original_execution']['returncode']==0 and baseline['drift']['count']==3
assert baseline['source_and_run']['fixed_product_and_docs_commit']=='b9b089bb4eae072affe9326681d4980eae15fd84'
assert baseline['document_applicability']['required_current_false_statement_fix'] is None
new_ci=info(ROOT/'round38-P8-006-regression-applicability/independent-V07-and-old-regression-applicability.json','8470f53fced7880511644a1ddfd3f391a0e10cce')

by_claim={v['id']:v for v in old['claim_updates']}
inherited={
 'P8-017':['017-owner','017-deletion','shared-V18','shared-V21'],
 'P8-018':['018-facts','018-doc-drift','018-install','shared-V18','shared-V21'],
 'P8-019':['019-custody','019-replay','019-V01','shared-V21'],
}
for ids in inherited.values():
    assert all(by_claim[i]['status'].startswith('accepted_') for i in ids)
baseline_record={
 'source':baseline['source_and_run'],
 'review':baseline_meta,
 'support_actual_blob':'a441c437f3c121568417ab490e722d644dd16a7b',
 'selection_actual_blob':'f70a585d1b5d152700a57a3f20e8e557ab4f0401',
 'repository_prefix':'artifacts/checkpoints/p8-round39-complete-bfcc-20261010/baseline/terminal/',
 'actual_publication_commit':None,
 'publication_state':'actual blobs exist; round39 checkpoint publication is root-owned and not yet supplied here',
 'argv':['./scripts/update-doc-baselines.sh'],
 'started_at':baseline['original_execution']['started_at'],
 'completed_at':baseline['original_execution']['completed_at'],
 'actual_exit_code':0,'wall_seconds':baseline['original_execution']['wall_seconds'],
 'stdout_member':baseline['outer_stdout'],'stderr_member':baseline['outer_stderr'],
 'drift_lines':baseline['drift']['exact_lines'],
 'interpretation':'本次确实执行原贡献约定命令，不是豁免；退出0与三条历史对照DRIFT同时成立。2815/76/2891仅原footer汇总，内部Cargo全文未保留，不称唯一测试数或全feature/发行通过。',
 'tracked_state':'before/after HEAD、tree、完整index不变；tracked/source/lock diff为空；Cargo.lock before/after/final逐字同。',
 'historical_docs':'TEST_PLAN c86e4628 明确历史快照和b069旧失败出处；保留该历史，不把当前计数刷进旧表。未识别需替换的当前false声明；可以追加带身份的新核对记录。',
 'unknowns':['deduplicated unique-method count','deduplicated per-crate census','all-features coverage'],
 'no_new_requirements':['no no-DRIFT gate','no forced rerun','no parser-fix gate','no full150/G8 gate for019'],
}

notes={
'P8-017':'复用原ownership、删除及相关V18/V21证据，原35轮publication bridge已确认PR198三文档实际合入；历史删除、lane收口、平台/回滚控制各保持原source/run，不把prepare-only或旧ignored误算执行。实际P8-016结项后，可按原定义进行017正式结项；当前准备不改状态。',
'P8-018':'原事实生成/检查、安装/故障/默认离线说明及PR198完整15表文字修正证据复用。新增固定b9、workflow de14、run38020782259/a1原update-doc-baselines实际退出0，三条历史DRIFT原样保留；TEST_PLAN历史基线不覆盖，原2815/76为脚本footer汇总，非unique/allfeatures/release。旧not_run保持其创建时态，新证据追加补足此约定执行缺口。实际017结项后可依原定义正式判断018；当前不改状态。',
'P8-019':'复用原42archive控制和3个native replay命名控制，原source/配置/重复执行分母保持；PR198已发布仅在副本的单run重算说明。完整性verify与数值replay分开，binary/checksum/manifest/raw/gates及no-clobber/latest-run原要求不变。实际018结项后可按归档功能scope正式判断019；不由此宣布完整150研究、指标门或G8发行通过。',
}
updates=[]
for n,id in enumerate(['P8-017','P8-018','P8-019'],1):
    t=tasks[id]
    evidence_keys=sorted({k for claim in inherited[id] for k in by_claim[claim]['evidence_keys']})
    updates.append({
      'task_id':id,'current_status':t['status'],'apply_now':False,'proposed_status_only_after_acceptance':'done',
      'original_hard_dependencies':t['depends_on'],'required_actual_dependency_closeout_receipt':None,
      'original_definition':next(d for d in old['original_definitions'] if d['id']==id),
      'reused_accepted_claim_ids':inherited[id],
      'reused_evidence_keys':evidence_keys,
      'own_scope_assessment':'existing components accepted; no additional concrete own-scope implementation or mandatory execution gap identified in these retained records',
      'new_evidence':(['018-convention actual baseline execution'] if id=='P8-018' else []),
      'remaining_before_actual_done':['原硬依赖实际done及其回执','实际最终结项决定与非空不可变证据指针；本准备不能代签'],
      'future_evidence_append':{
          'date':None,'target_sha':'b9b089bb4eae072affe9326681d4980eae15fd84',
          'status':'future_actual_task_acceptance_not_applied',
          'source_evidence_bridge_blob':oldmeta['git_blob'],
          'claim_map_blob':originalmap['git_blob'],
          'baseline_review_blob':baseline_meta['git_blob'] if id=='P8-018' else None,
          'final_dependency_receipt':None,'final_independent_decision':None,
          'artifact_paths':None,
          'scope':'原task自身组件与按序依赖闭合；不扩成新的统计、全release或live认证',
          'rollback_status':t['rollback'],
      },
      'future_implementation_notes_append':notes[id],
      'conditional_counts_after_nine_and_this_task':{'done':173+n,'remaining':19-n,'session_completed':10+n},
    })

report={
 'schema':'p8_post_nine_baseline_evidence_addendum_v1',
 'created_at_utc':datetime.now(timezone.utc).isoformat(),'author':'/root/todo_audit',
 'append_only':True,'apply_now':False,'task_state_modified':False,'current_task_credit':0,
 'purpose':'只追加018原baseline真实执行与post-nine适用判断；旧7108a17/370056a2/e863b8b3等原件不回写。',
 'fixed_ledger':ledgermeta,
 'inherited_unchanged_inputs':{'publication_bridge':oldmeta,'claim_map':originalmap,'M4_local_release_draft':old020},
 'actual_PR198_publication':old['publication'],
 'baseline_new_actual_evidence':baseline_record,
 'specific_preparation_delta':{
    'old_018_convention_status':'documented_process_check_not_run_no_known_waiver',
    'new_separate_evidence_status':'original_command_actual_exit0_with_three_historical_drifts',
    'old_text_preservation':'旧文件中的not_run、open和当时source按历史原样保留；本增补是后到真实证据，不把旧记录写成当时已执行。',
    'waiver':None,
    'scope_not_changed':['original task steps/acceptance/dependencies','PR198 exact3doc publication','all prior source/run identities','optional/live boundaries','original study registration and failures'],
 },
 'reused_evidence_identities':old['inherited_evidence_identities'],
 'current_source_regression_context':{
    'fixed_main':'b9b089bb4eae072affe9326681d4980eae15fd84',
    'same_tree_G':'9cc6bf49f6dd81e4069a8004eed49addba0ab79b',
    'tree':'6461938788665701cfa4fa095a064a3a404ec492',
    'retained_PR200_engineering_review_blob':'78ac902a240ddc2eccc7099aa5cd35dd26247ddf',
    'bounded_existing_log_applicability':new_ci,
    'boundary':'引用原成功和whole-tree桥，不重新执行，也不把历史组件执行字面改名为当前main。旧f675失败仍保留。'
 },
 'future_task_updates':updates,
 'conditional_decision':{
    'if_original_nine_tasks_really_close':'017→018→019 可按原硬依赖顺序进行实际结项；已有自身组件可复用，baseline not_run新增事实已解决。没有发现必须另做实现、另跑测试或等待full150/G8才能验归档功能的原门。',
    'not_automatic':'仍需实际依赖闭合及root最终有证据的任务决定；当前三个task仍in_progress，三个未来dependency receipts与final decisions全部null。',
    'real_own_scope_gap_identified':False,
    'original_study_and_release_still_open':'完整150/1500的N30研究与aggregate保留独立真实状态。P8-020/G8发布范围、原live/heldout条件和blockers不因017–019准备或将来任务完成而自动通过。',
    'historical_19_release_controls':'沿旧370056a2原release-review草案保持19工具控制和两个发布范围blocked的身份；不新增P8-020发布签署或信用。',
    'if_new_contrary_evidence_arrives':'按原不得忽略失败要求追加判断；不以本准备覆盖真实反例。'
 },
 'counts':{
    'now':{'done':164,'remaining':28,'session_completed':1,'user_target_session_completed_at_least':10},
    'after_real_nine_only':{'done':173,'remaining':19,'session_completed':10},
    'after_real_nine_and_three_only':{'done':176,'remaining':16,'session_completed':13},
    'all_future_counts_are_conditional':True,
 },
 'application_boundaries':{
    'allowed_future_task_fields':['status','evidence append','implementation_notes append'],
    'protected_definitions_dependencies_historical_evidence':'unchanged',
    'final_006_receipt':None,'final_actual_application_source':None,'actual_round39_publication_commit':None,
    'no_now_generator_or_checker':True,'no_new_application_tool':True,
    'future_original_generator':'原code_index_plan --write和无参检查由root在实际批准输入后处理；本增补未运行。',
 },
 'operations':{'new_tests':0,'original_script_reruns':0,'old_archive_or_CRC_rereads':0,'Actions_downloads':0,'network_reads':0,'task_writes':0,'ref_mutations':0},
}
main=put('post-nine-baseline-addendum.applyfalse.json',report)
summary=put('bounded-change-summary.json',{
 'apply_now':False,'old_input_preserved':oldmeta,
 'only_new_material_fact':'fixed b9 original baseline command actually completed exit0 with 3 historical DRIFT lines',
 'unchanged_accepted_scope':inherited,
 'hard_dependency_chain':['P8-016','P8-017','P8-018','P8-019'],
 'current_missing_facts':['actual nine-task closeouts including016','actual sequential017/018/019 decisions','future application/pub receipts'],
 'new_unmet_own_scope_gate':None,'full150_G8_pass_claim':False,
 'counts':report['counts'],'addendum':main,
})
selection=put('archive-selection.json',{'schema':'selected-evidence-v1','files':[main,summary,info(Path(__file__))],
 'no_old_large_artifact_copied':True,'apply_now':False})
manifest=put('upload-manifest.computed.json',{'server_upload_pending':True,'files':[dict(x,repository_path=PREFIX+Path(x['path']).name,mode='100644',type='blob') for x in [main,summary,info(Path(__file__)),selection]]})
assert info(oldp)==oldmeta and info(ledger)==ledgermeta
assert info(ROOT/'round35-post-nine-closeout-preparation/P8-020-M4-local-release-input.applyfalse.json')==old020
print(json.dumps({'addendum':main,'summary':summary,'selection':selection,'manifest':manifest,'old_inputs_unchanged':True,'current_task_credit':0},ensure_ascii=False))
