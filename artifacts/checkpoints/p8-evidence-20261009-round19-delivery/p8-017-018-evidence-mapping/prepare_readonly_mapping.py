#!/usr/bin/env python3
"""Read existing fixed-source evidence; never execute product/tests or edit a repo."""
import hashlib
import json
from pathlib import Path
import re
import subprocess

OUT = Path(__file__).resolve().parent
WS = Path('/workspace/scratch/a217aaae3bde')
RAM = Path('/dev/shm/a217aaae3bde')
VIEW = RAM / 'codecortex-combined-guard-view'
E = 'a23bb72d3c954f385b99fe81ce9189885c208557'
M = 'b21cce4c8661589267ad5719f850accbec088d2f'
GAP = WS / 'checkpoint-round19-prep/p8-017-019-gap-review'
PR = RAM / 'platform-review/pr167-execution'
read_records = {}

def digest(b):
    return {'bytes': len(b), 'sha256': hashlib.sha256(b).hexdigest(),
            'git_blob_sha1': hashlib.sha1(b'blob '+str(len(b)).encode()+b'\0'+b).hexdigest()}

def record(p):
    p = Path(p); b = p.read_bytes()
    r = {'path': str(p), **digest(b)}
    read_records[str(p)] = r
    return r

def load(p):
    record(p)
    return json.loads(Path(p).read_bytes())

def git(*args):
    return subprocess.check_output(['git', '-C', str(VIEW), *args])

assert git('rev-parse', 'HEAD').decode().strip() == E
source = {}

def src(path):
    if path not in source:
        b = git('show', E+':'+path)
        row = git('ls-tree', E, '--', path).decode().strip().split(None, 3)
        assert row[1] == 'blob' and row[2] == digest(b)['git_blob_sha1']
        disk = VIEW/path
        if disk.is_file():
            assert disk.read_bytes() == b
        source[path] = {'repository_path': path, 'git_mode': row[0], **digest(b),
                        'bytes_read_from_exact_Git': True, 'materialized_view_compared': disk.is_file()}
    return git('show', E+':'+path).decode()

tasks_snapshot = load(GAP/'original-complete-task-snapshots.json')
tasks = {t['id']: t for t in tasks_snapshot['tasks']}
gap = load(GAP/'gap-report.json')
facts = load(GAP/'current-E-original-CI-facts-archive-readback.json')
ci = load(PR/'job-113631480871-independent-review.json')
log_path = PR/'job-113631480871-original.log'
log_rec = record(log_path)
assert log_rec['sha256'] == ci['log']['sha256'] == facts['original_log_sha256']
lines = log_path.read_text(encoding='utf-8-sig').splitlines()
assert ci['actual_checkout'] == '75649f8cb08dcd5427e7e58b1ca2b8fa67c9d037'
assert ci['actual_tree'] == git('rev-parse', E+'^{tree}').decode().strip()
merge_path = RAM/'platform-review/round19-actual-E-merge/independent-review.json'
merge = load(merge_path)
assert merge['actual_merge'] == M and merge['all_nonartifacts_root_entries_equal_E']
platform = load(PR/'platform-e-complete-review/platform-e-cell-review.json')
rollback = load(PR/'recovery-e-review/recovery-e-review.json')
p7 = load(PR/'p7-e-originals-review/independent-review.json')
owner_path = RAM/'scale-combined-E-review/P8-017-narrow-static-owner-gap-review.json'
owner = load(owner_path)
owner_peer = record(RAM/'scale-combined-E-review/P8-017-gap-wording-cross-review.json')
for r in owner['actual_source_files']:
    src(r['path']); assert source[r['path']]['sha256'] == r['sha256']

for p in ['docs/ARCHITECTURE.md', 'docs/BENCHMARK.md', 'docs/MCP_TOOLS.md',
          'docs/CONFIGURATION.md', 'docs/roadmap/code-index-v2/P8-FACTS.md',
          'docs/roadmap/code-index-v2/06-VALIDATION.md', 'scripts/p8_facts.py',
          'crates/cc-server/src/cli.rs', 'crates/cc-server/src/tools.rs',
          'crates/cc-eval/tests/p1c_contract.rs', 'crates/cc-eval/tests/p1d_docs.rs',
          'crates/cc-eval/tests/p5d_contract.rs', 'crates/cc-eval/tests/p5b_execution.rs',
          'crates/cc-server/tests/p7_v18_parameter_contract.rs',
          'crates/cc-eval/tests/p8_measurements.rs', '.github/workflows/ci.yml']:
    src(p)
