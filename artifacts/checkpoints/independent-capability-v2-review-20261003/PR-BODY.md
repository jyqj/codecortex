本 PR 只增加固定 production 11af963c / final 29b03a0 / base 6db4d396 的单一独立审查证据，结论为 bounded pass。接受已批准的 point-in-time API；独立验证完整旧/新 snapshot，并继续拒绝旧 root + 新 coverage。

自编 7 unique tests / 9 positive matrix executions，3 个实际编译 mutants 被断言击杀，恢复后 7 executions 通过。实际 FakeProvider normal publish、pool1、Linux HAS_MOVED normal rename/unsupported fail-closed、service priority 和原生产 stdio search/context strict query fence 已验证。源码/二进制/features/失败 fixture 日志均可审计。作者 46 executions 与 AB 不计独立信用。

当前 CI 编译 actual default/semantic production，但没有 explicit semantic-http CI test command；本次已实际编译运行该 feature。跨平台 VFS、100k、general performance、GC/WAL faults/heldout 未覆盖。没有生产/版本/ledger 修改。
