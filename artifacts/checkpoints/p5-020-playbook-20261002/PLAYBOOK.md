# P5-020「P5本地增强版验收」执行手册（预写）

- 写入日期：2026-10-02。性质：只读规划产物。本手册是本轮唯一写入；未改动 `crates/`、`docs/roadmap/code-index-v2/tasks.json`、`docs/roadmap/` 既有文档或任何锁定链；未执行 git commit。
- 目标：P5-019 收口后，按本手册立即执行 P5-020（G5 冻结 + M2 本地版本证据），不改手册以外任何流程。
- 现状基线（不得推断为已完成）：`tasks.json` P5-019 `in_progress`（c16 单跑待静默窗口）；P5-020 `status=todo`、`evidence=[]`；G5/M2 未认证（`docs/roadmap/code-index-v2/05-TODO.md:1580-1594`；`docs/roadmap/code-index-v2/06-VALIDATION.md:5`）。

## 0. 依据文件清单

| 依据 | 路径 |
|---|---|
| P5-020 条目 | `docs/roadmap/code-index-v2/tasks.json` P5-020（steps：G5冻结source/配置和public schema；形成M2本地版本证据；validations：V11/V12/V18/V19/V20；acceptance：semantic 仅显示 disabled、旧功能回归通过） |
| 验证定义 | `docs/roadmap/code-index-v2/06-VALIDATION.md:31`（V11）、`:32`（V12）、`:38`（V18）、`:39`（V19）、`:40`（V20）、`:53`（G5）、`:57-58`（"同一 gate 的所有任务必须有当前 run-id 证据"） |
| 迁移/兼容 | `docs/roadmap/code-index-v2/07-ROLLOUT.md` 第 4 节（MCP 兼容与默认离线）、第 6 节（M2=本地完整交付） |
| GATE 样例 | `docs/roadmap/code-index-v2/P5-D-RUNTIME-GATE.json`（结构模板）；`artifacts/checkpoints/todolist-completion-audit/round07/G5-EXECUTION-APPROVAL-final-v5.json`（source_digest 计算口径与 premeasurement_rules） |
| F-1 修复 | `artifacts/checkpoints/findings-fix-proposals-20261002/FIX-PROPOSALS.md:21-58` |
| c4 竞态 | `artifacts/checkpoints/c4-race-analysis-20261002/ROOT-CAUSE-AND-FIX-PROPOSAL.md` §5 方案 d、§6 实施前置与验证 |
| 批次 0 / F0 | `artifacts/checkpoints/p6-implementation-planning-20261002/IMPLEMENTATION-ORDER.md:25-36` |
| P5-019 终局裁决 | `artifacts/checkpoints/todolist-completion-audit/round07/formal-v4-quality-acceptance.json`（`promote_to_done_assessment`、`sequence_recommendation`） |
| c16 零成本入口 | `artifacts/benchmarks/p5e-formal-runs-20261001-v4-rerun-r1/c16-final/run_c16_final.py`；`c16-FINAL-BLOCKED.json`（readiness_state_preserved）；`c16-QUIET-WATCH.md` |
| formal-v4 耗时 | `artifacts/benchmarks/p5e-formal-runs-20261001-v4/STAGE-RECEIPTS.json`（31 stage started/finished epoch） |
| 回归耗时基线 | `artifacts/benchmarks/p5d-20260930-resume/final-v3/validation.json`（commands[].seconds） |

---

## 1. 前置清单：P5-019 转 done 的精确条件

P5-020 的 `depends_on` 含 P5-019（`tasks.json` P5-020 `depends_on[]` 末项），因此执行本手册前 P5-019 必须先收口。依据 round07 质量验收的终局裁决（`formal-v4-quality-acceptance.json:113-118`），精确条件如下：

