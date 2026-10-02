# P6-008 实施记录：构建内容寻址 artifact cache

- 日期：2026-10-02
- 依据：`artifacts/checkpoints/p6-implementation-planning-20261002/TASK-BRIEFS.md`
  P6-008 节（布局、checksum 独立验证、原子写、Corrupt 分支）、
  `docs/adr/0003-semantic-persistence-single-db-boundary.md`（第 99-103 行：内容寻址、
  可逐条校验、损坏可检测、跨项目默认隔离、无秘密；第 144 行约束表）、用户 Q5 决策
  （2026-10-02 拍板）：**全局 cache 目录 + 项目身份 namespace，不绑 incarnation**、
  `OPEN-QUESTIONS.md` Q5/Q6、`crates/cc-semantic/docs/ENCODING-SPACE.md`（版本进
  SpaceDigest；ArtifactRef 构造器归属 cache）。
- 改动范围：`crates/cc-semantic` 3 个既有文件修改 + 1 个新模块 + 1 个新集成测试文件 +
  `Cargo.toml`/`Cargo.lock` 依赖边。零 schema 变更；`cc-db`（outbox/lease，P6-006/007
  交付）零触碰；`tasks.json` status 未改；artifacts 冻结链未触碰；未 git commit。

## 0. Q5 决策引用（namespace 与根目录）

> 用户 2026-10-02 拍板：全局 cache 目录 + 项目身份 namespace
> （`~/.cache/codecortex/semantic/<project-identity>/` 形态），跨克隆共享、跨项目
> 隔离、**不绑 incarnation**——P6-014 重建（换 incarnation）解析出同一 namespace，
> 已付费向量经 `(space, input, spec)` 三 digest 校验后直接复用；fencing 职责全部
> 留在主库侧，cache 内不含任何 incarnation 字段。

本实现逐条落地的位置：

| Q5 要素 | 落点 |
|---|---|
| 全局根目录 | `resolve_cache_root`（cache.rs:131）：macOS `~/Library/Caches/codecortex/semantic`，Linux `$XDG_CACHE_HOME\|~/.cache/codecortex/semantic`，env 覆盖 `CODECORTEX_SEMANTIC_CACHE_ROOT` 优先 |
| 项目身份 namespace | `namespace_key(identity)`（cache.rs:76）= `blake3("cc-semantic.cache-namespace.v1", identity)` hex；目录 `<root>/namespace-<ns>/`（简报布局原文） |
| 跨项目隔离 | namespace 是路径首层分隔；两 namespace 互不可见（测试 6） |
| 不绑 incarnation | `ObjectMeta` 七字段与 `artifact_ref` 均无 incarnation；重建后同 namespace 命中 |

**项目身份来源与解析顺序**：今日工作区内唯一既有 project identity 来源是规范项目
路径（`cc_model::config::project_cache_key` 的同一来源），由组合根（未来接线轮）以
字符串形式传入 `namespace_key`——即**身份当前不来自库内记录**，cache 打开不依赖
`index.sqlite3` 状态，不存在"先有库才有 cache 键"的顺序约束。若未来身份移入权威
记录，解析顺序变为：open DB → 读身份 → `namespace_key` → `ArtifactCache::open`；
本模块不需要改（身份只是输入字符串）。`namespace_key` 输出为 64 位 hex，绝对路径
不落入 `~/.cache` 目录名。

## 1. 文件:行号改动清单

