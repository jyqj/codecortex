"""Read retained text only; freeze an applicability map, not a product validator."""
from pathlib import Path
from datetime import datetime, timezone
import hashlib
import json
import re

ROOT = Path('/workspace/scratch/bfccb8494ba0')
OUT = Path(__file__).parent
MAIN = 'round34-PR200-original-CI-logs/114095050041-original.log'
CLOSE = 'round34-PR200-original-CI-logs/114095050109-original.log'
PRIOR = 'round34-PR200-original-CI-independent-review/independent-terminal-log-review.json'
CLAUSES = 'round36-006-and-dependency-scope-review/fixed-main-source/docs/roadmap/code-index-v2/'
SCOPE = 'round37-P8-006-second-scope-review/second-independent-task-study-boundary-review.json'
BRIDGE = 'round36-006-and-dependency-scope-review/official-fixed-main-commit.json'
G = '9cc6bf49f6dd81e4069a8004eed49addba0ab79b'
TREE = '6461938788665701cfa4fa095a064a3a404ec492'
MAIN_SHA = 'b9b089bb4eae072affe9326681d4980eae15fd84'

def meta(path, published=None):
    p = ROOT / path
    b = p.read_bytes()
    oid = hashlib.sha1(b'blob ' + str(len(b)).encode() + b'\0' + b).hexdigest()
    if published is not None:
        assert oid == published, (path, oid, published)
    return dict(path=str(p), bytes=len(b), sha256=hashlib.sha256(b).hexdigest(),
                git_blob=oid, already_published_blob=published is not None)

published = {
    MAIN: '14e542a2ffcabb8ea307ecc8b5964dcbc755c1aa',
    CLOSE: '4adb4f15c10b668dc47e3da3068850e5161c77d9',
    PRIOR: '78ac902a240ddc2eccc7099aa5cd35dd26247ddf',
    SCOPE: 'f95a908ae40172deacb8f25f6b0df594a6d20a8a',
}
inventory = [meta(p, oid) for p, oid in published.items()]
inventory += [meta(BRIDGE), meta(CLAUSES + 'tasks.json'), meta(CLAUSES + '06-VALIDATION.md')]
prior = json.loads((ROOT / PRIOR).read_text())
scope = json.loads((ROOT / SCOPE).read_text())
bridge_outer = json.loads((ROOT / BRIDGE).read_text())
bridge = json.loads(bridge_outer['structuredContent']['content'])
assert bridge['sha'] == MAIN_SHA and bridge['tree']['sha'] == TREE
assert prior['source']['G'] == G and prior['source']['tree'] == TREE
assert prior['checkout']['main']['tree'] == TREE
assert prior['checkout']['closeout']['actual'] == G
assert scope['fixed_main'] == MAIN_SHA and scope['same_complete_tree_source'] == G

main_raw = (ROOT / MAIN).read_text().splitlines()
close_raw = (ROOT / CLOSE).read_text().splitlines()
ansi = re.compile(r'\x1b\[[0-9;]*m')
main = [ansi.sub('', line) for line in main_raw]
close = [ansi.sub('', line) for line in close_raw]

def method(name, expected_ok=2):
    pattern = re.compile(r' test ' + re.escape(name) + r' \.\.\. (ok|ignored)(.*)$')
    records = []
    for n, line in enumerate(main, 1):
        m = pattern.search(line)
        if m:
            records.append(dict(line=n, outcome=m.group(1), detail=m.group(2).lstrip(', ')))
    assert sum(x['outcome'] == 'ok' for x in records) == expected_ok, (name, records)
    return dict(method=name, records=records)

def claim(key, requirement, names, note=None):
    r = dict(id=key, original_scope=requirement, status='existing_actual_pass_applicable',
             log='main', controls=[method(x) for x in names])
    if note:
        r['boundary'] = note
    return r