1. **c16 单跑落地**（step2 唯一剩余可执行证据）：
   - 零成本入口：`artifacts/benchmarks/p5e-formal-runs-20261001-v4-rerun-r1/c16-final/run_c16_final.py`（原 v4 plan seq 23 argv，仅输出前缀替换为 `c16-final/`；spawn 前再次断言静默谓词 + hash + 输出目录不存在，fail-closed；见 `c16-FINAL-BLOCKED.json` 的 `readiness_state_preserved`）。执行前无需任何其他准备。
   - 静默谓词：`loadavg_1m < 3.0 AND pgrep -f 'tm[-]r5bench' 为 0`（`run_c16_final.py` 内 `quiet()`）。上一轮 2 小时有界等待未获得窗口（load 最低 4.24，`c16-QUIET-WATCH.md` 末表），本轮允许直接执行脚本内置等待，或先用同样谓词等待。
   - **终态评估标准**：脚本 exit 0；`c16-final/mixed-c16-paired-report.json` 产出且 `status=computed_observations_not_G5_acceptance`（预期口径，见质量验收 R-d disposition 与 resumption_recipe）；600 请求（baseline 300 + candidate 300）0 硬 error；race census 原样记录不删改；v4/rerun-r1 raw 与 R1 收据零触碰。
   - 若静默窗口始终不可得：P5-019 不得转 done（step2 存在正式 invalid 并发点，不具备收口条件，`formal-v4-quality-acceptance.json:116`）。不得降窗跑。
2. **证据回填**：按质量验收 `p5_019_backfill_evidence_proposal`（`formal-v4-quality-acceptance.json:107-111`）四条 + c16-final 新产物，回填 `tasks.json` P5-019 `evidence[]`。
3. **c4 记 blocked**：P5-019 以 `c4=blocked_evidence_workload_inherent_race` 记录收口（workload-inherent，不得 rerun-to-green；`real_finding_for_product_record`；零数据损坏——每 stage 330 jobs count lock 一致，见 `formal-v4-quality-acceptance.json:52-54`），并把该竞态发现挂入产品/查询执行 backlog。
4. **红线**：全程不改 v4/rerun-r1 raw 与锁定链；不 git commit；P5-019 未满足上述条件前，本手册第 2 节不得开始。

> 注意第 2 节裁决点：若采纳"c4 修复纳入 source-v6"（推荐），c4 修复的实施窗口在 P5-019 转 done 之后、source-v6 冻结之前，P5-019 的 evidence 链保持锁定在 v5 闭包不动，修复产生的全部新证据计入 P5-020 轮（P5-E 新 run-id）。

---

## 2. Step1：G5 冻结程序

### 2.0 裁决点：c4 竞态修复是否纳入 source-v6（冻结前必须先裁决）

**建议：纳入——c4 修复（c4 文档方案 d：P0+P1+P2，`ROOT-CAUSE-AND-FIX-PROPOSAL.md:171-193`）在 source-v6 冻结之前实施，使 G5 冻结包含修复。**

| | 纳入（推荐） | 不纳入 |
|---|---|---|
| 依据 | c4 文档 §6 明确修复改动 ≤3 文件 ~50 行（P0：`crates/cc-server/src/handlers/context.rs:55` 拆 fence 外硬校验；P1：`crates/cc-search/src/engine_cache.rs:209` 有界退避；P2：fence 接受代透出 + `attach_observed` 精化），零写路径/零 epoch 语义改动 | 保持 `crates/` 零改动直达冻结，formal-v4 全部证据直接绑定冻结源 |
| 利 | G5/M2 认证的产品不含已知系统性竞态；V11"混代读拒绝/有限重试"证据记录的是修复后行为而非已知缺陷；`real_finding_for_product_record` 以修复+判据复测收口而非长期挂账 | G5 执行最快；formal-v4（V19/V20 主体）与 p5d final-v3（V11/V18 运行时子集）无需任何重跑即可按"当前冻结源"口径引用；无新测量风险 |
| 弊 | `crates/` 变更使 formal-v4 测量源（v5 闭包）与冻结源不再一致——mixed/facet/latency 类证据需按第 3 节条件化规则补采（测量成本很低，约 4–35 分钟，见第 5 节）；P2 引入 freshness 观测字段，需过一遍 additive-contract 检查（V18 面）；c4 修复本身需红绿测试先行（约 0.5–1 天人工） | P6 批次 0 红线"F0 之后 `crates/` 零改动直达批次 1 第一个提交；任何插入改动 = 重走批次 0"（`IMPLEMENTATION-ORDER.md:30`）——竞态修复若落在 P5-020 之后，必然重走冻结闭包 + 疑似全矩阵重认证，总成本反而更高；M2 必须声明"c4 对比证据不存在"，且 V11 冻结点带着已知硬 error 尾部 |
| 裁决依据小结 | 关键不对称：**修复迟早要做，而冻结锚点 F0 只能有一个**。修复落在冻结前 = F0 一次成型；修复落在冻结后 = F0 作废重走。前者新增测量成本（分钟级），后者新增重认证成本（小时级 + 审批链） | 仅当 c4 修复被裁决"另立任务且不阻塞 P6"时才成立；届时 M2 的 c4 声明义务不可省略（`formal-v4-quality-acceptance.json:117`） |