for path, sha in facts['facts_original_output']['input_sha256'].items():
    src(path); assert source[path]['sha256'] == sha

test_sources = {
 'all_fourteen_parameter_types_deny_unknown_and_keep_default_requests': 'crates/cc-eval/tests/p1c_contract.rs',
 'search_modes_optional_nulls_and_utf8_sanitize_are_stable': 'crates/cc-eval/tests/p1c_contract.rs',
 'p1c_fourteen_tools_valid_unknown_schema_and_modes': 'crates/cc-eval/tests/p1c_contract.rs',
 'search_examples_and_configuration_match_current_parameter_types': 'crates/cc-eval/tests/p1d_docs.rs',
 'p1d_documented_search_requests_respect_actual_scope_and_wire_shapes': 'crates/cc-eval/tests/p1d_docs.rs',
 'policy_parameters_are_optional_strict_and_documented': 'crates/cc-eval/tests/p5d_contract.rs',
 'real_stdio_policy_override_and_capability_state': 'crates/cc-eval/tests/p5d_contract.rs',
 'real_mcp_preserves_tool_schema_and_exposes_offline_query_policy': 'crates/cc-eval/tests/p5b_execution.rs',
 'advertised_schema_keeps_fourteen_tools_and_optional_strategy_defaults': 'crates/cc-server/tests/p7_v18_parameter_contract.rs',
 'malformed_strategy_and_symbol_incompatibility_are_public_parameter_errors': 'crates/cc-server/tests/p7_v18_parameter_contract.rs',
 'symbol_mode_preserves_legacy_array_and_sanitized_limits': 'crates/cc-server/tests/p7_v18_parameter_contract.rs',
 'omitted_null_and_explicit_overrides_inherit_project_strategy_through_queries': 'crates/cc-server/tests/p7_v18_parameter_contract.rs',
 'distribution_and_interval_keep_one_nearest_rank_convention': 'crates/cc-eval/tests/p8_measurements.rs',
 'legacy_report_percentiles_preserve_group_denominators_and_empty_wire_values': 'crates/cc-eval/tests/p8_measurements.rs',
}
test_receipts = {}
for name,path in test_sources.items():
    text = src(path); declaration = [i for i,l in enumerate(text.splitlines(),1) if re.search(r'\bfn '+re.escape(name)+r'\(',l)]
    assert len(declaration) == 1
    hits = [{'line':i, 'original_line':l} for i,l in enumerate(lines,1) if 'test '+name+' ... ok' in l]
    assert hits, name
    test_receipts[name] = {'source': path, 'declaration_line': declaration[0], 'source_sha256':source[path]['sha256'],
        'actual_ok_occurrences':hits, 'execution_log':log_rec, 'actual_checkout':ci['actual_checkout'],
        'count_semantics':'Occurrences, not independent unique samples; source #[ignore] alone is not a pass, these exact original execution lines are.'}

def docrow(key,path,line,claim,basis,limit):
    return {'id':key,'document':path,'line':line,'statement_scope':claim,'evidence':basis,'boundary':limit}

