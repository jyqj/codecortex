# P8 本地候选锁与证据归档

## 范围与阶段状态

`scripts/p8_release_evidence.py` 提供 Python 标准库实现的 `freeze`、`verify`、`archive`
三个命令，推进 P8-001 的候选输入冻结与 P8-019 的原始证据归档。运行不启动产品进程、
不编译、不联网、不调用 provider，不读取 holdout 正文，也不修改被测源码。

这是 [04-PHASES.md](04-PHASES.md) 允许提前开展的 **local engineering 子 profile**。
工件固定声明 `scope=local_engineering_only`、`release_certified=false`。
P7-020、P8-018 及完整 G8 尚需各自验收；本地脚本和反例通过不将这两项父任务自动标记 done。
它复用已有 runner 的 gate 状态及退出码，直接保存原始产物，不新增评分器或检索引擎。

## 候选具体锁住什么

| 输入 | 记录与验证 |
|---|---|
| 源码 | Git HEAD、tree、显式相对路径范围、HEAD 与暂存区 blob/mode、当前文件 SHA-256/字节数/可执行位；保留新增和删除 |
| 新文件 | 声明范围内未跟踪及被 `.gitignore` 忽略的新文件也纳入；冻结后新增即失效 |
| binary/config/corpus/scoring/model | 精确文件字节及 SHA-256、原始绝对路径、独立拷贝、可执行位 |
| 显式 corpus-root | 逐文件内容清单、独立快照及 actual content SHA-256；query/gold/source 正文增删改均使候选失效 |
| 构建声明 | `release/debug/fixture` 与 feature 列表；字段明确为 `operator_declaration_not_build_proof` |
| 环境 | OS/release/architecture、Python、CPU 数及下方显式变量允许表 |
| 模型 | 仅接受 disabled 或 fake；fake 必须给出 model_id/revision/encoding_space |

环境变量只读取 `LANG`、`LC_ALL`、`TZ`、`CARGO_BUILD_JOBS`、`RAYON_NUM_THREADS`，
逐项记录缺失值，不导出整个进程环境。变量值有长度与字符限制。config 必须是 JSON object，
拒绝已知凭据字段，如 `api_key`、`authorization`、`password`、`access_token`。
调用者仍负责提供不含凭据的配置、源码和 raw；脚本不声称任意日志都能自动脱敏。

`--source-path` 为必填、可重复的明确范围。不得重叠，不能用 `.` 或 Git 通配符隐式扩张；
Git 查找使用 literal pathspec。范围外文件不属于本锁；HEAD 改变仍会使候选失效。
暂存区有冲突、子模块或 symlink 的范围拒绝冻结。文件级删除会留在 manifest，不能通过
仅散列当前存在的文件隐藏删除。工具不会自动判断 Cargo 的完整传递构建依赖，调用者应纳入
Cargo.lock、Cargo.toml、所有生产 src/build.rs、构建配置及实际 runner 输入。

**binary 摘要与源码摘要同时出现，只证明这批被提供的字节被固定。**
这不是 binary 由该源码构建的证明，也不能验证 build profile/features 的真实性。
正式发行仍需外部构建收据、工具链/MSRV、当前生产 SHA、完整产品回归及发布 gate。
已有构建入口可参考 `scripts/p7_stdio_build_receipt.py` 与
`scripts/resource_harness/build.py`；本工具不复制这些构建流程。

## 准备并冻结

先在隔离工作树完成构建。冻结后不再原地改写源码、配置或 binary；需要变更时新建候选。
以下路径为使用示意，必须替换为本次实际输入，不是本轮已完成发行的收据：

```sh
python3 scripts/p8_release_evidence.py freeze \
  --source-root /absolute/isolated-codecortex \
  --source-path Cargo.toml \
  --source-path Cargo.lock \
  --source-path crates/cc-server/src \
  --source-path crates/cc-server/Cargo.toml \
  --binary /absolute/exact-build/codecortex \
  --build-profile release \
  --config /absolute/inputs/config.json \
  --corpus /absolute/inputs/dev-corpus-lock.json \
  --corpus-root /absolute/public-dev-corpus \
  --scoring /absolute/inputs/scoring-version.json \
  --model /absolute/inputs/model.json \
  --output /absolute/candidates/local-001
```