裁决记录要求：无论选哪边，在 P5-020 实施备注与本手册执行收据中写明裁决结果与理由。以下第 2.1–2.3 节按"纳入"路径编写；若裁决"不纳入"，跳过 2.1 的实施子步，第 3 节全部按"零改动引用"口径执行。

### 2.1 c4 修复实施（纳入路径时）

1. 按 c4 文档 §6 顺序 P0 → P1 → P2（每步独立可回退），TDD：先红后绿。
   - 单测落点：`crates/cc-eval/tests/p1d_concurrency.rs`（现有 `RetrievalChanged{attempts:3}` 契约）、`engine_cache.rs` 内 fence 四个测试（`engine_cache.rs:376-458`）、`freshness.rs:71-97`；P2 新增两侧用例——"接受代==当前代 → complete=true 且带 `dispatch_observed_generation_change` 观测字段"与"真 stale → 仍 `changed_during_query`"（`ROOT-CAUSE-AND-FIX-PROPOSAL.md:202`）。
2. 明确不做（c4 文档 §6.4）：不改 `epoch_rules.rs` 声明、不改 `swap_rebuild_staging` 时序、不改 harness 阈值与 known-red 判定逻辑。
3. 判据复测：同 seed 1905 / 同 manifest 跑 P5-E mixed-c4（新 run-id，见 3.3），判据：0 硬 error、0 假阳性 Partial；`changed_during_query` 观测字段计数应与历史普查量级一致（作为竞态仍被如实观测的证据，而非竞态消失的伪证）（`ROOT-CAUSE-AND-FIX-PROPOSAL.md:203`）。

### 2.2 source-v6 冻结步骤

沿用 v4→v5 已验证的 closure 流程（v5 由 v4 closure 拷贝 + patch 2 文件生成，`artifacts/benchmarks/p5e-candidate-release-20261001-v5/source-verification.json`），加 F-1 修复断言：

1. **确定冻结 head**：当前 git HEAD（不得含未提交工作树差异；若 c4 修复未提交，先由用户授权提交或以 worktree_status 显式声明——v5 的先例是 `worktree_status` 显式声明 2 个 patch 文件，source-manifest.json `worktree_status` 字段）。
2. **生成 source 拷贝**：整仓覆盖面拷贝至 `artifacts/benchmarks/p5e-candidate-release-20261002-v6/source/`（文件数以实际为准，v5 口径 629 文件）。
3. **生成 manifest**：`source-manifest.json`，字段照 v5：`{schema_version, status, head, worktree_status, source_digest_sha256, file_count, total_bytes, archive_sha256, scope, derived_from}`。
4. **F-1 修复断言（本轮新增，必须执行）**：对 manifest 每一条非 patch 条目回对声明 head 的 git blob。修复点与 diff 草案见 `FIX-PROPOSALS.md:30-51`，核心：