docs = [
 docrow('architecture-counts','docs/ARCHITECTURE.md',19,'8 workspace crates; schema25;30 ordinary tables+5 FTS;14 MCP tools', ['original_CI_facts','Cargo.toml','crates/cc-db/src/index_migrate.rs','crates/cc-db/src/sql/index_v1.sql','crates/cc-server/src/mcp.rs'],'Explicit SQL declarations exclude SQLite internal/shadow tables; no runtime MCP schema version is inferred from database25.'),
 docrow('architecture-features','docs/ARCHITECTURE.md',41,'Default server features do not include cc-semantic; semantic enables local subsystem, semantic-http transport',['crates/cc-server/Cargo.toml','original_platform_8_cells','original_P7_offline_2_packages'],'Two default/semantic release build profiles, not published release packages or live provider approval.'),
 docrow('architecture-index-cache-boundary','docs/ARCHITECTURE.md',3,'index.sqlite3 authoritative index plus optional discardable derived semantic cache; mismatch rebuild',['original_rollback_version_stages','original_cache_format_space_namespace'],'The later single-db invariant refers to index authority; it must not erase the explicitly separate artifact cache.'),
 docrow('benchmark-current-vs-history','docs/BENCHMARK.md',3,'Local content lock/archive/layers/failure gates are current tooling; historical/target observations are scoped separately',['scripts/p8_release_evidence.py','docs/roadmap/code-index-v2/P8-RELEASE-EVIDENCE.md','original_CI_39_archive_controls'],'Lines12–14 deny release certification from ordinary exit0; lines26+ identify legacy duplex; historical3600 command and45s observation are not current E soak evidence.'),
 docrow('benchmark-old-numbers','docs/BENCHMARK.md',181,'2026-08-23 scale observations and optimization-history tables retain dated original scope',['docs/BENCHMARK.md:181','docs/BENCHMARK.md:202','docs/BENCHMARK.md:313'],'Do not restate historical rates, controlled tail,100k or provider quality as current E acceptance. Current E scale matrix remains separately pending.'),
 docrow('mcp-tool-count','docs/MCP_TOOLS.md',3,'All14 names remain registered and public input contracts remain strict',['original_CI_facts','all_fourteen_parameter_types_deny_unknown_and_keep_default_requests','p1c_fourteen_tools_valid_unknown_schema_and_modes','advertised_schema_keeps_fourteen_tools_and_optional_strategy_defaults'],'The facts parser is a declaration check; real stdio14-tool test supplies runtime evidence, not the parser alone.'),
 docrow('mcp-query-policy','docs/MCP_TOOLS.md',20,'Optional retrieval_strategy local/auto/semantic traverses schema,sanitize,dispatch,point-in-time status; symbol retains array shape',['policy_parameters_are_optional_strict_and_documented','real_stdio_policy_override_and_capability_state','real_mcp_preserves_tool_schema_and_exposes_offline_query_policy','symbol_mode_preserves_legacy_array_and_sanitized_limits','omitted_null_and_explicit_overrides_inherit_project_strategy_through_queries'],'Mock/loopback optional semantic mechanisms do not certify real provider quality; point-in-time capability does not promise next-query readiness.'),
 docrow('mcp-errors','docs/MCP_TOOLS.md',42,'Malformed types/unknown fields and invalid strategy/mode have actual public error paths',['p1c_fourteen_tools_valid_unknown_schema_and_modes','malformed_strategy_and_symbol_incompatibility_are_public_parameter_errors'],'Keep actual tool-is-error deserialize versus sanitize protocol error distinction; no schema-version relabel.'),
 docrow('mcp-install-entry','docs/MCP_TOOLS.md',388,'mcp --project-path,install --force,uninstall CLI declarations and dispatch exist',['crates/cc-server/src/cli.rs:17','crates/cc-server/src/cli.rs:35'],'Only static command/dispatch correspondence reviewed; no product install/uninstall execution claimed or newly required by original018 literal.'),
 docrow('config-defaults','docs/CONFIGURATION.md',210,'auto_index defaults and semantic disabled/network opt-in/query authorization defaults match declarations',['original_CI_facts','crates/cc-model/src/config.rs','crates/cc-model/src/query.rs'],'Managed checker covers enumerated query/auto_index/semantic defaults only; not every configuration field or full runtime parser.'),
 docrow('config-actual-example','docs/CONFIGURATION.md',16,'Actual documented JSON is parsed as ProjectConfig and search defaults equal Rust declaration',['search_examples_and_configuration_match_current_parameter_types'],'Existing test reads actual documentation, not duplicated JSON; selected search examples additionally execute via actual stdio test.'),
 docrow('config-offline-and-authority','docs/CONFIGURATION.md',218,'Semantic disabled leaves ordinary configuration/local index usable; network and query opt-ins are separate',['original_P7_offline_2_packages','original_rollback_disabled_local','original_P7_closeout_lifecycle','crates/cc-server/src/capability_status.rs'],'Default offline syscall observation is Linux process-tree scope; no physical-device/all-platform guarantee and no paid cost assumed zero.'),
 docrow('config-failure-and-design-limits','docs/CONFIGURATION.md',264,'Policy/external credential references and failure explanations separate implemented mechanism from unimplemented zero-on-drop',['crates/cc-semantic/src/policy.rs','docs/CONFIGURATION.md:299','docs/roadmap/code-index-v2/P8-FACTS.md:100'],'No secret/provider call executed; zero-on-drop is explicitly not implemented rather than reported complete.'),
 docrow('facts-managed-entry','docs/roadmap/code-index-v2/P8-FACTS.md',64,'--check distinguishes drift1/invalid2 and records9source hashes; no production schema export claim',['scripts/p8_facts.py:65','scripts/p8_facts.py:144','original_CI_facts'],'No full Rust/SQL semantic parser claim; required四文档 manual scope rows above complement generated entry checks.'),
]
for p in ['scripts/p8_release_evidence.py','docs/roadmap/code-index-v2/P8-RELEASE-EVIDENCE.md','crates/cc-semantic/src/policy.rs']:
    src(p)