| 文件 | 变更 | 内容 |
|---|---|---|
| `crates/cc-semantic/src/cache.rs` | 新增（全文） | 模块文档（边界/无秘密/durability 范围声明）+ `CACHE_FORMAT_VERSION`/`NAMESPACE_DIR_PREFIX`/`QUARANTINE_DIR`/`CACHE_ROOT_ENV` 常量 + `namespace_key`(:76) + `validate_namespace`(:84) + `resolve_cache_root`(:131)/`resolve_cache_root_with`(:144) + `ObjectMeta`(:165) + `ValidatedVector`(:184)/`CacheRead`(:192)/`CorruptReport`(:201) + `ArtifactCache::{open,get,put,discard}`(:211-404) + `atomic_write`(:408) |
| `crates/cc-semantic/src/lib.rs:23` | 修改 | `pub mod cache;`；模块边界文档更新：cache 机制已落位（open 无副作用、put 惰性建目录），`try_init` 与组合根接线仍缺位（归独立接线轮），默认构建/启动仍不触文件系统 |
| `crates/cc-semantic/src/types.rs:102-109` | 修改 | `ArtifactRef` 文档：唯一公开构造路径 = `ArtifactCache::put`，格式 `cas.v1:<ns>:<space>:<input>:<spec>:<checksum>`（P6-003 留白兑现） |
| `crates/cc-semantic/Cargo.toml` | 修改 | 增加 `serde_json.workspace = true`（meta.json 序列化；workspace 已有，无新外部依赖） |
| `crates/cc-semantic/docs/ENCODING-SPACE.md:53` | 修改 | 一行失真文本订正：`ArtifactRef` 无公开构造器 → 构造路径为 P6-008 `ArtifactCache::put` |
| `crates/cc-semantic/tests/artifact_cache.rs` | 新增 | 17 个集成测试（§4） |
| `Cargo.lock` | 变更 | `cc-semantic` 依赖边（serde_json）+ `cc-server → cc-semantic` 补录（后者是先前会话 `crates/cc-server/Cargo.toml` 未提交改动的 lock 补齐，本轮 `cargo update -p cc-semantic --offline` 重生成时一并落盘，语义为既有 optional `semantic` feature 边） |

## 2. 机制要点

1. **寻址即路径**（简报原文口径）：`(namespace, space_id, input_digest,
   doc_spec_digest)` 四元组定位对象 `<root>/namespace-<ns>/<space>/<input>/<spec>.bin`
   + `.meta.json`；**checksum 不进路径**，是 `.meta.json` 的独立字段，读时
   `blake3(.bin)` 对比（ADR："namespace 与 input/spec/space/checksum 分开验证"）。
   payload 为 little-endian f32 序列（简报指定）。
2. **读时验证链**（`get`）：meta JSON 可解析 → `format_version` → meta 三 digest
   与被寻址四元组一致 → checksum → payload 字节长 = dimension×4 → dimension/model_id
   与冻结 `VectorSpace` 一致 → f32 全有限。任何一步失败得 `CacheRead::Corrupt`
   （带 path+reason），**既不是 Err 也不是部分 payload**；bin/meta 任一缺失 = Miss
   （半写状态与"从未写过"不可区分，meta 后写即完备性标记）。
3. **原子写**（`atomic_write`）：同目录唯一临时名（pid+序号）→ write_all →
   `sync_all` → `rename` 原子替换 → 父目录 fsync（best-effort）。并发写同 key 收敛：
   内容寻址使 payload 恒同，rename 后读到完整对象（测试 9）。**不宣称 crash-proof**
   （简报风险段：macOS/Linux fsync 差异归 P6-015 真实 kill/restart 验证）。
4. **可丢弃语义**：`discard()` 删 bin+meta 两半，后续读 Miss、返回是否删除。008 的
   `Corrupt` 检测**不自动删除/隔离**（简报："008 不隔离，018 隔离"；quarantine 搬移
   归 P6-018，`quarantine_dir()` 只报路径不创建），调用方以 discard 显式降级。
5. **无秘密红线**：payload = 纯向量字节；meta 七字段闭集
   `{format_version, dimension, model_id, checksum, created_at, input_digest,
   space_id, spec_digest}`（`deny_unknown_fields`，测试 14 断言键集恰等）；put/get
   路径**零环境变量读取**（env 只在根目录解析 `resolve_cache_root` 中出现）；无任何
   凭据/API key/token 字段或读取路径。