```python
def assert_manifest_matches_head(manifest_path, patched):
    m = json.loads(manifest_path.read_text())
    head = m["head"]
    for e in m["files"]:
        if e["path"] in patched:
            continue
        blob = subprocess.run(["git", "cat-file", "blob", f"{head}:{e['path']}"],
                              capture_output=True, check=True).stdout
        actual = hashlib.sha256(blob).hexdigest()
        assert (len(blob), actual) == (e["bytes"], e["sha256"]), \
            f"stale closure entry vs declared head {head[:7]}: {e['path']}"
```

   该断言若在 v5 冻结时存在，即可拦截 `.gitignore` 290B 陈旧条目（`FIX-PROPOSALS.md:24`）。执行环境要求：工作仓 checkout 含声明 head 对象（`FIX-PROPOSALS.md:58`）。
5. **不可回改红线**：v5 manifest 与 `78f83f0f...` digest 锁定链不动（`FIX-PROPOSALS.md:54,57`）；F-1 的行为修正只落 source-v6。
6. **登记**：见 2.4。

### 2.3 配置与 public schema 冻结口径

- **配置冻结**：`crates/cc-model/src/config.rs`（默认值语义：`semantic.enabled=false`、Tiny 读池=4 等）、`docs/CONFIGURATION.md`、`docs/MCP_TOOLS.md`——三者纳入 source 闭包（本就在 `crates/`/`docs/` 覆盖面内），并在 GATE 文件记录 `config_frozen_head`。语义能力要求：`semantic.enabled=false` 不创建 cache 文件、不探测模型 endpoint、不启动 worker（`07-ROLLOUT.md` 第 4 节）。
- **public schema 冻结**：14 工具旧输入属性和必填项保持一致，两个可选 `retrieval_strategy` 字段以外零漂移（`06-VALIDATION.md:9`；P5-D GATE `input_contract` 字段口径）；新参数（含 c4-P2 的 freshness 观测字段，若纳入）必须 additive 且贯穿 schema/sanitize/dispatch/status（`07-ROLLOUT.md` 第 4 节）。冻结证据 = additive-contract 检查 + real-mcp/protocol 测试组（见 3.3），登记进 GATE 的 `input_contract` 字段。
- **不冻结进本轮**：cc-semantic（P6 才添加，`07-ROLLOUT.md` 第 1 节）、任何新 provider/vector 面。

### 2.4 冻结锚点 F0：命名、hash 计算与登记位置

- **锚点命名**：本冻结 SHA 即 P6 规划定义的"冻结锚点 F0"（`IMPLEMENTATION-ORDER.md:29`）。run-id 约定：`artifacts/benchmarks/p5e-g5-freeze-20261002/`（冻结程序收据）+ `artifacts/benchmarks/p5e-candidate-release-20261002-v6/`（source 闭包与二进制）。
- **hash 计算口径**（照 v5 批准文件原文，`G5-EXECUTION-APPROVAL-final-v5.json` 的 `source_digest_basis`）：
  `source_digest_sha256 = sha256(json.dumps(全部文件条目 {path,bytes,sha256} 按 path 排序, separators=(",",":"), sort_keys=True))`；二进制单独 `sha256`（`BUILD-RECEIPT.json.files_sha256` 模式）。
- **登记位置**（三处，缺一不可）：
  1. `artifacts/benchmarks/p5e-candidate-release-20261002-v6/source-manifest.json`（原始登记）；
  2. `docs/roadmap/code-index-v2/06-VALIDATION.md` 头部冻结段（当前记载的是 623 文件 / `44b30ae...` 旧闭包，`:7`，收口时更新为 source-v6 数值并注明旧值归属 P5-D 轮）；
  3. `docs/roadmap/code-index-v2/P5-G5-GATE.json`（新建，结构见第 4.2 节）+ `tasks.json` P5-020 `evidence[]` 回填。

---

## 3. Step2：M2 本地版本证据（V11/V12/V18/V19/V20）

### 3.1 证据映射总表