claims = [
    claim('V07-mutations', '增删改；no-op/body/API 的旧行为', [
        'body_only_changes_keep_surface_and_do_not_promote_callers',
        'direct_writer_surface_storage_delete_and_reopen',
        'event_scoped_missing_file_and_candidate_changes_match_full_rebuild',
        'deleting_a_pending_root_revokes_every_consumers_old_target']),
    claim('V07-chain-cycle', '重导出链、环及多轮收敛', [
        'rust_workspace_pub_use_signature_change_converges',
        'python_and_rust_forward_cycles_match_full_after_signature_changes',
        'reexport_cycle_and_deep_chain_converge_across_bounded_rounds']),
    claim('V07-fanout', '大 fanout、明确未完状态、债务跨轮/重开恢复', [
        'dependency_fanout_over_budget_is_partial_not_normal',
        'budget_remainder_survives_reopen_and_an_empty_event_scope',
        'budget_remainder_is_resumed_by_unchanged_incremental_builds',
        'fanout_larger_than_a_lookup_window_is_not_truncated_out_of_debt'],
        '这些是原回归控制；规模研究中五个 fanout 的实际 first-incomplete/resume/closure 仍引用原 1k shard，不能从这些控制推断 100k 结果。'),
    claim('V07-negative-new-name', '负向查找、新同名/歧义转换及路径优先级', [
        'global_unique_becomes_ambiguous_then_unique_again',
        'missing_name_then_provider_appears_refreshes_unchanged_consumer',
        'negative_path_and_precedence_changes_refresh_import_routes',
        'go_package_files_and_same_name_other_packages_are_isolated']),
    claim('V07-warm-cold-full', 'warm/cold cache 多轮固定点与 full parity', [
        'repeated_warm_catalog_mutations_match_cold_rebuild',
        'late_provider_change_rebases_without_losing_old_consumers',
        'pending_changes_rebase_and_shrinker_cannot_certify_incomplete']),
    claim('006-config-route', 'config-only 与 API 路由旧行为', [
        'p3a_configuration_only_change_retargets_unchanged_consumers_and_survives_reopen',
        'local_go_replace_resolves_members_across_files_and_retargets_on_config_only',
        'api_edit_keeps_route_manifest_equal_to_full_and_preserves_route_nodes']),
    claim('006-oracle-compatibility', '完整对账、重复/Value/尾行与原资源错误不变', [
        'agrees_with_legacy_for_all_types_duplicates_and_physical_columns',
        'every_existing_table_remains_part_of_the_comparison',
        'equal_inputs_must_both_fit_the_original_scratch_limit',
        'second_input_budgets_and_first_error_remain_enforced',
        'row_byte_and_disk_budgets_fail_instead_of_certifying_a_prefix',
        'compares_more_than_the_legacy_row_budget_and_detects_the_last_row_change'],
        '原 benchmark_oracle_streaming 目标每配置 13 passed；不把旧 witness 源的结果移植到本源。四个借用序列化 correctness 方法与默认/eval-http 来源已由原 78ac902… 报告核实，此处直接引用，不重审算法。'),
    claim('006-scale-protocol', '真实阶段计时、独立 fanout 真值、15 表、旧/新 profile 分离', [
        'complete_stage_timing_accounts_for_real_split_handoffs_and_full_staging',
        'failed_independent_fanout_truth_remains_a_failure',
        'bounded_python_forwarding_resume_keeps_all_targets_and_fifteen_table_parity',
        'named_wide_dirty_profile_is_distinct_and_preserves_legacy_budgets',
        'wide_dirty_cli_rejects_cross_profile_budgets_before_starting_work',
        'actual_shard_keeps_global_ids_and_changes_exact_batch_including_yaml_and_routes',
        'real_scale_smoke_preserves_builds_counts_mutations_and_full_parity'],
        '工程 fixture/smoke 不代替五个真实 rep0，不得记为新的研究样本或 N30/性能通过。'),
]

suite_lines = [(1615,1630),(1632,1650),(1652,1668),(1675,1690),
               (1395,1412),(2166,2186),(7713,7728),(7730,7748),
               (7750,7766),(7493,7510),(8264,8284)]
suites = []
for start, end in suite_lines:
    # p2d's header position is resolved exactly rather than assumed from neighbouring targets.
    if start == 1675:
        candidates = [(i+1, s) for i,s in enumerate(main[:2000]) if 'Running tests/p2d_mutations.rs ' in s]
        assert len(candidates) == 1
        start = candidates[0][0]
    assert 'Running ' in main[start-1] and 'test result: ok.' in main[end-1]
    suites.append(dict(target_line=start, target=main[start-1].split('Running ',1)[1],
                       footer_line=end, footer=main[end-1].split('test result: ',1)[1],
                       configuration='default' if start < 7284 else 'eval-http'))

