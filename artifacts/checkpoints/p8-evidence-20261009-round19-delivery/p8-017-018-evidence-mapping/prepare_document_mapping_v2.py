#!/usr/bin/env python3
"""Add peer-reviewed precise locators and correct an unsupported SDK-field claim."""
import hashlib,json
from pathlib import Path
P=Path(__file__).resolve().parent
old=P/'P8-018-four-document-existing-evidence-map.json'
peer=Path('/dev/shm/a217aaae3bde/platform-review/round19-P8-018-platform-mapping-scoped-review.json')
pub=Path('/workspace/scratch/a217aaae3bde/checkpoint-round19-prep/original-39-complete-published.json')
def rec(p):
 b=p.read_bytes();return {'path':str(p),'bytes':len(b),'sha256':hashlib.sha256(b).hexdigest(),'git_blob_sha1':hashlib.sha1(b'blob '+str(len(b)).encode()+b'\0'+b).hexdigest()}
before={str(p):p.read_bytes() for p in [old,peer,pub]}
j=json.loads(before[str(old)]);review=json.loads(before[str(peer)]);publication=json.loads(before[str(pub)])
assert review['input_mapping']['sha256']==rec(old)['sha256']
assert publication['commit']=='44745440dbe5b81666e80d2f46a8cefe29feb7b8'
j['schema']='P8-018-existing-E-four-document-and-V18-V21-mapping-v2'
j['previous_v1']=rec(old)
j['platform_locator_independent_review']=rec(peer)
j['revision_scope']=['保留原 v1；仅收紧未被原收据支持的 OS SDK metadata 描述，并追加实际原 member/hash/JSONpointer。','不把 SDK 偷换为 MCP rmcp；原平台 SDK blocker 按实际受测组合的构建成功解释，未观测的 OS SDK 版本仍未观测。','另附实际原件公开进展快照，不修改旧 pending publication 历史。']
row=next(r for r in j['validation_clause_mapping'] if r['id']=='V21-sdk-msrv')
row['finding']=review['required_correction']['accepted_replacement']
row['limit']='仅已观测 Linux x86_64 / macOS aarch64 × Rust1.95.0/1.99.0 × default/semantic 八组合，实际新 target 冷构建及 stdio 成功。原收据没有 OS SDK 版本字段；不认证任意 OS/SDK 配置，也不以 rmcp 版本替代平台 SDK 含义。'
j['evidence_index']['original_platform_8_cells']['precise_original_member_locators']=review['platform_receipt_locators']
j['evidence_index']['original_rollback_disabled_local']['precise_original_member_locators']=review['disabled_local_original_member_locators']
j['evidence_index']['original_P7_offline_2_packages']['precise_no_key_original_member_locators']=review['no_key_original_member_locators']
j['publication_snapshot_addendum']={'record':rec(pub),'commit':publication['commit'],'tree':publication['tree'],'complete_original_39_zip_parts':193,'runtime_lifecycle_required_parts':44,'runtime_lifecycle_published_parts':31,'runtime_lifecycle_pending_soak_parts':13,'additional_soak_50k_included':False,'prior_19_of_44_snapshot_preserved':True,'claim_scope':'发布对象实际 readback 由原根/独审收据证明；本映射没有远端写入，也不把上传但未在 ref/tree 公开的 parts 计入。'}
j['own_scope_assessment']='原018四文档的计数/schema/工具/关键默认值、设计与历史区分、安装入口说明，以及原V18/V21各条款，现已明确绑定既有准确来源证据。此次只读核对没有证明需要新增产品修改或新的 native 实验；完整任务仍受原017依赖与最终交付门约束。'
j['remaining_formal_conditions']=['原 P8-017 依赖保持开放，本映射不关闭它。','完整原件及 benchmarks 报告的不可变公开/导航仍独立处理；保留所有旧 pending 历史快照。','本 v2 的原字段修正与 locator 追加待 peer 最终窄确认；任务状态保持不变。']
p=P/'P8-018-four-document-existing-evidence-map-v2.json'
with p.open('xb') as f:f.write((json.dumps(j,ensure_ascii=False,sort_keys=True,indent=2)+'\n').encode())
for n,b in before.items(): assert Path(n).read_bytes()==b
print(json.dumps(rec(p),ensure_ascii=False))