示例只展示参数形状，不代表四个 source-path 已覆盖本仓全部构建依赖。`--feature` 可重复。
`model.json` 的离线声明是 `{"mode":"disabled"}`；fake 声明例如：

```json
{"mode":"fake","model_id":"fixture","revision":"v1","encoding_space":"fixture_4d"}
```

本地 profile 的 `--corpus` 输入是**显式 development/fixture 元数据锁**，格式包含：

```json
{
  "split": "dev",
  "corpus_sha256": "<64 lowercase hexadecimal characters>",
  "query_sha256": "<64 lowercase hexadecimal characters>",
  "gold_sha256": "<64 lowercase hexadecimal characters>"
}
```

也接受 `split=fixture`。这些摘要由现有语料准备流程声明，不能仅靠字段存在声称正文已验证。
`--corpus-root` 必须显式指向获准的 public dev/fixture 文件目录；本脚本只遍历这个目录，
不解引用元数据中的路径，也不解析或执行 query。应把实际 query、gold 和语料源文件一起放入该范围。
它逐文件复制与验证，并将实际文件清单的摘要单列为 `corpus_content.content_sha256`，
状态为 `verified_explicit_corpus_files`。正文改变而元数据不变，同样会导致 verify/archive 失败。

为保留准备阶段用途，可省略 `--corpus-root`，但此时 manifest 明确记录
`unverified_corpus_content`，不具备完整本地内容锁，**即使 gate 声明 passed_local 也不能提升 latest**。
无论哪种模式，都不将调用者提供的 corpus/query/gold 摘要自动当作已与正文一一核验的结果。
实际 runner 必须继续执行语料分类、query/gold 对应及源码准入验证，相关锁和验证报告应放入 evidence。
保留已有 runner 的摘要算法与格式，不能把本脚本的 SHA-256 当成旧 BLAKE3 值直接比较。

输出包含 `candidate.json`、`source/`、`inputs/` 及显式启用时的 `corpus/`。所有输入复制前后核对，失败保留
`INCOMPLETE.json` 及已写内容；不复用失败目录。成功 JSON 返回 `candidate_sha256`，
它是除该字段外规范化 candidate manifest 的 SHA-256。

```sh
python3 scripts/p8_release_evidence.py verify \
  --candidate /absolute/candidates/local-001 \
  --expected-sha256 <the-recorded-candidate-sha256>
```

候选验证同时检查快照自身和原始输入：HEAD/index/内容/增删/可执行位/允许表环境漂移均失败；
启用 corpus-root 时，query/gold/语料源文件的正文与文件清单也必须保持相同。
运行测量前后都应验证。两端核对不是文件系统原子快照，也不能证明两次观察之间没有
瞬态修改；生产测量需静止的隔离工作树，不与编译或源码编辑并发。

## 由测量流程绑定 gate

候选锁完成后，**运行该候选的测量流程或其集成者**将返回的 `candidate_sha256`
写入本次 `evidence/gate.json`，并保留建立关联的命令及构建收据。例如：

```json
{
  "candidate_sha256": "<the-exact-candidate-sha256>",
  "status": "baseline_recorded_not_quality_certified",
  "exit_code": 0,
  "reasons": ["local baseline only; quality/release gates remain open"]
}
```

归档不自动把任意旧报告重新绑定新候选；缺少绑定或绑定不同会失败。归档也不替 runner
计算 gate，不判断人手声明的 `passed_local` 是否充分。正式 CI 集成者应从真实已验收的
本地测量结果产生该状态，不能仅把现有 status 改名以发布 G8 成功。

| gate.status | exit_code | 可更新本地 latest |
|---|---:|---|
| passed_local | 0 | 实际 corpus 内容已锁且显式 `--update-latest` 时可以 |
| passed / baseline_recorded_not_quality_certified | 0 | 不更新 |
| failed / inconclusive / gate_failed | 1 | 不更新 |
| invalid_measurement | 2 | 不更新 |
| cancelled | 3 | 不更新 |

`passed_local` 另要求非空 `raw/`、非空 `metrics.json`、非空 `report.md`；这只验证归档所需
的基本包装完整性，不能替代完整报告 schema/数值复算。失败和取消允许不完整的 raw，保留现有
文件。gate 的额外字段原样入档，状态与退出码不一致、未知状态及 boolean 退出码都拒绝。