mcp_names = [
    'p2a_public_mcp_reports_coverage_and_refreshes_provider',
    'p2b_public_mcp_retracts_ambiguous_edges_and_updates_go_package',
    'p2c_public_mcp_reports_debt_resumes_and_survives_restart',
    'p2d_public_mcp_multilanguage_rename_truth_and_no_fake_jsts_calls',
    'p3a_real_mcp_configuration_only_retarget_and_restart',
    'real_mcp_package_configuration_retarget_resumes_after_restart',
    'p3c_real_mcp_go_work_only_change_resumes_across_restart',
]
mcp = [method(n, 1) for n in mcp_names]
mcp_command_lines = [8614,8615,8655,8656,8702,8703,8764,8765,8766,
                     8852,8853,8854,8915,8916,8917,8978,8979,8980]
assert '"source_commit": "1e14b7ad8d097da0000e18597da0316143de919c"' in main[8367]
assert '"source_inputs": 1096' in main[8367]

ignored = [method(n, 0) for n in [
    'tests::bench_incremental_dirty_closure',
    'p2d_release_incremental_growth_and_bounded_resume',
    'p3c_release_go_config_change_scales_with_consumers_not_unrelated_files',
    'benchmark::oracle::projection_tests::borrowed_serialization_equal_lifetime_release_cost_probe',
]]
assert all(all(v['outcome']=='ignored' for v in x['records']) and x['records'] for x in ignored)
assert 'ready_publication_status_keeps_request_policy_and_per_query_coverage_distinct ... ok' in close[3673]
assert '12 passed; 0 failed; 0 ignored' in close[3916]

task_root = json.loads((ROOT / (CLAUSES + 'tasks.json')).read_text())
task_list = task_root['tasks'] if isinstance(task_root, dict) else task_root
task = next(x for x in task_list if x['id']=='P8-006')
assert task['status']=='in_progress' and task['validations']==['V07','V20']
required = {k:task[k] for k in ['id','status','depends_on','steps','deliverables','acceptance','validations','rollback']}
validation_line = (ROOT / (CLAUSES+'06-VALIDATION.md')).read_text().splitlines()[28]
assert '| V07 |' in validation_line

