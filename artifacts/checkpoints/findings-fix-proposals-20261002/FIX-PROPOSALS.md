# formal-v4 审计/复放轮 6 项发现 — 修复提案（只提案，不实施）

- 日期：2026-10-02
- 依据原文：`artifacts/checkpoints/todolist-completion-audit/round07/formal-v4-independent-audit.json`（F-1、F-2）、`artifacts/checkpoints/todolist-completion-audit/round07/formal-v4-quality-acceptance.json`（N-1～N-4）
- 核对方式：全部发现已于 2026-10-02 在本仓库现场逐项重算/复核（manifest 条目、脚本行号、断言原文、空日志计数），与 round07 原文一致；与任务清单的出入见文末"清单与原文出入"。
- 红线遵守：本文档为唯一写入；未改动任何既有文件；未执行 git commit。

## 汇总表

| ID | 严重度 | 修复点 | 实施时机 |
|----|--------|--------|----------|
| F-1 | low_metadata_only | source-v6 冻结程序（非 v5 manifest） | 必须等 source-v6 冻结窗口 |
| F-2 | info_design | `p5e-formal-plan-20261001-v4/execute_registered.py:12` | 必须等 plan-v5/harness-v9 窗口（技术上无 hash 锁定，但按证据纪律推迟） |
| N-1 | low_replayability | final-v8 `inputs/` 下 4 份脚本各 1 行 assert | 必须等 harness-v9（4 份脚本被 final-v8 manifest 锁定） |
| N-2 | low_metadata | final-v8 `inputs/compare_mixed.py:157` | 必须等 harness-v9（同被锁定） |
| N-3 | info_process | 无需实施（replay_v4_serial.py 已含修正；本提案完成同型核验） | 已完成核验，零改动 |
| N-4 | info | 未来 harness 的 stage runner（日志落盘处） | 必须等 harness-v9 |

---

## F-1 (low)：v5 source-manifest 的 `.gitignore` 条目为 v4 冻结值

### 影响面
`artifacts/benchmarks/p5e-candidate-release-20261001-v5/source-manifest.json` 声明 head=`0de7c89...`，其中 `.gitignore` 条目为 `{bytes: 290, sha256: 67982aa7...}`，继承自 v4 冻结点（head=`0a56a25`）的 closure 拷贝；实际 `0de7c89` 的 `.gitignore` 为 636B（sha256 `59b02b64...`，cd22597 改动）。现场重算证实：629 条中 628 条与磁盘一致，唯此一条陈旧；v5 `source/.gitignore` 冻结拷贝同为 290B，closure 自身内部一致（manifest==source 拷贝，digest `78f83f0f...` 可重算）。`.gitignore` 不进入任何被测二进制或评测输入，无编译/测量语义，不推翻任何收据。根因：v5 closure 由 v4 closure 拷贝 + 仅 patch 2 个文件增量生成（`source-verification.json` 的 `prior_canonical_source` 字段可证），未对未 patch 文件回对声明 head。

### 精确修复点
不在 v5 manifest 本身（其内容与 digest `78f83f0f...` 已锁入 profile final-v5 的 `locked_inputs`，改动即断链）。修复点是 **source-v6 冻结程序**：在增量 closure（拷贝上版 + patch）完成后，新增"manifest 每条未 patch 条目回对声明 head 的 git blob"断言。该断言若在 source-v5 冻结时存在，本条会被当场拦截。

### diff 草案（source-v6 冻结程序中的校验步骤）
```python
import hashlib, json, pathlib, subprocess

def assert_manifest_matches_head(manifest_path: pathlib.Path, patched: list[str]) -> None:
    m = json.loads(manifest_path.read_text())
    head = m["head"]                      # 声明的冻结 head，如 0de7c89...
    for e in m["files"]:
        if e["path"] in patched:
            continue                      # patch 文件按 patch 定义核对，不回对 head
        blob = subprocess.run(
            ["git", "cat-file", "blob", f"{head}:{e['path']}"],
            capture_output=True, check=True).stdout
        actual = hashlib.sha256(blob).hexdigest()
        assert (len(blob), actual) == (e["bytes"], e["sha256"]), \
            f"stale closure entry vs declared head {head[:7]}: {e['path']}"

# source-v6 冻结时调用：
assert_manifest_matches_head(
    pathlib.Path("artifacts/benchmarks/p5e-candidate-release-20261001-v6/source-manifest.json"),
    patched=[],   # v6 实际 patch 清单
)
```

