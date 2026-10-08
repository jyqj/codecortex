"""Read existing accepted records and prepare append-only task drafts; no execution or repo edits."""
import collections
import datetime
import hashlib
import json
from pathlib import Path
import subprocess
import tarfile

BASE = Path('/dev/shm/a217aaae3bde')
ROOT = BASE / 'codecortex'
OUT = BASE / 'runtime-review/final-ten-draft'
OLD = '599a7050e7d52b5b7b93975c419138e175b3f754'
NEW = '29682890c89511dd6f477a6bf48bd969aa1537af'
TASK_PATH = 'docs/roadmap/code-index-v2/tasks.json'
CHECKPOINT = 'artifacts/checkpoints/p8-completion-20261009/execution-599/'
BENCH = 'artifacts/benchmarks/p8-completion-20261009/'
CHECKLIST = BASE / 'platform-review/round6-final-ten-status-review-checklist.json'
PROOF = BASE / 'platform-review/round6-execution-applicability-proof.json'
ALLOWED = {'status', 'evidence', 'implementation_notes'}

def digest(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()

def read(path):
    return json.loads(Path(path).read_text())

def ref(path):
    path = Path(path)
    result = {'local_path': str(path), 'bytes': path.stat().st_size, 'sha256': digest(path)}
    if path.is_relative_to(ROOT):
        result['repository_path'] = str(path.relative_to(ROOT))
    return result

def write(name, value):
    path = OUT / name
    encoded = json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True) + '\n'
    if path.exists():
        assert path.read_text() == encoded, ('existing draft differs; preserve it', path)
    else:
        path.write_text(encoded)
    return ref(path)

start_head = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
task_before = (ROOT / TASK_PATH).read_bytes()
frozen_tasks = (BASE / 'codecortex-round6-frozen' / TASK_PATH).read_bytes()
assert task_before == frozen_tasks
plan = json.loads(task_before)
tasks = {task['id']: task for task in plan['tasks']}
checklist = read(CHECKLIST)
proof = read(PROOF)
assert digest(ROOT / TASK_PATH) == checklist['current_tasks_sha256']
assert proof['old_execution_head'] == OLD and proof['new_execution_head'] == NEW
assert proof['source_identity']['complete_input_count'] == 1087
assert proof['source_identity']['all_git_modes_blobs_and_disk_bytes_equal']
assert proof['validation_identity']['changed'] == ['scripts/p8_runtime.py', 'scripts/tests/test_p8_runtime.py']
assert proof['unresolved_source_applicability_blockers'] == []
counts = collections.Counter(task['status'] for task in plan['tasks'])
assert len(tasks) == 192 and counts['done'] == 163
for item in checklist['tasks']:
    task = tasks[item['task_id']]
    assert {key: value for key, value in task.items() if key not in ALLOWED} == item['original_definition']
    assert task['status'] == 'in_progress'

catalog = {}
def add(key, path, source, artifact_ids=(), run=None, state='accepted_scoped'):
    record = read(path)
    catalog[key] = dict(ref(path), source_commit=source, artifact_ids=list(artifact_ids), workflow_run=run, state=state)
    return record

applicability = add('applicability', PROOF, NEW, state='accepted_scoped_source_applicability_only')
add('historical_observer_addendum', BASE / 'platform-review/round6-historical-observer-applicability-addendum.json', NEW, state='accepted_scoped_source_applicability_only')
for name, item in proof['old_original_review_records'].items():
    assert digest(ROOT / item['path']) == item['sha256'], name

scale_mapping = add('scale_mapping', BASE / 'scale-round5-review/original-acceptance-mapping.json', OLD, run=37835810882, state='original_requirements_not_completion')
scale_state = add('scale_build', BASE / 'scale-round5-review/validated/state.json', OLD, [11576810573], 37835810882, 'accepted_build_only')
scale_ids = [11577045484, 11576619839, 11577518538, 11580138693]
partial_count = 0
for aid in scale_ids:
    r = add('scale_shard_' + str(aid), BASE / 'scale-round5-review/validated/shards' / str(aid) / 'review.json', OLD, [aid], 37835810882, 'accepted_one_original_shard_only')
    assert r['source_commit'] == OLD
    partial_count += r['sample_count']