clauses = [
 {'id':'V18-tools','literal':'14工具不丢失','evidence':['original_CI_facts','p1c_fourteen_tools_valid_unknown_schema_and_modes','advertised_schema_keeps_fourteen_tools_and_optional_strategy_defaults'],'finding':'Actual registered name set equals14; real stdio calls every tool valid+unknown input paths.','coverage':'accepted_existing_exact_E_scope'},
 {'id':'V18-modes','literal':'旧 mode不变','evidence':['search_modes_optional_nulls_and_utf8_sanitize_are_stable','p1c_fourteen_tools_valid_unknown_schema_and_modes','symbol_mode_preserves_legacy_array_and_sanitized_limits'],'finding':'hybrid/object and symbol/array remain; null/omitted/local and invalid mode paths are checked.','coverage':'accepted_existing_exact_E_scope'},
 {'id':'V18-parameters','literal':'新参数贯穿schema/sanitize/dispatch/status','evidence':['advertised_schema_keeps_fourteen_tools_and_optional_strategy_defaults','malformed_strategy_and_symbol_incompatibility_are_public_parameter_errors','omitted_null_and_explicit_overrides_inherit_project_strategy_through_queries','policy_parameters_are_optional_strict_and_documented','real_stdio_policy_override_and_capability_state'],'finding':'Optional retrieval_strategy documented example→typed sanitize→real requests→policy/status; explicit override and generation nonmutation on rejected args.','coverage':'accepted_existing_exact_E_scope','limit':'Applies to concrete optional query-policy contract; no claim that all conceivable future parameters are tested.'},
 {'id':'V18-offline','literal':'默认无网络无key可用','evidence':['original_P7_offline_2_packages','original_P7_closeout_lifecycle','original_rollback_disabled_local','original_CI_facts'],'finding':'Default and semantic Linux products each4real process trees+2positive controls, zero observed network socket attempts; local retrieval operates with semantic unconfigured/disabled.','coverage':'accepted_existing_exact_E_scope','limit':'Default/no-key policy, not live opt-in quality or unlimited-duration network impossibility; install/uninstall not inferred.'},
 {'id':'V21-schema','literal':'新旧 schema 降级重建','evidence':['original_rollback_version_stages'],'finding':'Actual source-pair schema25→24→25 plus original25backup restore, retained SQL integrity/FK checks,unchanged source/config and5RPC/stage.','coverage':'accepted_existing_exact_E_scope','limit':'Public source revisions, dev correctness profile; not already released package downgrade,arbitrary power-loss or physical-device certification.'},
 {'id':'V21-cache','literal':'cache不误读','evidence':['original_cache_format_space_namespace'],'finding':'3actual Rust format/space/namespace cases plus3active product format999 rejection/restoration seeds223/227/229.','coverage':'accepted_existing_exact_E_scope','limit':'Original limited rollback subcase remains not_run; explicit complementary source-equal actual tests close that narrow scope. Paid cost remainsnull.'},
 {'id':'V21-packages','literal':'默认/semantic 两包','evidence':['original_platform_8_cells'],'finding':'Both packages crossed Linux/macOS and1.95/stable with8private fresh release builds and5RPC/cell.','coverage':'accepted_existing_exact_E_scope','limit':'Release configuration cold-build coverage,not release distribution or OS-cache-cold performance.'},
 {'id':'V21-sdk-msrv','literal':'SDK/MSRV','evidence':['original_platform_8_cells'],'finding':'Original portable-v2 receipts bind rustc1.95.0/1.99.0 and actual platform host/compiler/SDK metadata; same1087E inputs.','coverage':'accepted_existing_exact_E_scope','limit':'Exactly observed Linux x86_64 and macOS aarch64 matrix; nootherOS/SDK configuration claim.'},
 {'id':'V21-docs','literal':'文档事实漂移检测','evidence':['original_CI_facts','search_examples_and_configuration_match_current_parameter_types','policy_parameters_are_optional_strict_and_documented','four_document_scope_rows'],'finding':'Actual originalCI --check passed with9hashes; facts/manualcurrent-vs-history doc mappings anddoc-derived parameter regressions are linked.','coverage':'accepted_existing_exact_E_scoped_documentation_mapping','limit':'Parser support is narrow and fail-closed; no claim all architecture prose has been formally verified.'},
]