### 实施时机
**必须等 source-v6 冻结窗口。** v5 manifest 及其 source 拷贝、digest 全部处于锁定链（profile final-v5 `locked_inputs` / `78f83f0f...`），任何回改都是断链行为；F-1 的行为修正只能落在新冻结版本。原文 action："记录即可，无需重跑"。

### 风险
- 断言本身零风险（纯校验）。风险在纪律面：若有人"顺手修正"v5 manifest 条目，将破坏 `78f83f0f...` digest 锁与下游全部收据——本提案明确禁止。
- `git cat-file` 依赖工作仓 checkout 含声明 head 对象；冻结程序应在锁定 head 的同一仓内执行。

---

## F-2 (info)：`--check-only` 的 exit 0 只在 pre-run 状态可达

### 影响面
`artifacts/benchmarks/p5e-formal-plan-20261001-v4/execute_registered.py:12` 将运行目录新鲜度守卫 `assert not out.exists()` 置于 `--check-only` 分支（第 13–14 行）之前，导致 v4 正式运行落盘后，任何 post-run `--check-only` 复放必然在第 12 行失败（round07 审计的 `check_only_replay.post_run_direct` 现场复现：`FAIL at line-12`；pre-run 等价状态下全部断言 PASS、exit 0）。链条断言本身全部通过，属设计内行为而非链条缺陷，但使"运行后原位复核"结构上不可达。

### 精确修复点
`execute_registered.py:12`（守卫位置）与第 15 行（`out.mkdir()` 执行路径起点）。将守卫移入非 check-only 分支，check-only 保留全部 identities/hash/decision/sequence 断言。

### diff 草案
```diff
-identities();out=root/'artifacts/benchmarks/p5e-formal-runs-20261001-v4';assert not out.exists();
+identities();out=root/'artifacts/benchmarks/p5e-formal-runs-20261001-v4'
 if a.check_only:
 	print('CHECK-ONLY: decision/profile_sha256/plan_sha256/sequence-equality/manifest+binaries+locked_inputs identities all PASS; nothing executed, no run dir created');raise SystemExit(0)
+assert not out.exists()
 out.mkdir();logs=out/'stage-logs';logs.mkdir();env=os.environ.copy();env.update(plan['process_env']);receipts=[];progress=out/'progress.jsonl'
```

### 实施时机
**建议等 plan-v5/harness-v9 冻结窗口。** 现场核实：`execute_registered.py` 未被任何 hash 锁定（`CHAIN-VERIFICATION.json` 仅记录其 argv 与 stdout，final-v8 manifest 不含它），改动不会破坏锁定链；但 post-hoc 修改 v4 正式计划的注册执行器会改变已记录的复放观察——round07 审计的 `verification_commands` 明确记录了"运行后仅新鲜度守卫失败"这一行为，修改后该观察不再可复现。按证据纪律，随下版 plan 一起落。原文 action 同口径："可在后续 harness 版本将新鲜度守卫移入非 check-only 分支；本轮不修改"。

### 风险
- 移动守卫后 check-only 的 stdout 文本不变，pre-run 与 post-run 语义均保持（check-only 永不建目录）；执行路径守卫语义不变（run 目录已存在仍拒绝覆盖）。
- 若不等窗口提前修改：锁定链不破，但 v4 审计记录中的 post-run 行为描述与现场漂移，需在改动处注明修订原因。

---

## N-1 (low)：4 份离线重算脚本对已存在输出 fail-closed

### 影响面
final-v8 的 4 份确定性离线重算脚本均以 `assert not <output>.exists()` 换取防覆盖，任何复核（含 R2 本轮）必须先删除输出侧文件才能重算。现场确认行号：
- `artifacts/benchmarks/p5e-harness-20261001/final-v8/inputs/cluster_report.py:24`（`assert not a.output.exists(),'immutable report'`）
- `artifacts/benchmarks/p5e-harness-20261001/final-v8/inputs/compare_fanout.py:13`（`assert not a.output.exists()`）
- `artifacts/benchmarks/p5e-harness-20261001/final-v8/inputs/check_noanswer.py:26`（`assert not a.output.exists()`）
- `artifacts/benchmarks/p5e-harness-20261001/final-v8/inputs/compare_mixed.py:109`（`assert not args.output.exists()`）

