# Requests / Gin 公开 DEV declaration-kind-v2 准入登记

状态 **admitted_public_dev_opt_in**，版本
`public-dev-pygo-declaration-kind-v2`。这是 root 在用户委托项目决策范围内
采用的声明分类；[采用决定](root-decision.txt)绑定来源 thread 与决定范围。
仅适用于 Requests/Python、Gin/Go 的公开 DEV 输入，不构成 holdout 准入、
产品通过或四 repo 统一规范。Requests 历史宽泛 function 有依据，Gin 原约定
仍未确定；不追溯宣称历史 gold 错误。具体规则沿用固定候选 manifest。

候选 `97c478cdb05d2bb85f852ff42af5452b00ba6e3a` 与独审
`47706907868007f71246f74674164263019bba79` 的全部文件原样保留；它们的
not_admitted / not_decided 是当时状态。新 receipt 追加 root 的后续采用决定。
独审来源 PR13 及用户明确决定的表述按 [追加勘误](ERRATA.md)理解，真实来源
PR133 `6c1416109003bcff0c1911307a4af5bd48870517`。原准入 PR91
`5385f5a7a2a875c6d5cbd049bdde039bf71bbf32` 不变。

## 固定产物与范围

- `admission-receipt.json`：决定、候选、独审、原准入、input/license pins、
  source author Git SHA、原 suite inventories，以及重算的 source 原字节 SHA256。
- `selector.json`：仅显式 opt-in；历史 default 不替换，不接受子集或额外 repo。
- `selector.py`：从固定 Git 原字节重放 166 个精确 kind token 位置，只在内存
  返回派生 native 输入；反向补丁必须逐字节恢复原文件，compat 返回原 bytes。
- 固定 `change-manifest.json` 是 166 个 before/after 的权威位置表，包含
  query/group/alternative ordinal、JSON pointer、line/file byte offsets、行与
  alternative 哈希、source/owner 绑定；receipt 直接绑定其原 Git entry 与哈希。
  不复制 query/gold 正文，不另造位置表替代固定证据。

| 范围 | Requests | Gin | 合计 |
| --- | ---: | ---: | ---: |
| native DEV | 91 | 67 | 158 |
| compat DEV（原字节） | 83 | 55 | 138 |
| kind delta | 85 | 81 | 166 |
| 有 delta 的行 | 58 | 43 | 101 |

独审已验证 268 alternatives、41 声明绑定 source、9 法律文件。本登记另核对
原 suite 完整 73 文件 inventory 的实际原 Git bytes。既有外部 source/套件
pins 保留；新增每文件 SHA256 是独立重算值，不充当外部 pins 的替代或认证。
旧 gold/admission/old1671/allPartial/raw/thresholds/renderer/source 不改。
301 native / 256 compat / 280 相关组是历史规模，无新独立样本。

## 后续 paired runner 加载接口

两个 runner 使用同一固定本登记 commit、相同显式版本和相同原 source bytes。
用 `importlib` 或普通 Python import 加载本目录 `selector.py`，调用：

```python
packages = selector.load_version(
    version='public-dev-pygo-declaration-kind-v2',
    repositories=['requests', 'gin'],
    source_bytes=actual_runner_source_bytes,
)
```

`actual_runner_source_bytes` 是 **完整 entry -> bytes** map；entry 为 receipt
中的 `source_bytes_sha256` key，bytes 必须读取实际 runner 将加载的 corpus。
不可只提供哈希或以 `source_inputs()` 替代实际 corpus 校验；`source_inputs()`
用于离线 Git 重放。缺失 Git 对象、错误外部 pin、source/author 漂移、source
缺项/增项、其它 repo 拼混、错 version、subset、duplicate 都拒绝。

返回 `packages[repo]` 的 `native`、`compat` 是精确输入 bytes；`suites` 是两个
**历史 suite 原 bytes 模板**，`source_bytes` 是核验过的原 source，`input_lock`
给出 author SHA 与前后文件 SHA256。每个 runner 在独立临时目录加载这些输入，
保留原 scorer、seed、top_k、repetitions、warmup、engine_config 及全部其它设置。
由于 cc-eval suite 的 `queries_digest` 使用 BLAKE3，native 新 suite 必须由
runner 对返回 native bytes 计算 BLAKE3，仅更新该字段及隔离目录所需路径；
不可把 receipt 的 SHA256 放入 BLAKE3 字段。compat 沿用原 queries_digest。
保存原 suite pins、派生 suite/config pins 与本 receipt/selector pins，核对
source digest 等于历史值；比较两边输入锁完全一致，再执行 paired run。
loader 本身不写 query/gold、不运行 scorer，也不启动产品检索。

在另一 owner 提供 qname schema25 **fixedsource** 及二进制 pin 前不跑新结果。
baseline/candidate 都加载本版本，不能拿历史原 gold 分数与新版本单边比较。
不合并四 repo 分数，不写中央产品 done，不改变排名、raw 或评分阈值。

## 离线验证

```sh
D=artifacts/checkpoints/public-dev-declaration-kind-admission-20261003
python3 "$D/selector.py" --version public-dev-pygo-declaration-kind-v2 \
  --repositories requests gin
python3 "$D/test_selector.py"
git diff --check
```

stdout 仅聚合计数与 pins。测试包含真实固定输入重放、错 pin、错 source、
错 author、其它 repo 拼混、subset/duplicate、非 kind 字段与重封 selector
负例。验证结果见 `validation-receipt.json`；全部新文件由
`artifact-manifest.json` 封存。登记独占此新目录，draft PR 仅用于审阅，
不 merge/deploy。