| 验证包 | G5 对应断言（06-VALIDATION.md:53 + 各 V 定义行） | 可直接引用的既有 raw（避免重跑） | 必须新采 |
|---|---|---|---|
| V11 查询执行/缓存（`:31`：deadline/取消、有界线程、1连接读池、锁外网络、混代读拒绝/有限重试、缓存 key 完整） | G5：网络等待锁验证用 fake | 混代守卫机制证据：formal-v4 mixed c1/c8 paired reports（`computed_observations_not_G5_acceptance`，含 per-lane latency）；p5d final-v3 运行时子集（`P5-D-RUNTIME-GATE.json`）作旧基线引用 | 纳入 c4 修复时：mixed-c4 判据复测（0 硬 error / 0 假阳性 Partial）+ fence/freshness 红绿测试收据。不纳入时：无（V11 以 c4=blocked 记录） |
| V12 选择与预算（`:32`：首块超大、metadata 膨胀、重叠、单文件 locate/多文件 facets、完整 JSON、source freshness） | G5：预算完整性 | formal-v4 facet cell_100-111 零失败 + fanout 300/300 issues=[] + original51 严格预算记录（`formal-v4-quality-acceptance.json:28-34`） | 纳入 c4 修复时：facet 8 cell 复跑（约 3s）+ fanout 双侧复跑（约 42s），绑新冻结二进制。不纳入时：无 |
| V18 MCP/配置兼容（`:38`：14工具不丢失，旧 mode 不变，新参数贯穿 schema/sanitize/dispatch/status，默认无网络无 key 可用） | G5 内含 | 旧契约回归口径：p5d final-v3 real-mcp 25/protocol 1 收据（历史基线） | **任何路径都必须新采**：冻结源上的 real-mcp + protocol + focused 契约组 + capability_status "semantic 仅显示 disabled" 探针 + additive-contract 检查（并入 3.3 回归轮） |
| V19 检索质量/消融（`:39`：compat/native 分报、真实 corpus、hard negatives、facet/span、macro/micro/CI） | G5：本地检索质量 | **V19 主体全部引用 formal-v4**：48/48 edges、1224 请求 8-cell、9 witness、original51 baseline strict 红（R2 复放 25/25 PASS 确认，`formal-v4-quality-acceptance.json:16-34`） | 纳入 c4 修复时：在 GATE limitations 声明"V19 主体为修复前源 mechanism-scope 证据 + 修复判据复测补强"；可选严格路径 = 新批准链全矩阵复跑（第 5 节给出增量成本）。不纳入时：无 |
| V20 性能/资源/成本（`:40`：C1/4/8/16、RSS 进程归属、no best-of、费用复用） | G5：tail latency | mixed c1/c8 paired（per-lane latency + native resource proof）、typedgraph、noanswer absence、lifecycle-cost（p5d final-v3） | 纳入 c4 修复时：mixed-c16（P5-019 已新采）+ mixed-c4 判据复测让 4/4 并发点在冻结源上有观测。不纳入时：c16/c4 按 blocked/invalid 记录 |
| acceptance"semantic 仅显示 disabled" | tasks.json P5-020 acceptance[0] | 无可引用 | **必须新采**：默认配置 stdio 起服 → capability_status 探针，断言 semantic 状态=disabled/未配置且不冒充 ready（对应 P5-016 验收"不出现 status ready 但 dense 因未配置跳过的假象"），零网络零 key |

**必须新采证据项数：纳入 c4 修复路径 = 6 项**（①c16 单跑 paired——P5-019 收口件；②c4 修复红绿测试收据；③mixed-c4 判据复测；④source-v6 冻结闭包 + F-1 断言；⑤冻结源全回归（双工具链 + release cost）；⑥V18 契约 + semantic-disabled 探针 + additive-contract）。**不纳入路径 = 4 项**（①④⑤⑥，且①在 P5-019 已完成）。

### 3.2 哪些 formal-v4 raw 直接引用（禁止重跑，避免破坏保真）

以下原件只引用不重跑：`STAGE-RECEIPTS.json` 31 stage、original51 两 suite、facet 8 cell、typedgraph 双侧、fanout 双侧、mixed c1/c8 paired、original-noanswer-absence、CELL-SOURCE-POSTBUILD-CHECK；引用方式 = 在 GATE `artifacts[]` 与 `evidence[]` 列路径 + 在 `limitations` 声明测量源闭包 digest（v5 `78f83f0f...`）。R1/R2/round07 三份收据作为链完整性证据引用。重跑这些 stage 属"伪造当前性"——若走严格路径，必须在新 run-id 下全新落盘，不覆盖旧 raw（`07-ROLLOUT.md` 第 6 节"不把重跑成功覆盖首个失败"）。

