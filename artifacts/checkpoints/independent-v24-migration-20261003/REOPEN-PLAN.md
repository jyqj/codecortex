# 最小普通验证设计（未执行；不能计为通过）

本轮因工具链只读拒绝已停止。以下是可审查的设计，不是重试许可，也不是已跑命令日志。

三份固定 Git源码分别导出到本目录 runtime/v22/source、v23/source、v24/source。各自全新 target-v22/target-v23/target-v24，default `cargo build -p cc-server --bin codecortex --locked --offline --message-format=json-render-diagnostics`。记录source SHA、Cargo.lock/Cargo.toml/产品Rust/SQL源码hash、工具链版本、全部compiler-artifact feature数组和fresh字段、可执行binary SHA256；不复用外部rlib、不链接另一版本、不允许all-features覆盖默认产品。未产生任何target就停止了本轮，因此没有此类亲跑receipt。

每个旧版本独立新建5个普通项目（路径均在本目录runtime里），源码来自audit.py自身model fixture，函数名review_use，固定源码sha256及stat.st_mtime_ns。Python `_`/`__`/`℘`/`℮` 用ast确认合法注解指向唯一class；TypeScript `$` 用最小声明语法确认。期待的UsesType目标从源码结构派生，不从作者expected或数据库after观察派生。

旧binary普通MCP initialize，然后tools/call index(full:false)。校验真实DB版本v22/m1或v23/m2、files1、保存原source hash/mtime、DBgeneration及sqlite只读快照。v22合法边应存在；v23无合法type边为源码缺陷负对照。退出用正常stdin EOF并等待，若超时报告阻断，不使用kill/fault injection。新binary重新initialize，auto_index关闭以分离两个事件：初始化观察schema24/files0、新非零incarnation、epoch高于旧值；再普通index(full:false)断言parsed1、files1、manifest3、唯一consumer→class uses_type且target UID与目标symbol UID相等、合法name_bucket存在且没有空key/ellipsis bucket。源码sha和mtime必须逐阶段相等。两次独立新进程reopen/index，断言parsed0/skipped1且generation、SQL rows、manifest digest稳定。任一步权限拒绝停止对应动作。

自己的普通regression追加小模型：Go `f(){ a.B(); a.C() }` 和 `f(){ a.B().C().D() }`，通过源码位置映射独立预期不同call site，断言manifest无重复site_id；Python `class Item: pass; def review_use(x: tuple[Item, ...])`（合法换行缩进）预期Item边有UID、tuple为保守外部类型，无`...`语义边或依赖、无空key。每项再普通no-op与reopen；只跑cc-model manifest版本测试、cc-index类型/Go小范围regression、cc-db普通schema guard/reopen测试。不得选GC/WAL/fault或semantic_runtime test target。

pinned普通读者设计另用v22和**真正v23旧产品源码**的独立probe（记录probe源码hash、rlib及feature身份、binaryhash）。旧产品实际生成库，保持QueryHandle活跃并用同一个`review_use`查询预热，释放SQL lease，不持有事务。新default产品普通迁移同一库且不改源码：旧handle读取新generation、同一query重查不复用旧generation缓存，结果包含新目标类型证据；旧reader的resolution_manifests明确拒绝v3。同时新v3 reader通过真实旧payload检查拒绝v1和v2，不手工改DBversion伪造实际旧库；独立payload版本矩阵通过对象validate测试验证。equal-epoch/different-incarnation是纯模型cache key断言，不冒充生产epoch重置测试。

作者现有v23 pinned reader来自修复same-version bf10b64，审计必须单列此身份，不与57bedba生成者混用；不能声称本轮已替作者补做真正旧v23 reader。