6. **`ArtifactRef` 构造器落位**（P6-003 留白兑现）：唯一公开构造路径 =
   `ArtifactCache::put` 返回值，格式 `cas.v1:<namespace>:<space>:<input>:<spec>:<checksum>`
   （ADR 第 101 行地址元组含 checksum）；`types.rs` 的 `pub(crate) new` 密封不变，
   跨 crate 仍不可伪造。P6-014 重建补 manifest 时经 `get` 的 Hit 取回同一 ref。
7. **懒创建**：`open` 只校验 namespace，零文件系统副作用（P6-002 验收"不生成空
   缓存目录"延续，测试 1）；首个 `put` 才 `create_dir_all`。`try_init` 与组合根接线
   本轮不做（红线：接线归后续轮），默认依赖图不含 cc-semantic（实测
   `cargo tree` 两处计数 0）。

## 3. 与 P6-014 的复用关系（简报要求的显式说明）

namespace 不含 incarnation ⇒ P6-014 staging 重建换库后：新库 incarnation 全新、
旧行全部 fencing 拒绝，但 cache 侧四元组与 incarnation 无关 →
`get(space, input, spec)` 对重建前的付费向量直接 Hit，manifest 按 artifact_ref 补链，
零重复嵌入费用。空间回滚（P6-017）同理：旧 `SpaceDigest` 对象未被触碰，校验通过
即可回滚复用。cache 损坏时该不变式靠读时验证链兜底：任何字节级漂移都是 Corrupt
而非错值（ADR "只降级不污染"）。

## 4. 测试清单（`crates/cc-semantic/tests/artifact_cache.rs`，17 个，全绿）

1. `open_creates_nothing_and_put_lays_out_the_brief_layout`——open 零副作用 +
   简报布局逐层断言（含 `.bin`/`.meta.json`）。
2. `read_write_roundtrip_is_lossless`——put→get 数据逐值相等、dimension、ref 一致。
3. `same_input_hits_across_independent_cache_instances`——跨克隆共享语义：独立句柄
   同 root+ns，先 Miss、一方 put 后另一方 Hit；异输入不误命中（验收 1）。
4. `namespaces_are_isolated`——两 namespace 互不可见（跨项目默认隔离）。
5. `different_spaces_never_cross_hit`——同输入异模型空间 Miss（V16 存储层延续）。
6. `spec_change_routes_to_a_new_object`——DocSpecDigest 变化 → 新对象，旧对象不受扰。
7. `corrupt_payload_is_detected_and_discard_degrades_to_miss`——flip 字节 →
   Corrupt(checksum) → 检测不破坏现场（008 不隔离）→ discard → Miss → 空 discard
   返回 false（验收 2 + 可丢弃语义）。
8. `meta_tampering_is_detected_as_corrupt`——五分支：(a) 截断 payload（重算
   checksum 使 dimension 分支精准命中）；(b) space_id 改写 → addressing；(c)
   format_version=99；(d) meta 非法 JSON；(e) bin 缺失 = Miss。
9. `concurrent_puts_of_the_same_key_race_safely`——8 线程同 key 并发 put，全
   Ok 且 ref 恒同，读回 payload 完整。
10. `rewrite_of_existing_object_is_idempotent_and_stable`——重复 put 同 ref 同内容。
11. `namespace_validation_rejects_unsafe_names`——空串/空格/`/`/`..`/`.hidden`/
    含空格/含 NUL/超长全部 open 拒绝且零落盘（防目录穿越）。
12. `namespace_key_is_stable_distinct_and_path_safe`——稳定、异身份异值、64 hex、
    空白身份拒绝、可作 namespace 打开。
13. `artifact_ref_is_stable_and_content_scoped`——同元组恒同 ref、`cas.v1:` 前缀、
    input/space/spec 任一分量变化 → 异 ref、cache 是唯一公开构造路径。
14. `invalid_vectors_are_rejected_before_touching_the_cache`——错维/NaN/Inf 拒绝
    且零落盘（坏向量进不了 cache，V15 门禁语义衔接）。