assert partial_count == 41

runtime = read(BASE / 'runtime-review/round6-runtime-six-profile-closeout.json')
add('runtime_six_index', BASE / 'runtime-review/round6-runtime-six-profile-closeout.json', NEW, [p['artifact_id'] for p in runtime['profiles']], 37844310853)
for p in runtime['profiles']:
    r = add(p['profile'], ROOT / BENCH / 'runtime-296' / p['profile'] / 'independent-review.json', NEW, [p['artifact_id']], 37844310853)
    assert r['head'] == NEW and r['unresolved_blockers'] == []
    assert digest(catalog[p['profile']]['local_path']) == p['accepted_review']['sha256']

life = add('lifecycle', ROOT / (CHECKPOINT + 'lifecycle-independent-review.json'), OLD, [11576107053], 37835809247)
add('lifecycle_cargo', ROOT / (CHECKPOINT + 'lifecycle-cargo-selection-addendum.json'), OLD, [11576107053], 37835809247)
fault = add('fault', ROOT / (CHECKPOINT + 'fault-recovery-independent-review.json'), OLD, [11575083478], 37835809266)
rollback = add('rollback', ROOT / (CHECKPOINT + 'recovery-independent-review.json'), OLD, [11575083478], 37835809266)
platform = add('platform', ROOT / (CHECKPOINT + 'platform-eight-cells-independent-review.json'), OLD, run=37835809266)
catalog['platform']['artifact_ids'] = [aid for c in platform['cells'] for aid in [c['bundle_artifact_id'], c['raw_artifact_id']]]
add('platform_collector', ROOT / (CHECKPOINT + 'platform-collector-equivalence-review.json'), OLD, [11577615032], 37835809266)
gates = add('gates', ROOT / (CHECKPOINT + 'gates-independent-review.json'), OLD, [11574928381], 37835809216)
add('old_p7', ROOT / (CHECKPOINT + 'p7-closeout-originals-independent-review.json'), OLD, state='historical_regressions_not_current296_substitute')
add('old_engineering', ROOT / (CHECKPOINT + 'p7-engineering-independent-review.json'), OLD, state='historical_regressions_not_current296_substitute')

# Read four already-preserved raw records only to resolve the original disable
# fallback wording. This neither starts a product nor reruns an acceptance test.
archive = BASE / 'platform-review/raw-preservation-599/platform-recovery-original-raw.tar.gz'
fallback = []
with tarfile.open(archive, 'r|gz') as tar:
    for member in tar:
        if '/actual-version-pair/' not in member.name or not member.name.endswith('/local.json'):
            continue
        data = tar.extractfile(member).read()
        record = json.loads(data)
        retrieval = record['capabilities']['retrieval']
        assert retrieval['dense_state'] == 'disabled' and retrieval['semantic_state'] == 'not_configured'
        assert retrieval['default_strategy'] == 'local' and retrieval['local_state'] == 'available'
        assert retrieval['query_encoding']['configured_opt_in'] is False
        assert retrieval['query_encoding']['network_authorized'] is False
        assert retrieval['query_encoding']['reason'] == 'semantic_disabled'
        assert any(hit['file_path'] == 'src/lib.rs' and hit['name'] == 'p8_rollback_source_marker' for hit in record['search'])
        fallback.append({'member': member.name, 'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest(),
                         'observed': {'dense_state': 'disabled', 'semantic_state': 'not_configured',
                                      'configured_opt_in': False, 'network_authorized': False,
                                      'default_strategy': 'local', 'local_state': 'available',
                                      'actual_hit': 'src/lib.rs:p8_rollback_source_marker'}})