### 3.3 最小新采命令序列（纳入路径；不纳入路径只做第 4–6 步）

```sh
# 1) P5-019 前置：静默窗口 c16 单跑（内置谓词等待，约 40s 测量 + 等待）
python3 artifacts/benchmarks/p5e-formal-runs-20261001-v4-rerun-r1/c16-final/run_c16_final.py

# 2) c4 修复红绿（TDD，实施由 2.1 节定义；此处为定向验证面）
cargo test -p cc-search --locked --offline   # engine_cache fence 四测试
cargo test -p cc-server --locked --offline   # freshness / context handler
cargo test -p cc-eval  --locked --offline --test p1d_concurrency

# 3) mixed-c4 判据复测（新 run-id p5e-formal-runs-20261002-fix1/，同 seed 1905、
#    同 final-v8 harness argv 口径；新批准收据仿 round07 链落 run 目录）
#    判据：0 硬 error、0 假阳性 Partial、count lock 330/330
# 3b) facet 8 cell + fanout 双侧绑新 codecortex 二进制（各 <1s / 各 ~21s）

# 4) source-v6 冻结（2.2 节程序，含 F-1 断言）+ release 构建
SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX15.4.sdk \
CARGO_BUILD_JOBS=2 cargo build --offline --locked --release \
  -p cc-server --bin codecortex
# BUILD-RECEIPT.json 照 p5e-candidate-release-20261001-v4/BUILD-RECEIPT.json 字段落盘

# 5) 冻结源全回归（双工具链，命令照 p5d final-v3 validation.json 的 argv 原文）
cargo +stable  clippy --workspace --all-targets --features cc-eval/eval-http --locked --offline -- -D warnings
cargo +stable  test  --workspace --no-fail-fast --locked --offline -- --test-threads=1
cargo +stable  test  -p cc-eval --features eval-http --locked --offline -- --test-threads=1
cargo +1.95.0  clippy --workspace --all-targets --features cc-eval/eval-http --locked --offline -- -D warnings
cargo +1.95.0  test  --workspace --no-fail-fast --locked --offline -- --test-threads=1
cargo +1.95.0  test  -p cc-eval --features eval-http --locked --offline -- --test-threads=1
cargo +stable  test  -p cc-eval --test p5a_cost --test p5b_cost --test p5c_cost --test p5d_cost --release

# 6) V18 契约 + semantic-disabled 探针（冻结二进制起 stdio，零网络零 key）：
#    - tools/list：14 工具名/必填项/旧 mode 与 p5d additive-contract 基线逐项比对
#    - capability_status：semantic 仅显示 disabled，无 ready 假象
#    - protocol smoke（握手/错误/取消）1 条
```

---

## 4. 验收门

### 4.1 P5-020 → done 的判定标准（全部满足才可转 done）

1. P5-019 已 done（第 1 节条件满足）。
2. source-v6 冻结闭包落盘且 F-1 断言通过；`source_digest_sha256` 与 head 按口径登记三处（2.4 节）。
3. G5 GATE 文件落盘，`G5` 字段从 P5-D GATE 的 `not_run` 变为 `passed_declared_local_scope` 或带 blocked 项的显式口径；`full_retrieval_gate` 如实——原 source/intent Partial 和 S11 已知失败原样保留即通过（不要求翻绿，`tasks.json` P5-020 `implementation_notes` 原文约束）。
4. V11/V12/V18/V19/V20 逐项有证据：引用件给出路径，新采件给出收据；**没有证据的项标 not_run/blocked 而非 done**（P5-020 acceptance[1] 原文）。纳入路径下 c4 并发点若判据复测通过则计入 computed_observations，否则按 blocked 记录。
5. acceptance[0]：semantic 仅显示 disabled 探针通过；不需要真实 embedding 即交付。
6. acceptance[1]：旧功能回归通过——第 3.3 节第 5 步双工具链全绿（失败 0，ignored 明列不计通过，`06-VALIDATION.md` 第 5 节统计口径）。
7. M2 口径声明：本地完整交付（`07-ROLLOUT.md` 第 6 节 M2 定义），limitations 明列不含项——真实 provider、holdout、100k、跨平台、发行认证、（若未修复）c4 对比证据。
8. `tasks.json` P5-020 状态回填 + `P6-001` 状态回填裁决（批次 0 出口条件，`IMPLEMENTATION-ORDER.md:35-36`）。