15. `meta_contains_only_the_documented_fields`——meta 键集恰等七字段 + payload
    字节级等于 le-f32 序列（无秘密红线实证）。
16. `cache_root_resolution_branches`——env 注入（闭包 seam）：override 两平台皆优先、
    空白 override 视为未设、macOS `~/Library/Caches` 分支、Linux XDG/`~/.cache` 两
    分支、无 HOME → None（`macos_layout` 参数使 Linux 分支在 macOS 可测）。
17. `process_env_override_reaches_resolve_cache_root`——真实进程 env
    `CODECORTEX_SEMANTIC_CACHE_ROOT` 注入/恢复（互斥锁防并发 env 竞争）。

## 5. 验证结果（原样）

```
SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk \
  cargo test -p cc-semantic --locked --offline
running 22 tests  ... test result: ok. 22 passed; 0 failed   （lib：P6-002/003 既有 22 个）
running 17 tests  ... test result: ok. 17 passed; 0 failed   （artifact_cache.rs 本轮 17 个）
running 0 tests   ... test result: ok. 0 passed              （doc-tests）

SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk \
  cargo check --workspace --locked --offline
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 10.06s   （零 warning）
```

默认依赖图复查（2026-10-02 收口轮订正：原命令 flag 无效，结论经有效命令复验成立；
`--no-dev-deps` 非 `cargo tree` 有效 flag，而 `--no-dev-dependencies` 与 `-e normal`
互斥，故采用 `-e normal` 口径——该口径本身即只含 normal 依赖边）：真实输出
`cargo tree --workspace -e normal | grep -c cc-semantic` = 1（唯一一次为 workspace
成员根行，0 条依赖边指向它）；`cargo tree -p cc-server -e normal | grep -c
cc-semantic` = 0（cc-semantic 仅经未启用的 `semantic` feature 可达）。

## 6. 偏差清单（对简报接口草案）

1. **`put` 返回 `ArtifactRef`**（草案：`String`）：兑现 P6-003"由 P6-008 cache
   构造"的留白，类型化密封优于裸字符串。
2. **`get`/`put` 收 `&VectorSpace` 而非 `&SpaceDigest`**：digest 在内部派生。
   只收 `SpaceDigest` 无法取 dimension/model_id，meta 七字段无从填写、读时
   "dimension/model 与冻结空间一致"这一层校验不存在——ADR 要求读时验证 spec/space，
   故收更强的冻结类型；`SpaceDigest` 仍是路径寻址键（内部派生，测试 5 守护）。
3. **`put` 增加 `now_unix: i64` 形参**：`created_at` 需要时钟；沿 P6-007 纪律
   （时钟由调用方传入，SQL/IO 层不取时钟），测试因此完全确定。
4. **`ValidatedVector.artifact_ref` 类型为 `ArtifactRef`**（草案：String），同偏差 1。
5. **Corrupt 不自动删除**：任务提示"损坏对象按可丢弃语义处理（删除+视为 miss）"
   与简报"008 不隔离，018 隔离"并存落法 = `get` 只检测报告（简报权威）+
   `discard()` 显式删除降级原语（提示语义），隔离搬移归 P6-018。
6. **namespace 为 blake3 域分隔摘要而非明文身份**：绝对路径不落 `~/.cache` 目录名，
   固定 64 hex 天然路径安全；身份字符串来源归调用方（§0 解析顺序说明）。
7. **`try_init`/组合根接线未落**：红线明确本轮只落 cache 机制本身；P6-002 验收
   "try_init 与任何 cache 路径在 P6-008 前不存在"的解禁点仅覆盖 cache 模块，
   接线归后续轮，`lib.rs` 文档已同步该边界。
8. **`Cargo.lock` 补录 `cc-server → cc-semantic` 边**：先前会话 `crates/cc-server/
   Cargo.toml`（未提交改动）的 lock 补齐，随本轮 lock 重生成落盘；非本轮新增依赖。