evidence = {
 'original_CI_facts':{'record':record(GAP/'current-E-original-CI-facts-archive-readback.json'),'JSON_pointer':'/facts_original_output','result_line':5669,'run_id':37871838887,'job_id':113631480871,'actual_checkout':ci['actual_checkout'],'scope':'Declared docs facts only;9actualE input hashes checked.'},
 'original_CI_39_archive_controls':{'record':record(GAP/'current-E-original-CI-facts-archive-readback.json'),'JSON_pointer':'/release_evidence_unit_controls','scope':'39actualfixturecontrols,not product archive/native quality certification.'},
 'original_platform_8_cells':{'record':record(PR/'platform-e-complete-review/platform-e-cell-review.json'),'JSON_pointer':'/cells','artifact_ids':[i for c in platform['cells'] for i in [c['bundle_artifact_id'],c['raw_artifact_id']]],'run_id':37871838952,'source':E,'schema_specific_manifest':platform['source']['manifest_sha256'],'scope':'8/8 original strict portable-v2 + same-byte raw lineage. Original schema-specific source hash not replaced with4e0.'},
 'original_rollback_version_stages':{'record':record(PR/'recovery-e-review/recovery-e-review.json'),'JSON_pointers':['/version_stages','/retained_database_snapshots','/source_and_configuration_unchanged'],'artifact_id':11592146562,'run_id':37871838952,'source':E},
 'original_cache_format_space_namespace':{'record':record(PR/'recovery-e-review/recovery-e-review.json'),'JSON_pointers':['/cache_scope_resolution','/original_fault_tests'],'artifact_id':11592146562,'source':E},
 'original_rollback_disabled_local':{'record':record(PR/'recovery-e-review/recovery-e-review.json'),'original_members':['p8-full-recovery/actual-version-pair/'+s+'/local.json' for s in ['01-current-build','02-previous-opens-current','03-current-restored','04-original-backup-restored']],'artifact_id':11592146562,'scope':'Disable/unconfigured local retrieval usable; not live-provider outage automatic switching.'},
 'original_P7_offline_2_packages':{'record':record(PR/'p7-e-originals-review/independent-review.json'),'JSON_pointers':['/artifacts/2/details','/artifacts/3/details'],'artifact_ids':[11591113592,11591814707],'run_id':37871838966,'source':E,'scope':'Each4product trees+2positive controls,131raw tracefiles,0observednetwork socket attempts; actual trace replays alreadyaccepted.'},
 'original_P7_closeout_lifecycle':{'record':record(PR/'p7-e-originals-review/independent-review.json'),'JSON_pointer':'/artifacts/1/details/original_lifecycle_case_names','artifact_id':11592301062,'run_id':37871838966,'source':E,'scope':'Unconfigured/disabled/authority/restart/cache/HTTP500 loopback lifecycle; no live quality/billing claim.'},
}

common = {'execution_source':E,'source_tree':ci['actual_tree'],'actual_merged_main':M,'actual_main_tree':merge['actual_tree'],
 'main_applicability':{'record':record(merge_path),'all_12_nonartifact_roots_equal_E':True,'executions_not_relabelled_as_M':True},
 'counts':{'done':163,'remaining':29},'formal_task_completion':False,'status_action':'no_change',
 'native_test_or_product_executions':0,'source_task_index_ref_or_remote_edits':False,
 'original_gap_record':record(GAP/'gap-report.json')}