与 F-2 同型：新鲜度/不存在性守卫防覆盖的代价是运行后不可原位复核。不推翻任何收据（R2 已通过先清输出完成 7/7 byte-identical 重算）。

### 精确修复点
上述 4 行，各新增受 sanctioned 目录约束的 `--force` 选项，默认行为不变。

### diff 草案（以 compare_mixed.py 为例；其余 3 份同模式）
```diff
     parser.add_argument('--workload-count-lock',type=pathlib.Path,default=pathlib.Path(__file__).parent/'EXACT-MIXED-WORKLOAD-COUNT-LOCK.json')
-    args=parser.parse_args();assert not args.output.exists();runs=[args.baseline,args.candidate]
+    parser.add_argument('--force',action='store_true',help='recompute in place; only for outputs under artifacts/benchmarks/')
+    args=parser.parse_args()
+    assert args.force or not args.output.exists(),'immutable report: pass --force to overwrite a sanctioned benchmark output'
+    if args.force:
+        parts=args.output.resolve().parts
+        assert 'artifacts' in parts and 'benchmarks' in parts,'--force is restricted to sanctioned benchmark outputs'
+    runs=[args.baseline,args.candidate]
```
```diff
     # cluster_report.py:24
-    p.add_argument('--output',type=pathlib.Path,required=True);a=p.parse_args()
-    assert not a.output.exists(),'immutable report';data=json.loads(a.ablation.read_text());pairs=collections.defaultdict(list)
+    p.add_argument('--output',type=pathlib.Path,required=True);p.add_argument('--force',action='store_true');a=p.parse_args()
+    assert a.force or not a.output.exists(),'immutable report: pass --force to overwrite a sanctioned benchmark output'
+    if a.force:
+        parts=a.output.resolve().parts
+        assert 'artifacts' in parts and 'benchmarks' in parts,'--force is restricted to sanctioned benchmark outputs'
+    data=json.loads(a.ablation.read_text());pairs=collections.defaultdict(list)
```

### 实施时机
**必须等 harness-v9 冻结窗口。** 现场核实：这 4 份脚本全部是 final-v8 `FINAL-HARNESS-INPUT-MANIFEST.json` 的 `sourcefiles` 锁定条目（bytes+sha256），改动任一副本即破 `99cb8295...` manifest 锁及其下游 profile/plan 锁。原文 action："后续 harness 版本可考虑 --force-output-under-sanctioned-dir 选项"（本 diff 即按 sanctioned-dir 约束起草，默认 fail-closed 不变）。

### 风险
- `--force` 引入误覆盖重算产物的可能；sanctioned-dir 断言 + 默认 fail-closed 缓解。
- `--force` 覆盖的是"本轮的输出副本"，不是 v4 raw 输入；重算类复核本就以输入不可变为前提，输出侧可写不损害保真。

---

## N-2 (info)：`compare_mixed.py` 经 count lock 以相对路径解析 registered plan

### 影响面
`final-v8/inputs/compare_mixed.py:157` 的 `source_plan=pathlib.Path(registered['path'])` 按 `EXACT-MIXED-WORKLOAD-COUNT-LOCK.json` 内的相对路径（现场确认指向 `artifacts/benchmarks/p5e-harness-20261001/final-v6/inputs/mixed-c{1,4,8,16}.json`）解析 registered workload plan。相对路径隐含"仓库根 cwd"前提；从非仓库根执行会 `FileNotFoundError`（R2 初跑即触发）。内容侧有 sha256 锁校验（第 158 行），不推翻任何收据。属 final-v6→final-v8 跨版本路径口径遗留。

### 精确修复点
`compare_mixed.py:157`：把相对路径解析锚定到脚本自身位置（沿 `inputs/` 向上定位仓库根），sha256 锁校验逻辑不动。