### 4.2 G5 GATE 文件结构（仿 P5-D-RUNTIME-GATE.json，新建 `docs/roadmap/code-index-v2/P5-G5-GATE.json`）

```json
{
  "batch": "P5-D",
  "gate": "G5",
  "accepted_tasks": ["P5-019", "P5-020"],
  "status": "passed_declared_local_scope",
  "G5": "passed_declared_local_scope",
  "M2": "passed_local_scope",
  "target_sha": "<source-v6 冻结 head>",
  "covered_source_digest_sha256": "<source-v6 digest>",
  "covered_files": 629,
  "config_frozen_head": "<同 head；CONFIGURATION.md/MCP_TOOLS.md 在闭包内>",
  "input_contract": "14 tools and existing properties preserved; additive fields: retrieval_strategy x2 (+ freshness observation fields if c4-P2 landed)",
  "semantic_delivery": "no embedding required; capability_status shows semantic disabled only",
  "toolchain_tests": {
    "stable":  { "workspace": {"passed": 0, "failed": 0, "ignored": 0}, "http": {}, "focused": {}, "protocol": {}, "real-mcp": {}, "watcher": {} },
    "1.95.0":  { "workspace": {}, "http": {}, "focused": {}, "protocol": {}, "real-mcp": {}, "watcher": {} }
  },
  "validations": {
    "V11": { "status": "…", "evidence": ["…mixed-c4 判据复测收据…", "…formal-v4 mixed c1/c8 引用…"] },
    "V12": { "status": "…", "evidence": ["…facet/fanout 新采或引用…"] },
    "V18": { "status": "…", "evidence": ["…real-mcp/protocol/additive-contract/semantic-disabled 探针…"] },
    "V19": { "status": "passed_mechanism_scope_with_frozen_source_reference", "evidence": ["artifacts/benchmarks/p5e-formal-runs-20261001-v4/"] },
    "V20": { "status": "…", "evidence": ["…mixed paired + lifecycle-cost…"] }
  },
  "full_retrieval_gate": "not_passed_known_failures_retained",
  "artifacts": ["…全部引用件与新采件路径…"],
  "limitations": [
    "原 source/intent Partial 与 S11 失败原样保留；不称完整检索认证。",
    "formal-v4 主体证据测量源为 v5 闭包（78f83f0f...）；冻结源差异 = c4 修复判据复测补强（或未修复时如实声明 c4 对比证据不存在）。",
    "无真实 provider/holdout/100k/跨平台/发行认证；G7/G8 不在本轮。"
  ],
  "c4_disposition": "fixed_verified | blocked_evidence_workload_inherent_race（二选一，与 2.0 裁决一致）",
  "freeze_anchor": "F0",
  "independent_audit": "<审计收据路径>",
  "next_phase": "P6 batch 1（F0 后 crates/ 零改动直达批次 1 首个提交）"
}
```

### 4.3 独立审计要求

仿 round07 双层结构，独立审计人（非实施会话）至少核验：