assert len(fallback) == 4
fallback_ref = write('P8-016-existing-raw-locators.json', {'kind': 'read_existing_preserved_raw_only', 'artifact_id': 11575083478,
    'source_commit': OLD, 'archive': ref(archive), 'records': fallback, 'execution_rerun': False,
    'scope': 'disable 后 local 检索可用；不扩写成 live provider 掉线自动切换。'})
catalog['disable_local_raw'] = dict(fallback_ref, source_commit=OLD, artifact_ids=[11575083478], state='existing_raw_field_mapping')

pending = [
    {'id': 'scale_150_shards', 'status': 'pending', 'observed_accepted': 4, 'required': 150, 'note': '仅固定599原件；旧6e3/6956不计数。'},
    {'id': 'scale_1500_samples', 'status': 'pending', 'observed_accepted': 41, 'required': 1500, 'note': '50组×全局30 repetitions；每环境stratum不自动拥有N30。'},
    {'id': 'original_scale_combine_and_ci_aggregate', 'status': 'pending', 'accepted_result': None, 'note': '完整原combine/CIaggregate、唯一覆盖及分层结果尚未得到。'},
    {'id': 'pr165_actual_ci', 'status': 'pending', 'accepted_result': None, 'note': '纯归档修复的真实CI与历史guard必须通过；不能用静态迁移审查替代。'},
    {'id': 'current296_p7_mechanism', 'status': 'pending', 'accepted_result': None},
    {'id': 'current296_p7_semantic', 'status': 'pending', 'accepted_result': None},
    {'id': 'fresh_main_and_final_head_required_gates', 'status': 'pending', 'accepted_result': None, 'note': 'root需读取新main与最终PR实际HEAD、真实CI/P7及必要合并门，权限不足不能推断无门。'},
    {'id': 'final_pr_review_and_merge', 'status': 'pending', 'accepted_result': None, 'note': '最终仅状态/证据/说明及既有导航派生视图；合并结果尚未发生。'},
]
scale_refs = ['scale_mapping', 'scale_build'] + ['scale_shard_' + str(aid) for aid in scale_ids]
specs = {
 'P8-005': {
  'refs': scale_refs, 'own': 'pending_complete_original_matrix',
  'steps': ['固定599 release来源/build11576810573已绑定；五档×30完整覆盖尚待150/150片与1500/1500样本，当前仅4片41样本。',
            '每规模原counts包含files/symbols/chunks/各edge表；vectors须保持disabled/null状态。完整汇总尚待原combine。'],
  'acceptance': '保留工作量与环境分层；不能把异质host或不同工作量直接计算倍数。完整五档与原回归尚未通过，当前不得done。',
  'summary': '固定599规模执行run37835810882的release build和1k/5k/10k/50k各rep0原件已独审，合计4/150片、41/1500样本；完整五档矩阵及原combine仍pending。',
  'limits': ['不主张严格paired因果提速或稳定p99；环境分层如实保留。', 'semantic disabled、vectors unavailable/null不能写成0向量吞吐或live-provider规模。', '原acceptance_subgates是不可变历史快照，full_1k_5k_10k_50k_100k_scale_acceptance=open等原值永久保留，新验收另附。'],
 },
 'P8-006': {
  'refs': scale_refs, 'own': 'pending_complete_original_matrix',
  'steps': ['原计划覆盖cold/no_op/body/api/config/batch_1/10/100/1000及fanout_1/4/16/64/128；须含真实首次incomplete后完成闭包，当前完整矩阵pending。',
            '逐样本保留IndexReport.phase_timing/build_timing与index/outer/full_control/parity计时；嵌套阶段不重复相加。'],
  'acceptance': '每阶段完整原15表canonical parity、integrity/FK与公开查询/手写target见证；中间incomplete及rebuild不能丢弃，比较前不修增量侧。当前只验部分片，完整接受pending。',
  'summary': '固定599的4份原片41样本已按源/驱动/二进制绑定复验；完整增量、批量和fanout覆盖、150/150片、1500/1500样本及原最终combine仍pending。',
  'limits': ['registered 200/1024与fanout8/128不改；native5h/512MiB、oracle16GiB canonical/8GiB scratch/5m rows/2MiB cache不改。', '超预算或未闭合必须保留原status；不截断对账、不只汇总成功阶段。', 'P8-005未正式完成时本任务不得done；release_certification=not_run、G8=not_evaluated保持。'],
 },
 'P8-007': {
  'refs': ['mixed-c1','mixed-c4','mixed-c8','mixed-c16','backfill','runtime_six_index'], 'own': 'accepted_scoped_original296',
  'steps': ['新296四档C1/4/8/16各900请求全部成功，实际RPC最大1/4/7/12；C4/8/16实际read/build overlap。独立原worker fake-provider backfill为seeds7/19/43×C1/4/8/16×quiet/held共24cell、768请求。',
            '原offered/dispatch/queue/service/end-to-end记录、全部唯一终态和尾部保留；实际统计重放字节相同，终点队列与owned cleanup已核对。'],
  'acceptance': '已观测窗口内全部请求终态齐全、无死锁/饥饿或隐藏timeout，负载统计和原15表oracle真实重放一致。原V11相关取消/deadline/缓存等回归仍需当前CI/P7闭合。',
  'summary': '固定296/run37844310853的四mixed原件与独立fake-provider backfill已完整复验，1087源及exact9观察器绑定，原sealed raw和所有失败历史保留。',
  'limits': ['配置并发上限不是实际峰值：C1/4/8/16实际RPC峰1/4/7/12。', '默认mixed没有semantic backfill；backfill由另一个原worker执行证明，不能称为同进程live-provider混合压测。', 'fake provider无真实网络吞吐/付费结论；2s/5s/100ms原控制不改；不主张稳定p99或性能提升。'],
 },
 'P8-008': {
  'refs': ['lifecycle','lifecycle_cargo'], 'own': 'accepted_scoped_original599',
  'steps': ['固定599 artifact11576107053分层30 cold build、400 process reopen、400 warm uncached、400 result-cache hit；OS page cache状态保留未清除/unknown。',
            '1230/1230 attempts与1200源核对查询均保留，无best-of；431个owned进程正常关闭，原evaluator重放等价。'],
  'acceptance': '全部N、分布及原95%次序统计CI保留。cold N30的p95/p99上界null及inconclusive_insufficient_tail_samples如实保留，不伪造有限尾部结论。',
  'summary': '599生命周期原件已按固定源、构建/evaluator收据、分层计数、实际查询及原生重放接受；599→296适用性由全1087相同产品输入与不变生命周期观察器闭包单独证明，原receipt仍为599。',
  'limits': ['process cold不等于OS缓存冷/磁盘冷；只有报告的四层。', 'CI方法保留原IID假设；N30冷建不能主张稳定p99或统计性能改善。', '不可将原599源、原binary或source_root改标为296。'],
 },
 'P8-009': {
  'refs': ['lifecycle','lifecycle_cargo'], 'own': 'accepted_scoped_original599',
  'steps': ['1261个资源stage快照区分client native、server SELF与native process-tree，PID/父子/namespace身份核对；原ps unavailable另列，不冒充server。30关闭DB的90个物理对象按device/inode去重，128901120B物理总量，FTS逻辑页不重复加总。',
            'reported/estimated/input/output tokens/requests_billed均保持null及disabled-provider原因，network_filter=not_measured。'],
  'acceptance': '角色、单位、物理/逻辑归属已独立核对；unavailable不填0，不相加重叠角色与非同时峰值。完整旧回归仍待当前CI/P7。',
  'summary': '599原生命周期/资源账本、native树关联、SQLite integrity/FK/dbstat与物理文件长度通过只读独审；原不可用行和成本null保留。原记录无损派生包另存，省略仅收据绑定ELF。',
  'limits': ['这些是stage snapshots，sampling_interval_ms=null，不构成连续峰值或全系统内存总量。', 'server SELF与tree采样时刻可能不同；不能把两者或runner替代观测叠加。', '未知外部服务/LSP/容器与实付费用不补0；不据此宣称免费或无网络。'],
 },
 'P8-010': {
  'refs': ['soak','backfill','runtime_six_index'], 'own': 'accepted_scoped_original296',
  'steps': ['同一296 stdio product实际3600.037999068秒；3601点每秒计划、3601成功，持续六类修改并完成200真实branch切换和25 catalog compaction；worker/backfill复用边界结合原worker独立768请求记录。',
            '终点增量侧未补修，与fresh full全部15表原生oracle重放相同；资源/队列/统计原raw可重算。'],
  'acceptance': '3596资源点maxgap1.031181477s<原5s；RSS warm92676096B→tail109080576B低于原149399552B阈值；终点CPU/async/admitted/pins=0；owned worker/sampler/product均确认停止。',
  'summary': '新296 artifact11585132175在原1h/3601点/1000文件协议完成并独立重放，17 build+2110 runtime seal全部通过，统计字节相同、完整终点一致。',
  'limits': ['soak配置C4但实际RPC峰1且无read/build overlap；不能称饱和C4或复用mixed峰值。', '原中位数增长规则与有限1h窗口通过，不是统计学无泄漏或任意时长不增长证明。', '默认soak非live semantic provider；backfill原件作用域分开，不重标599旧soak。'],
 },
 'P8-011': {
  'refs': ['fault','rollback','old_p7','old_engineering'], 'own': 'accepted_scoped_original599',
  'steps': ['599 artifact11575083478包含kill/restart、SQLite busy、删除源本地场景；active seeds223/227/229分别执行真实loopback HTTP断开、缓存payload损坏、格式999拒读恢复、并发换库；四原Rust crash/rename边界控另行绑定。',
            '495-file完整封存清单、原RPC/stdout/stderr/退出与故障DB均保留；预期SIGKILL缺一未完成回应为明确场景，正常退出严格拒坏JSON/重复/缺失response。'],
  'acceptance': '原raw已证无假ready/删除复活、换库incarnation隔离与恢复完整性；每seed观测6个loopback provider请求，paid_cost/actual_paid_cost=null，费用边界清楚。',
  'summary': '599故障/恢复完整CLI与七个原生产故障测试、三个active seeds已获独立own-scope接受；旧有限subreceipt的not_run原封保留，由同源更完整原件映射覆盖，不能改写历史。',
  'limits': ['原SIGKILL本地pending-request边界与四Rust内部crash边界分别证明，不相互替代。', 'loopback fake provider不是liveprovider；null付费不等于实付0，不主张任意断电/设备故障。', '恢复使用声明dev profile，不能写成release冷性能；当前296 P7 mechanism/semantic仍需真实原门。'],
 },
 'P8-012': {
  'refs': ['platform','platform_collector','old_engineering'], 'own': 'accepted_scoped_original599',
  'steps': ['599 Linux/macOS×MSRV1.95/stable×default/semantic全部八格与16份原bundle/raw逐一核对；当次stable实际rustc1.99.0，Linux x86_64与macOS aarch64按实际记录。',
            '每格全新private target、Cargo fresh=false、release真实profile、exact features/producer/toolchain/SDK/1087源+4observer及stdio实际源码命中通过strict portable-v2；八格collector为8/0/0。'],
  'acceptance': '八格真实冷构建覆盖完整；macOS SDK/声明MSRV边界按该runner实测接受，缓存命中不替冷建。原单机其余七格not_run及其exit2保留，由最终完整collector闭合。',
  'summary': '599八格原件和独立collector等价审查已接受；collector仅source_root路径归一，其他报告字段逐字一致。平台、compiler host与包feature范围无缩减。',
  'limits': ['只代表当次Linux x86_64/macOS aarch64及1.95.0/实际stable1.99.0，不扩写成所有架构/未来stable。', '产品冷构建不表示依赖下载/OS page cache冷；工程回归的warm target不可替代该矩阵。', 'P8-011及最终CI/文档事实漂移门仍是原硬条件。'],
 },
 'P8-013': {
  'refs': ['gates'], 'own': 'accepted_scoped_original599',
  'steps': ['599 artifact11574928381六个原nonignored gate测试及六次原CLI重放覆盖quality/perf/lock与invalid policy。',
            'quality/latency failed→exit1，零分母/不足样本inconclusive→exit1，raw-lock/bad-policy invalid_measurement→exit2；原raw与机器报告保留，坏policy拒覆写原报告。'],
  'acceptance': '故意红线失败真实非零，inconclusive不转passed；原报告重放仅必要路径迁移，其余结果/原因/raw不变。V03 scorer goldens与V04广义传输回归仍随当前CI/P7闭合。',
  'summary': '原599六项失败退出与收据/raw保留控制获得独立接受，invalid/failed/inconclusive状态分别保留，未改变产品性能或质量认证标准。',
  'limits': ['fixture是明确synthetic；这些是失败门行为证明，不是产品质量、heldout或性能改进证明。', '零plan library项由原exact nonignored测试binary/source/log证明，无新Cargo代替。', '标题“最终认证”不扩写成G8或release批准。'],
 },
 'P8-016': {
  'refs': ['rollback','fault','disable_local_raw','platform','platform_collector','old_p7'], 'own': 'accepted_scoped_original599',
  'steps': ['实际599 schema25→原277f2490 schema24旧binary受控重建→599 schema25恢复→原备份恢复；四原local.json明确dense disabled、semantic not_configured、network_authorized=false，local available且命中原源码marker。cache格式/space/namespace由原三Rust测试及三个active seed格式999拒读→恢复完成证明。'],
  'acceptance': '用户source/config哈希保持，四DB snapshot integrity/FK通过；旧binary不误读新schema、cache不跨版本/空间/namespace。默认/semantic包发布构建范围另由599八格真实release证据配合。',
  'summary': '599/277f2490真实两revision的schema往返与备份恢复已独审；disable后local检索可用有四原raw字段和实际命中；原有限rollback的active-reader not_run不改，由同源完整active/raw证据补齐映射。',
  'limits': ['两公开source revisions不是已发布包版本，恢复dev profile不是release性能。', 'disable后local可用不能扩写成liveprovider掉线自动切换；实付费用仍null。', '不覆盖任意断电/物理设备或live-provider认证；P8-012/013及P7-020原硬依赖不变。'],
 },
}