report = {
    'schema':'p8_006_bounded_regression_applicability_v1',
    'created_at_utc':datetime.now(timezone.utc).isoformat(),
    'reviewer':'/root/todo_audit',
    'status':'accepted_scoped_existing_regression_evidence_applicable',
    'apply_now':False,
    'decision':{
        'V07_minimum_scenarios':'原 06-VALIDATION:29 的各类 V07 场景已有同树工程控制实际通过，相关 public MCP ignored 方法又在同日志后段按显式 --ignored 实跑通过。',
        'additional_regression_execution_required_by_this_review':False,
        'new_source_or_regression_blocker_found':False,
        'task_done':False,
        'remaining':'100k rep0 真实终态与原 validate_shard 接受仍 pending；取得后补五尺度完整 45 主阶段 + 5 fanout 的描述性原事实、最终独立收尾 receipt 和持久 locator。已有四片/回归不重跑。',
        'limits':'仅补足已有六项 closeout 输入中的第 4 项 V07/旧回归适用性；其余要求沿 f95a908…/9263e672… 原文，不新增测试、统一 N30/150 task gate 或发布通过。',
    },
    'source_identity':{
        'fixed_main':MAIN_SHA, 'fixed_G':G, 'whole_tree':TREE,
        'main_job':prior['official_jobs'][0], 'closeout_job':prior['official_jobs'][1],
        'actual_checkouts':prior['checkout'],
        'main_bridge':{'path':str(ROOT/BRIDGE),'actual_commit':bridge['sha'],'actual_tree':bridge['tree']['sha'],
                       'parents':[p['sha'] for p in bridge['parents']]},
        'claim':'main 工作实际 checkout 仍称 1e14b7…；以完整 tree 等同桥应用到 G9cc/b9，绝不重标字面执行来源。closeout 直接 checkout G9cc。'
    },
    'original_task':required,
    'original_clause_refs':{
        'tasks_lines':[12145,12178], 'V07':{'line':29,'text':validation_line},
        'V20_line':42, 'release_G8_line':58,
        'previous_scope_review':published[SCOPE],
        'scope_is_unchanged':'描述性各 host N=1 条件收尾与原注册 N30/150/1500 研究分栏；后者未齐不能 passed。'
    },
    'original_byte_inventory':inventory,
    'claim_to_actual_named_controls':claims,
    'selected_target_footers':suites,
    'ignored_then_explicitly_executed_public_controls':{
        'producer_receipt_line':8368,'producer_receipt_text':main[8367],
        'commands':[{'line':n,'text':main[n-1]} for n in mcp_command_lines],
        'methods':mcp,
        'scope':'这七方法在 ordinary default/eval-http targets 的 ignored 记录原样保留，后续显式 default product MCP 各 1 passed。日志中的生产者 receipt 是原执行证据；本审未重新获取 ELF/源快照或运行 MCP。'
    },
    'related_closeout':{
        'source':G, 'original_review':published[PRIOR],
        'V18_configurations':[{k:x[k] for k in ['configuration','target_line','footer']} for x in prior['closeout']['V18']],
        'repaired_control':prior['closeout']['repaired_method'],
        'prior_failure_log_blob':'3efadf710a054e2beda886cdd29938639145e94b',
        'boundary':'修后原 8/9/12 V18 targets 的实际成功补相关兼容回归，不能代替 V07；旧 f675 pending>0 失败不抹除。未下载或重放 closeout ZIP。'
    },
    'not_credited':{
        'still_ignored_cost_controls':ignored,
        'zero_pass_filtered_invocations':'原日志 6821–7233 等 filtered-only footers 不计新增通过；同名不同 crate 或两配置执行不累加为唯一测试总数。',
        'no_performance_pass':'本映射不提供成本 probe、新 100k 测量、稳定尾部、统计 CI、whole release/G8 或任务完成信用。',
        'retained_negative_evidence':'旧 Gc8/C3ff/G8 失败、旧 witness 兼容边界和 Gbcc3 workflow jobs0 失败沿既有归档保持；当前工程 pass 不改写它们。'
    },
    'current_working_count':{
        'run_id':38013753078,'attempt':1,'source':G,'profile':'scale_wide_dirty_v1',
        'accepted_shards':4,'accepted_observations':41,'registered_shards':150,'registered_observations':1500,
        'pending_100k_rep0_receipt':None,'final_006_receipt':None,
        'task_done_count':164,'remaining_tasks':28,'session_completed':1,
        'observation_boundary':'沿 root 本任务委派时态与已封四片；本审未新 poll，不声称研究最新实时终态。'
    },
    'ready_to_reference':{
        'prior_fourth_shard_review':'f24820113d77c649c778f68943a8c36a20531588',
        'prior_fourth_progress':'371bd3bb09e602d6f61b29fb87fc43684f6c71fb',
        'future_nine_applyfalse':'9263e672782ea2f26c8d2f075756d01574368634',
        'use':'未来证据/implementation_notes 可引用本报告补第 4 项原输入；当前不执行状态更新，不能自动应用旧字段中的 future null。'
    },
    'operations':{
        'read_retained_text_only':True,'new_network_reads':0,'Actions_downloads':0,
        'ZIP_or_CRC_reads':0,'product_or_test_runs':0,'original_validator_runs':0,
        'source_scans':0,'task_mutations':0,'ref_mutations':0,
        'extractor_success_is_not_product_exit':'本脚本只冻结既有原文本行及出处；执行退出不计为 Rust/规模验证退出。'
    }
}

def write(name, value):
    p=OUT/name
    assert not p.exists(), str(p)
    p.write_text(json.dumps(value,ensure_ascii=False,indent=2)+'\n')
    return meta(str(p.relative_to(ROOT)))

r=write('independent-V07-and-old-regression-applicability.json',report)
s=write('archive-selection.json',{'schema':'selected-evidence-v1','kind':'bounded_mapping_no_original_log_duplication',
    'files':[r,meta(str(Path(__file__).relative_to(ROOT)))],
    'original_logs_referenced_not_copied':published})
print(json.dumps({'status':'frozen','report':r,'selection':s,'new_product_runs':0,'new_task_credit':0},ensure_ascii=False))