mapping = {**common,'schema':'P8-018-existing-E-four-document-and-V18-V21-mapping-v1',
 'original_task':tasks['P8-018'],'original_validations':[v for v in gap['validation_literals'] if v['code'] in ['V18','V21']],
 'four_document_scope_rows':docs,'validation_clause_mapping':clauses,'evidence_index':evidence,
 'test_original_execution_index':test_receipts,'source_inputs':source,
 'own_scope_assessment':'Named documentation facts,history/current distinction,installation-entry instructions andoriginalV18/V21 clauses now have explicit existing-evidence mappings; no new product defect or new native execution is established by this read-only review.',
 'remaining_formal_conditions':['P8-017 original dependency staysopen; this mapping cannot close it.','Complete immutable raw/report publication and benchmarks navigation remain separate; priorpending publication records are preserved.','This scoped documentation mapping awaits independent narrowreview; formal task status staysunchanged.'],
 'not_added_requirements':['No product install/uninstall execution requirement is added from a title; static command/dispatch evidence is labelled static.','No live-provider/paid/G8/holdout experiment added.','No additional scale/runtime rerun for this mapping.'],
 'manual_review_boundary':'Four documents read for original018 count/schema/tool/default/install/failure/design-history obligations; other architecture algorithms,allconfigurationvalues andhistoric performance claims are not newly recertified.'}

owner_rows = [
 {'id':'rust-quantile-owner','owner':'benchmark::statistics::nearest_rank','callers':['distribution','quantile_interval','bench::run_benchmark_named','bench::IncrementalStats::from_durations','bench duration stats','report::compute_summary'], 'source_paths':['crates/cc-eval/src/benchmark/statistics.rs','crates/cc-eval/src/bench.rs','crates/cc-eval/src/report.rs'],'classification':'shared_owner_already_implemented','necessary_compatibility':'Caller-specificms/us units andempty-wire0 stay atlegacycallers; versionedemptyOption remainsnull.','existing_regression':['distribution_and_interval_keep_one_nearest_rank_convention','legacy_report_percentiles_preserve_group_denominators_and_empty_wire_values'],'action':'Keep existingE implementation; oldcleanup claim bench/reportremain duplicate is historical,notcurrent.'},
 {'id':'runtime-python-ns','owner':'scripts/p8_runtime.py::latency_summary','callers':['run report.latency','run report.latency_by_operation'],'source_paths':['scripts/p8_runtime.py','crates/cc-eval/src/bin/p8-runtime-statistics.rs'],'classification':'separate_legacy_ns_estimator_requires_classification_before_change','necessary_compatibility':'Raw ns andlegacy ns summary cannotbe reconstructed byRust integerus×1000; preserveallterminaldenominator,null/empty andwireprecision.','existing_regression':['Accepted E runtime6sealedoriginals andexactRuststats replay retained separately; not proof a futureconsolidation is wire-identical.'],'action':'No replacement now; aftercurrentmatrix,decide explicitcompatibility adapter versus same-unitsharedowner andonlythen scope differentialregression.'},
 {'id':'scale-nontail-summary','owner':'benchmark::p8_scale::distribution','callers':['scale reporttiming aggregates'],'source_paths':['crates/cc-eval/src/benchmark/p8_scale.rs'],'classification':'different_n_min_max_mean_statistic','necessary_compatibility':'No p95/p99/tail estimate promised.','existing_regression':['Original b107 shard review andpendingfullmatrix; not needed to prove deletion because no deletion proposed.'],'action':'Keep; functionname collisionis not duplicatequantilealgorithm.'},
 {'id':'versioned-quality-owner','owner':'benchmark::metrics::score','callers':['benchmark::report::summarize'],'source_paths':['crates/cc-eval/src/benchmark/metrics.rs','crates/cc-eval/src/benchmark/report.rs','crates/cc-eval/src/benchmark/schema.rs'],'classification':'one_dispatch_with_explicit_OceCompat_and_Native_profiles','necessary_compatibility':'Distinctpathpatterns/nativegroups/alternatives/spansandprofiledenominators areintentional.','existing_regression':['ExistingCI benchmarktestgroups,notanewprofile-equivalenceordeleteproof.'],'action':'Keepsemanticallydistinctprofiles; do notmergeby metricname.'},
 {'id':'legacy-evalcase-assertions','owner':'runner::check_assertions andlegacy expected_symbols path','callers':['TOML EvalCase runner'],'source_paths':['crates/cc-eval/src/runner.rs','crates/cc-eval/src/types.rs','crates/cc-eval/src/corpus.rs'],'classification':'legacy_assertion_contract_distinct_from_versioned_Query','necessary_compatibility':'Legacy symbolnameRecall@5/MRR+per-casethreshold differsfromgroup/path/spanqueryscore.','existing_regression':['No explicitfullwireadapter/deletionmappingprovedbythisstaticinventory.'],'action':'Keepuntilanexplicitsemanticadapterandoriginalcorpusregressionisspecified.'},
 {'id':'versioned-schema-owner','owner':'Rust Suite/Query/Row+schemars','callers':['cc-eval schema CLI'],'source_paths':['crates/cc-eval/src/benchmark/schema.rs','crates/cc-eval/src/bin/cc-eval.rs'],'classification':'single_schema_source_in_read_path','necessary_compatibility':'GeneratedJSONschemasareoutputs,notanotherhandwrittenowner.','existing_regression':['CurrentCIversionedbenchmarkcontracts; staticownerproofdoesnotcertifyeveryschemafileinrepository.'],'action':'KeepRustowner; inventoryotherdomainsbeforeclaimallmultischemasgone.'},
 {'id':'compat-lock-adapter','owner':'scripts/p8_compat.py lock/normalization adapter','callers':['compatibility evidence/legacy inputnormalization'],'source_paths':['scripts/p8_compat.py'],'classification':'separate_lock_envelope_not_second_qualityscorer','necessary_compatibility':'Pinsactualoriginalschema/scorerbytes andparsedQuerynormalization; externalwiremayneedadapter.','existing_regression':['No deletionproposed; originalsourceguardandcompatcontrolsremainhistorical/currentpertheiridentities.'],'action':'Keepunless specifictemporarypathisprovenunnecessary.'},
 {'id':'test-only-percentiles','owner':'p7_worker_contention andincremental_write_bench localdiagnosticclosures','callers':['test/benchmarkdiagnosticoutput'],'source_paths':['crates/cc-eval/tests/p7_worker_contention.rs','crates/cc-db/tests/incremental_write_bench.rs'],'classification':'testdiagnostics_notsecondproductqualityowner','necessary_compatibility':'Differenttestprotocols/units/denominatorsmustnotbe silentlyreplaced.','existing_regression':['Acceptedactualbackfill usesp7_worker_contention; thisdoesnotestablishbothhelpersaretemporary.'],'action':'No deletionbyname; specificcleanuprequirescompatibilityandtargetedregressiondecision.'},
]
owner_report = {**common,'schema':'P8-017-scoped-owner-compatibility-and-regression-inventory-v1','original_task':tasks['P8-017'],
 'peer_static_inventory':record(owner_path),'peer_gap_wording_review':owner_peer,'owners':owner_rows,
 'actual_deleted_duplicate_implementation_regression':{n:test_receipts[n] for n in ['distribution_and_interval_keep_one_nearest_rank_convention','legacy_report_percentiles_preserve_group_denominators_and_empty_wire_values']},
 'source_inputs':{p:source[p] for r in owner_rows for p in r['source_paths']},
 'remaining_scope':'Thislistisexactforthereadownerpaths,notanexhaustivecc-index/cc-search/cc-evalallbranchinventory. Remainingclassificationanddeletion-specificregressionmapstayopen;noadditionalbugorblanketrefactorisasserted.',
 'decision_timing':'No fixedE change duringcurrentfullscaleacceptance; rootdecideslaterproductchangeonlyafterreview.'}