### diff 草案
```diff
-    count_registry=json.loads(args.workload_count_lock.read_text());registered=count_registry['plans'][str(plans[0]['concurrency'])];source_plan=pathlib.Path(registered['path']);locked_plan=json.loads(source_plan.read_text());
+    count_registry=json.loads(args.workload_count_lock.read_text());registered=count_registry['plans'][str(plans[0]['concurrency'])]
+    reg=pathlib.Path(registered['path'])
+    if not reg.is_absolute():
+        anchor=pathlib.Path(__file__).resolve()      # .../final-v8/inputs/compare_mixed.py
+        while anchor.name!='artifacts' and anchor.parent!=anchor:anchor=anchor.parent
+        assert anchor.name=='artifacts','cannot locate repo root from script position'
+        reg=anchor.parent/reg                        # 锚定仓库根，与 count lock 相对路径口径一致
+    source_plan=reg;locked_plan=json.loads(source_plan.read_text());
     if hashlib.sha256(source_plan.read_bytes()).hexdigest()!=registered['sha256'] or locked_plan!=plans[0]:issues.append({'field':'registered_exact_source_workload_mismatch'})
```

### 实施时机
**必须等 harness-v9 冻结窗口**（`compare_mixed.py` 同为 final-v8 manifest 锁定条目；count lock 文件亦在锁链内）。原文 action 给出两个等价方向："把 registered path 收进当版 manifest 或改绝对锚点"——本 diff 取"绝对锚点"实现；"收进当版 manifest"可作为 harness-v9 产出 count lock 时的替代写法，二者择一即可。

### 风险
- 锚定错误会读到错误副本或 FileNotFoundError；第 158 行的 sha256 锁校验保留后，任何错配都 fail-closed（`registered_exact_source_workload_mismatch`），不会静默通过。

---

## N-3 (info)：前任 census 断言错误已修正；同型核验通过，无需再改

### 影响面与原文核对
**清单与原文有出入，以原文为准**：原文 N-3 记录的是前任复放 runner `replay_v4.py` 的两处缺陷——①stage_b 8 路并发（停滞根因）；②census 断言写错——并在本轮已由 `replay_v4_serial.py` 修正（全程串行 + schema v2 收据）；原文并无"其余 3 份离线重算脚本"的待办。任务清单把核验对象表述为"其余 3 份离线重算脚本"，本提案按原文口径记录，并补做该核验。

现场确认缺陷与修正：
- `artifacts/benchmarks/p5e-formal-runs-20261001-v4-replay/replay_v4.py:218-222`：`req_per_cell = total_rows // 3` 后断言 `req_per_cell == 1224`——实际 1224 是 32 目录的 `measured_rows` 总和，除以 3 得 408，该检查若跑通会误报 FAIL。
- `replay_v4_serial.py:262-266` 已修正为 `total_rows == 1224`（R2 有效 runner，25/25 PASS）。

### 同型断言核验（本提案完成，零改动）
对 final-v8 全部 4 份离线重算脚本 grep `// 3` / `== 1224` / `== 408` / 除以 reps 后对比总数的模式：**零命中，无同型错误**。逐份确认：
- `cluster_report.py`：无行数 census 断言；edge/case 覆盖用集合差与重复计数（第 34–47 行），口径正确。
- `compare_fanout.py`：repetition 顺序断言 `seq!=list(range(expected))`（第 21 行），按单 run 口径，正确。
- `check_noanswer.py`：`sorted(...repetition)!=expected`，`expected=list(range(suite['repetitions']))`（第 32–33 行），单 run 口径，正确。
- `compare_mixed.py:117`：`len(rows[n])!=len(expected)`，rows 对 expected jobs 总数（330/run），正确；1224 型总量只出现在 R2 报告文字中，不在脚本内。

### diff 草案
无。`replay_v4.py` 按证据纪律保留原样（复放轮的缺陷记录本身就是 N-3 的证据载体，post-hoc 修改会破坏"前任脚本如实保留"的审计口径）；修正已存在于 `replay_v4_serial.py`。

### 实施时机
已完成（本提案的核验部分）；无需冻结窗口。