## 归档、复核与 latest

```sh
python3 scripts/p8_release_evidence.py archive \
  --candidate /absolute/candidates/local-001 \
  --evidence /absolute/measurements/local-001 \
  --output-root /absolute/archives \
  --run-id local-001 \
  --update-latest
```

实际目录名为 `<run-id>-<binary摘要前12位>-<corpus内容摘要前12位>-<scoring摘要前12位>`；
只有元数据的准备模式使用 corpus 元数据锁摘要替代第二段，但保持正文未验证状态。
包含独立的 `candidate/`、`evidence/`、`archive.json` 和 `checksums.sha256`。
归档先重验当前候选，再复制证据并检测复制期间变化，最后独立校验归档。
任何已存在目录，包括空目录，都拒绝覆盖；失败 gate 的归档仍保留，命令返回其 1/2/3 退出码。
损坏输入/读写失败返回 2，取消返回 3，部分目录保留。

成功的 `passed_local` 归档，候选带实际 corpus 内容锁且显式请求提升后，
才原子替换同一 output-root 下 `latest.json`。
latest 只是 `{run, archive_sha256, candidate_sha256}` 指针；被指向的历史目录不改写。
原生 `passed`、普通 baseline、失败、取消均不提升。现有 latest 若为 symlink 或无关 JSON，
拒绝替换；完整新归档仍可单独复核。该 latest 仅表示本地工程证据，不是发行频道。

```sh
python3 scripts/p8_release_evidence.py verify \
  --archive /absolute/archives/<actual-run-directory> \
  --expected-sha256 <the-recorded-archive-sha256>

# 在该归档目录内可使用独立校验工具：
sha256sum --check checksums.sha256
```

archive 验证完全离线，不要求原工作树、原 binary 或原 evidence 仍存在。它检查完整文件清单、
每份文件摘要、candidate 自身、gate 绑定和 checksum 列表；额外/缺少/改写文件均失败。
`verify` 的 0 只表示完整性验证成功，**不把已归档的失败 gate 变为通过**。
checksums 覆盖全部 payload 和 `archive.json`；checksum 文件本身按规范清单重算。
可以从 archive 取回精确 binary/config 供独立回滚流程核验。

## 文件边界与资源上限

- 只接受普通文件和目录。所有输入/输出路径的 symlink、特殊文件、目录冒充文件、
  `..` 遍历、路径控制字符及自包含输出都拒绝。
- `.git`、`.env*`、含 holdout/heldout 的路径段直接拒绝。corpus 正文只读取显式指定的开发/夹具目录。
- 单文件最大 1 GiB；JSON 最大 16 MiB；每份候选/归档 payload 总量最大 2 GiB、
  最多 20,000 文件/目录项；路径最多 32 段。空目录不是独立校验对象。
- 文件按 1 MiB 分块读取，记录读取前后 stat；发现增长、替换或修改即拒绝。
  Git plumbing 输出上限 8 MiB、单命令超时 30 秒；不执行 fsmonitor hook。
- “不可覆盖”由 CLI 的独占目录创建和验证规则实现，不是文件系统 ACL/WORM 或数字签名。
  SHA-256 能验证字节，不能认证报告作者；可信环境应另行保存返回摘要，并通过
  `--expected-sha256` 固定信任锚。归档内原始路径只作来源记录。

## 本轮验证与未完成范围

```sh
python3 -m unittest discover -s scripts/tests -p test_p8_release_evidence.py -v
```

测试在临时 Git 仓库中验证真实索引/HEAD/文件变化，涵盖全部显式输入漂移、元数据不变的
query/gold/source 正文修改、corpus 文件增删、未验证 corpus 禁止 latest、暂存区变化、
已忽略的新文件、读取期间变更、fake/live 分界、凭据变量不落盘、部分拷贝失败、历史目录不覆盖、
gate 绑定与非零退出码、失败后 latest 保持、路径/特殊文件/读量上限及可离线归档复核。
CLI 子进程和系统 `sha256sum` 也作为独立验证入口；夹具 binary 从不执行，不冒充产品测试。

本轮不提供 Rust 构建来源证明、完整传递构建输入自动发现、指标重算、公开 holdout、100k、
跨平台/MSRV、soak、真实模型收益、完整 G8 或发行批准。对应验收继续记录为未执行/待前置闭合。