validation_lines = {}
for line in (BASE / 'codecortex-round6-frozen/docs/roadmap/code-index-v2/06-VALIDATION.md').read_text().splitlines():
    if line.startswith('| V'):
        columns = [part.strip() for part in line.strip('|').split('|')]
        validation_lines[columns[0]] = {'name': columns[1], 'minimum_scenarios': columns[2], 'level': columns[3]}

common_note = '本草案时formal task completion=false；完整150/150片、1500/1500样本及原combine，PR165实际CI、296 P7 mechanism/semantic、fresh main/最终PR必要门与合并仍pending，当前163 done/29 remain。原历史失败与not_run证据全部保留。'
mapping, drafts = [], []
for item in checklist['tasks']:
    tid = item['task_id']; task = tasks[tid]; spec = specs[tid]
    assert len(spec['steps']) == len(task['steps'])
    refs = spec['refs'] + ['applicability']
    if 'old_p7' in refs:
        refs.append('historical_observer_addendum')
    criteria = []
    for i, (literal, explanation) in enumerate(zip(task['steps'], spec['steps'])):
        criteria.append({'original_field': 'steps', 'index': i, 'literal': literal, 'mapped_observation_zh': explanation, 'evidence_ids': spec['refs'], 'scope_state': spec['own']})
    for i, literal in enumerate(task['acceptance']):
        criteria.append({'original_field': 'acceptance', 'index': i, 'literal': literal,
            'mapped_observation_zh': spec['acceptance'] if i == 0 else common_note,
            'evidence_ids': spec['refs'] if i == 0 else ['old_p7','old_engineering'],
            'scope_state': spec['own'] if i == 0 else 'pending_current_regressions_and_dependencies'})
    validations = [{'code': code, 'original_minimum': validation_lines[code], 'mapping': '上述own-scope原记录只覆盖本任务情景；广义V套餐的相关旧回归需由当前CI/P7原门闭合，不据本条宣称整个套餐已通过。', 'final_package_acceptance': 'pending'} for code in task['validations']]
    mapping.append({'task_id': tid, 'title': task['title'], 'original_definition': item['original_definition'],
        'original_definition_sha256_from_checklist': item['immutable_definition_sha256'], 'status_now': task['status'],
        'formal_task_completion': False, 'own_scope_state': spec['own'], 'criteria': criteria, 'validation_mapping': validations,
        'hard_dependencies': item['hard_dependencies'], 'evidence_ids': refs, 'scope_limits_zh': spec['limits'],
        'pending_final_gate_ids': [p['id'] for p in pending], 'status_action_now': 'keep_in_progress'})
    source = NEW if tid in ['P8-007','P8-010'] else OLD
    evidence = {'status': 'accepted_own_scope_formal_completion_pending' if tid not in ['P8-005','P8-006'] else 'partial_original_matrix_not_complete',
        'source': source, 'applicable_fixed_execution': NEW, 'summary': spec['summary'],
        'artifacts': [catalog[k]['repository_path'] for k in refs if 'repository_path' in catalog[k]],
        'review_references': {k: catalog[k] for k in refs}, 'limitations': spec['limits'],
        'formal_task_completion': False, 'pending_final_gate_ids': [p['id'] for p in pending],
        'historical_scope_preserved': '所有599原件保持599 source/binary/runner/seal身份；适用性单独引用1087产品输入+相应不变观察器闭包，不声称136 validation全部等于599。',
        'pending_note': common_note}
    if tid == 'P8-005':
        evidence['historical_acceptance_subgates_note'] = '任务原acceptance_subgates所有字段是不可变历史快照；包含open的原值不修改。将来完整规模验收作为新的追加evidence记录，不能回写旧子门。'
    notes = '\n本轮固定来源验收准备：' + spec['summary'] + ' ' + ' '.join(spec['limits']) + ' ' + common_note
    drafts.append({'task_id': tid, 'expected_current_status': 'in_progress', 'status_now': 'in_progress',
        'status_action_now': 'no_change', 'final_status_not_prepopulated': None,
        'evidence_append_draft': evidence, 'implementation_notes_append_draft': notes,
        'finalization_instruction': '这只是草案。root补入每个pending门的实际固定HEAD/run/raw/review结果并独审原依赖全done后，才可将status改done；保留所有旧evidence/notes并追加，不得用未来成功预填。'})