### 风险
无实施风险。唯一注意项：后续审计引用 `replay_v4.py` 时应知其 C2 检查为已知缺陷，以 `replay_v4_serial.py` + `REPLAY-RECEIPT.json`（schema v2）为准。

---

## N-4 (info)：21/31 stage log 为合法空文件，建议 harness 写 owned 空日志标记

### 影响面
v4 正式运行 31 份 stage log 中 21 份为 0 字节（现场复核计数一致）：对应 stage（0/1/12–30）为 evidence-driver 或 cc-eval 直写结构化输出到 output 目录的形态，stdout 无内容属设计内；`log_sha256` 声明为 hash-of-empty 且与 `STAGE-RECEIPTS.json` 逐一重算一致。合法但易被后续审计误判为"缺日志"。原文 action："无需动作"，本提案的修复属 harness 侧预防性改进。

### 精确修复点
未来 harness 的 stage runner 日志落盘处。v4 对应实现为 `p5e-formal-plan-20261001-v4/execute_registered.py:29-31`（`with log.open('w')` → `Popen`/`wait` → 收据内 `log_sha256: sha(log)`）；在 `actual_rc` 取得后、计算 `log_sha256` 之前补写标记，收据即与落盘内容一致。

### diff 草案（harness-v9 runner）
```diff
 with log.open('w') as stream:
 	child=subprocess.Popen(step['argv'],cwd=root,env=env,stdout=stream,stderr=subprocess.STDOUT);actual_rc=child.wait()
+if log.stat().st_size==0:
+	log.write_text(f"# owned-empty-log stage={step['stage']} seq={step['sequence']} rc={actual_rc}: stdout empty by design (evidence-driver/structured-output stage); presence of this line proves the log was created and deliberately empty, not lost\n")
 receipt={'step':step,'started_epoch':start,'finished_epoch':time.time(),'actual_return_code':actual_rc,'child_pid':child.pid,'log_sha256':sha(log),...}
```

### 实施时机
**必须等 harness-v9 冻结窗口**（同 F-2 的 runner 载体）。仅对新运行生效；既有 31 份 v4 log 与收据不动。

### 风险
- 标记会改变新运行的 log bytes——`log_sha256` 必须在写标记之后计算（diff 已按此顺序），否则收据失配。
- 标记文本必须单行、以注释样式开头、明示 owned/empty-by-design，避免被下游解析器误当测量输出。

---

## 清单与原文出入（以原文为准）

1. **N-3**：任务清单表述为"R2 已修正该份脚本——确认其余 3 份离线重算脚本无同型断言错误"；原文 N-3 实为前任 runner `replay_v4.py` 的**两处**缺陷（8 路并发 + census 断言写错），修正落在 `replay_v4_serial.py`，且原文未布置"其余 3 份离线重算脚本"核验。本提案已按原文记录并补做核验（4 份脚本全查，结论：无同型错误）。
2. **N-1**：任务清单建议"--force 或输出目录不存在断言统一模式"；原文 action 为"--force-output-under-sanctioned-dir 选项"。语义一致，diff 按原文的 sanctioned-dir 约束起草。
3. **N-2**：任务清单建议"路径解析锚定到脚本自身位置"；原文 action 为"把 registered path 收进当版 manifest 或改绝对锚点"。脚本位置锚定即"绝对锚点"的一种实现，不冲突。
4. **F-1 / F-2 / N-4**：与原文一致；F-1 的关键数值（290B / `67982aa7...` / 636B / 629 条中 628 条一致 / head `0de7c89` vs v4 冻结点 `0a56a25`）已现场重算证实。

## 实施窗口结论

- **可立即安全实施：1 项**（N-3——纯核验，本提案已完成，零文件改动）。
- **必须等冻结窗口：5 项**——F-1 等 source-v6；F-2 等 plan-v5/harness-v9（技术上无 hash 锁定，推迟原因是证据纪律）；N-1、N-2、N-4 等 harness-v9（脚本/runner 处于 final-v8 manifest 锁定链内）。
- 全局红线（各窗口均适用）：不回改 v5 manifest 与 final-v8 inputs 锁定副本；任何修复只落入下一冻结版本；修复落地后以新版本收据重放验证。