1. **冻结断言重放**：F-1 断言脚本对 source-v6 manifest 全量重跑（629 条逐条 `git cat-file` 回对 head），0 stale；digest 口径按 `G5-EXECUTION-APPROVAL-final-v5.json` `source_digest_basis` 重算 MATCH。
2. **新采件 raw 保真**：mixed-c4 判据复测与回归日志 mtime census（实施完成后无新增写入）；逐命令 exit code 与 GATE `toolchain_tests` 数字一致；参照 R2 的"复放是核验不是延迟重测"口径。
3. **引用件链锁**：formal-v4/R1/R2/round07 四件 hash 复验 MATCH（参照 `formal-v4-quality-acceptance.json:9-15` 的 chain_locks_reverified 方法）。
4. **验收门逐条对账**：4.1 节 8 条逐条 PASS/FAIL 表；任何 not_run/blocked 项在 GATE 中显式列出。
5. **红线对账**：v4/rerun-r1 raw 零触碰、v5 manifest 零回改、无 git commit、tasks.json 仅回填 P5-019/P5-020/P6-001 状态与 evidence。

审计收据落 `artifacts/checkpoints/p5-020-playbook-20261002/` 同级新目录（如 `g5-independent-audit-<date>/AUDIT.json`），路径回填 GATE `independent_audit` 字段。

---

## 5. 时间线估算（基于 formal-v4 / final-v3 实测）

| 步骤 | 实测依据 | 估算 |
|---|---|---|
| c16 单跑（run_c16_final.py） | v4 seq23 mixed_c16_baseline 20.0s + paired 0.3s（`STAGE-RECEIPTS.json`） | 测量 ~40s；静默窗口等待 0–2h 有界（上一轮 2h 未得窗，`c16-QUIET-WATCH.md`，为外部变量不计入主线） |
| c4 修复实施 + 红绿（人工） | 方案 d ≤3 文件 ~50 行 + 新用例（c4 文档 §5/§6） | 0.5–1 天（含 TDD 往返）；不纳入路径为 0 |
| 定向测试（第 2 步三命令） | fence/freshness/cc-eval 子集，参照 focused 组量级 | 3–8 min |
| mixed-c4 判据复测 + facet + fanout（3b） | v4 实测：mixed 单 stage ~19–20s ×2、facet 0.4s×8、fanout ~21s×2（`STAGE-RECEIPTS.json`） | ~2 min 测量 + 新批准收据 ~15 min |
| source-v6 冻结 + release 构建 | manifest hash 秒级；release 冷构建收据无耗时字段，参照 final-v3 同机 cargo 量级（278–616s 组）预留 | 冻结 ~10 min；构建预留 5–10 min（CARGO_BUILD_JOBS=2） |
| 双工具链全回归（第 5 步七命令） | final-v3 实测：clippy 60.0+39.4s；workspace 616.0s（stable）/278.0s（1.95.0，暖缓存）；cc-eval 374.5s/189.6s；release cost 120.7s（`final-v3/validation.json` commands[].seconds） | 冷态 ~25–30 min；暖态 ~14 min |
| V18 契约 + 探针（第 6 步） | real-mcp 25 用例 + protocol 1，p5d final-v3 实测秒级组 | ~5 min |
| GATE 装配 + tasks.json 回填 | 文档工作 | ~30 min |
| 独立审计（4.3 节） | R2 全量复放耗时对照：25 项检查含 1224 请求 census | ~1–2 h |

**总估算（纳入 c4 修复路径）**：测量+装配约 **2.5–4 小时**（不含 c16 静默窗口等待、不含 c4 修复人工 0.5–1 天）。**不纳入路径**：约 **1.5–2.5 小时**，但 P6 批次 0 面临 F0 作废重走风险（`IMPLEMENTATION-ORDER.md:30`），期望总成本更高。

---

## 6. 红线（执行本手册全程有效）

1. 不推断 P5-019/P5-020/G5/M2 已完成；一切以收据为准。
2. 不改 v4/rerun-r1 raw、R1/R2 收据、v5 manifest 与 `78f83f0f...` 锁定链、final-v8 harness inputs。
3. c4 判 BLOCKED 时不得 rerun-to-green；不降窗跑（load≥3 不跑 mixed）。
4. 不放宽 known-red 清单、gold、阈值；原 Partial/S11 失败原样保留。
5. 不删 Partial 或改证据换绿；重跑只落新 run-id，不覆盖旧 raw。
6. 全程不 git commit，除非用户明确授权。
7. semantic 相关：默认构建零网络零 key；不冒充真实语义效果。