original = write('original-task-snapshot.json', {'task_file': ref(ROOT / TASK_PATH), 'checklist': ref(CHECKLIST),
    'source_commit': NEW, 'counts': dict(counts), 'done': 163, 'remaining': 29,
    'immutable_definition_base': checklist['original_definition_base'],
    'immutable_definition_projection_sha256': checklist['complete_immutable_192_definition_projection_sha256'],
    'allowed_final_task_fields': sorted(ALLOWED), 'definition_objects_equal_to_independent_checklist': True,
    'tasks': [{'id': item['task_id'], 'immutable_definition': item['original_definition'],
               'prior_evidence_sha256_from_checklist': item['prior_evidence_sha256'],
               'current_evidence': tasks[item['task_id']]['evidence'],
               'implementation_notes_present': 'implementation_notes' in tasks[item['task_id']],
               'current_implementation_notes': tasks[item['task_id']].get('implementation_notes')} for item in checklist['tasks']]})
evidence_ref = write('accepted-evidence-index.json', {'sources': catalog, 'no_acceptance_rerun': True,
    'scope': 'Existing accepted records actually read/hashed. Partial scale records remain partial; fixed-source applicability is not new execution.'})
map_ref = write('final-ten-acceptance-mapping.json', {'schema_version': 1, 'prepared_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
    'formal_task_completion': False, 'counts': {'done': 163, 'remaining': 29}, 'closure_order': checklist['closure_order'],
    'original_snapshot': original, 'accepted_evidence_index': evidence_ref, 'pending_final_gates': pending, 'tasks': mapping})