for name,obj in [('P8-018-four-document-existing-evidence-map.json',mapping),('P8-017-owner-compatibility-regression-inventory.json',owner_report)]:
    p=OUT/name
    with p.open('xb') as f:f.write((json.dumps(obj,ensure_ascii=False,sort_keys=True,indent=2)+'\n').encode())
    print(json.dumps(record(p),ensure_ascii=False))
for p,r in list(read_records.items()):
    assert digest(Path(p).read_bytes()) == {k:r[k] for k in ['bytes','sha256','git_blob_sha1']}, p
assert git('rev-parse','HEAD').decode().strip()==E
receipt={**common,'schema':'readonly-mapping-preparation-receipt-v1','files':list(read_records.values()),'all_read_inputs_unchanged_after':True,'exact_E_source_files_read':len(source),'original_CI_test_names_bound':len(test_receipts),'new_tests_or_native_invocations':0,'output_files':[str(OUT/n) for n in ['P8-018-four-document-existing-evidence-map.json','P8-017-owner-compatibility-regression-inventory.json']]}
p=OUT/'preparation-receipt.json'
with p.open('xb') as f:f.write((json.dumps(receipt,ensure_ascii=False,sort_keys=True,indent=2)+'\n').encode())
print(json.dumps(record(p),ensure_ascii=False))
