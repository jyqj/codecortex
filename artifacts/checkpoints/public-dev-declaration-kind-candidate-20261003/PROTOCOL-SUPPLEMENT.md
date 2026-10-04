# public-dev-declaration-kind-v2-candidate 协议补充

状态为 `not_admitted`。此文件只声明一个独立版本候选；原准入 PR91
`5385f5a7a2a875c6d5cbd049bdde039bf71bbf32`、原协议、默认选择器、
gold/source-map、原始检索结果与分数继续保留原字节。

## 新版声明分类决定

taxonomy owner 在本任务中明确采用固定 proposal 的 `normative_rule`：

> Class-owned Python definitions (including static/class/property accessors) and Go receiver declarations use method; free/nested-in-function definitions use function. This describes declarations, not runtime callable types.

Python 定义的直接词法声明 owner 是 class 时使用 `method`，包含实例方法、
`staticmethod`、`classmethod`、property getter/setter/deleter，以及异步定义。
函数内的函数定义使用 `function`；函数内局部 class 自己直接拥有的定义仍为
`method`。class 本身不是本次 function→method 变更的对象。
Go 有 receiver 的函数声明使用 `method`，值与指针 receiver 适用同一规则；
普通函数声明使用 `function`。字符串/注释中的伪声明不能成为证据。

这描述声明分类，与访问 callable 时的 Python 运行时类型不同。static method
访问值可以仍是 function；class-owned 规则不会因此变化。没有修改 scorer，
没有为 `function` 增加 wildcard 或 alias，也没有调整 parser、ranking、qname 主链。

## 历史界限

Requests 原 author 对所有非 ClassDef 使用宽泛 `function`，原 source owner 明确
认可这套标注。此候选不追溯宣称原 85 条 gold 错误。Gin 的历史分类约定仍然无法
确定；原 owner 的 source 接受不能被当成原 taxonomy 的明确决定。两者原审查事实
与原分类结论均保留，新决定仅适用于本候选。

仅 Requests 85 和 Gin 81 条原 proposal alternative 进入覆盖层。其余原始
alternative 仍在 source-map 检验控制集中，不能自动变成新版标注。Express 与
TypeScript 仍为历史版本，未在本任务中审定是否遵守新 taxonomy；不产生四 repo
合并分数，也不把 mixed-version 输入称为统一的新版本评测。

301 个历史 native DEV 行、256 个历史 compat 行和 280 个保守全局相关组只是
原准入的历史规模。重复、覆盖或本次分类修订均不产生新的独立样本；原关联关系、
组归属与分母不变。这里不证明这些相关组具有完整语义独立性，不产生 holdout、
正式 600 题准入或新的 accuracy 声明。

## 覆盖层及精确重放

`candidate-gold.json` 只保存 166 个 replacement alternative 的原样 JSON token、
序号和原始绑定哈希，不复制查询正文或 source corpus。其身份依附于固定原始
native 行；它不是供默认 selector 直接读取的新 suite。

`change-manifest.json` 逐条保留原 proposal 的完整 hash/owner/source coordinate
绑定，给出只替换 `/answers/<group>/alternatives/<alt>/symbol/kind` 的行内和
文件内原始 UTF-8 字节坐标、精确前后 token、alternative/answers/line/file raw
哈希。记录中 `binding.applied=false` 是原审查原样绑定；
`overlay_applied_in_candidate=true` 表示新覆盖层已经派生，二者不是准入状态。
source binding 的 before/after 完全相同，gold source-map 原记录也保持原哈希。

重放器先验证固定 review 全部 artifact，再在独占 scratch 中重跑原审查，验证
268 条 alternative、41 个 source 文件、owner receipts、source map 和原 compat
投影。之后用 JSON token 精确定位执行 kind 字节替换。反向还原后的整行必须与原行
逐字节一致，且每行结构的所有非 kind 字段必须一致。因此 query、qname、span、
facet、primary、语言、权重、难度、阈值及任何其它字段都保持原样。
原 native JSONL 派生结果只在内存中存在；文件 raw 哈希独立记录，可离线复算。

这不是行为 gold、facet/图关系或 parser 输出的重新认证。当前产品检索结果未作为
gold 真值，未运行新的检索、评分或 live provider。所有原 raw/score/admission
文件均未写入。许可证/NOTICE 只从已经授权的公开固定 Git blob 复制，逐字节保留。

## 独立准入门槛

全部文件仍为 `not_admitted`。候选交付后停止，由 root 另行独审所有 166 条
source 事实与语义、所有非 kind 字段、source notices 和 exact compat 投影。
只有独立准入明确通过后，才能在另一个版本准入决定中使用；本任务不更改默认
选择器、不合并、不部署，也不把本候选的构造检查称为 root 的独立审查。