draft_ref = write('task-append-drafts.json', {'schema_version': 1, 'formal_task_completion': False, 'apply_now': False,
    'allowed_final_task_fields': sorted(ALLOWED), 'top_level_navigation_and_four_derived_views': 'root最后按既有生成器处理，本草案不修改',
    'counts_now': {'done': 163, 'remaining': 29}, 'conditional_only_after_exact_ten_close': {'done': 173, 'remaining': 19},
    'tasks': drafts})

findings = write('independent-findings.json', {'formal_task_completion': False, 'current_counts': {'done': 163, 'remaining': 29},
    'actual_pending_blockers': pending,
    'own_scope_coverage_review': '既有八个own-scope接受记录与原steps/acceptance均有明确映射；本轮仅读取记录与四个原local.json定位，没有新实验或放宽标准。005/006完整矩阵仍不具备最终验收。',
    'preserve_and_avoid_overclaim': [
        'P8-005 acceptance_subgates不是可更新状态字段：含open/not_established/unknown全部保留，完整新验收只能另附evidence。',
        '新296六runtime已实际验过；其余599保持原身份，引用适用性桥。134/136 validation不变并不等于136/136不变。',
        'mixed配置C1/4/8/16实际RPC峰1/4/7/12，soak配置C4实际峰1无overlap；fake-worker backfill是独立进程协议。',
        '生命周期OS缓存未清；cold N30尾部CI上界null/inconclusive保留，不能写成有界稳定p99。',
        '资源是有限stage快照，不能宣称连续峰值或加总重叠runner/server/tree；未知服务及付费数据保持null。',
        '故障是loopback fake provider，费用请求数不是实付；SIGKILL观察边界与原Rust事务边界分别绑定。',
        '平台只覆盖当次x86_64 Linux/aarch64 macOS及实际1.95.0/1.99.0；单格七not_run原记录不改，由8/0/0完整collector证明。',
        'gate synthetic controls只证明failure/inconclusive/invalid行为，不证明质量/速度/heldout通过。',
        'rollback两public revisions不是已发布包；四原local.json只证明disable后local可用，不证明live-provider掉线自动切换。',
        '既有平台旧mapping的awaiting字段是历史导航；当前接受取最终原review，不能改写旧文件。',
        '原ZIP与derived非ELF/MachO包/分片不同：必须保留原ZIP来源和省略收据；不能称派生包完整ZIP。',
        '未完成任何必要CI/P7/最终合并门时不得把own-scope接受升级为task done；source approval与release/G8认证分离。'],
    'per_task_validation_packages': {row['task_id']: row['validation_mapping'] for row in mapping},
    'repository_files_modified_by_this_generator': [], 'task_statuses_changed': [], 'new_product_execution': False})

assert (ROOT / TASK_PATH).read_bytes() == task_before
assert subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip() == start_head
for item in catalog.values():
    assert digest(item['local_path']) == item['sha256']
manifest = write('draft-manifest.json', {'outputs': [original,evidence_ref,map_ref,draft_ref,findings,fallback_ref],
    'generator': ref(Path(__file__)), 'repo_HEAD_before_after': start_head,
    'task_file_unchanged': True, 'all_referenced_input_files_unchanged': True,
    'formal_task_completion': False, 'done': 163, 'remaining': 29,
    'no_repository_mutation_or_ref_write': True, 'no_acceptance_rerun': True})
print(json.dumps(manifest, ensure_ascii=False))
