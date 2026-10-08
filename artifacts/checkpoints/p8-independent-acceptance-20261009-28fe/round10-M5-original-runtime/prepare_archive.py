"""Preserve fixed M5 original evidence as opaque bytes; never run product/artifact code."""
import datetime
import hashlib
import json
import lzma
import os
from pathlib import Path, PurePosixPath
import stat
import tarfile
import time

ROOT = Path('/workspace/scratch/28fef0db5e01')
OWN = ROOT / 'acceptance-review/round10-M5-original-runtime'
PREFIX = 'artifacts/checkpoints/p8-independent-acceptance-20261009-28fe/round10-M5-original-runtime'
OUT = OWN / 'delivery' / PREFIX
PART = ROOT / 'runtime-review/round9-partial-execution-preservation'
MIXED = ROOT / 'runtime-review/round9-m5-mixed-review'
START = time.monotonic()
SOURCE = 'fec0698c7fa4d76076b828cc17cf277ad8b307e0'
EXPECTED_MIXED = (11728748, 'a37d29dafce06eca6444cee9f258d8a379256bebf24f4a7e6b432c07f6d840eb')
EXPECTED_PARTIAL = (4774280, '96cd0f808e4e8babba7eefdbfdf2a329c114dc501ca339c71e96bf67a4e2993c')

def now():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()

def digest(path):
    size = 0
    sha = hashlib.sha256()
    with path.open('rb') as stream:
        while block := stream.read(1024 * 1024):
            size += len(block)
            sha.update(block)
    return {'bytes': size, 'sha256': sha.hexdigest()}

def stable(st):
    return (st.st_dev, st.st_ino, st.st_size, st.st_mtime_ns, st.st_ctime_ns, stat.S_IMODE(st.st_mode))

def safe(name):
    p = PurePosixPath(name)
    assert name and not p.is_absolute() and '..' not in p.parts and '\\' not in name, name
    assert str(p) == name.rstrip('/'), name

def write_json(path, obj):
    with path.open('xb') as stream:
        stream.write((json.dumps(obj, ensure_ascii=False, indent=2) + '\n').encode())

OUT.mkdir(parents=True, exist_ok=False)
plan = json.loads((PART / 'mixed-package-size-plan.json').read_bytes())
original_manifest = json.loads((MIXED / 'evidence-manifest.json').read_bytes())['files']
selected = set(original_manifest) | {'evidence-manifest.json', 'final-independent-handoff.json'}
planned = {entry['path']: entry for entry in plan['files']}
assert len(planned) == len(plan['files']) == len(selected) == 100
assert set(planned) == selected
assert sum(e['bytes'] for e in planned.values()) == 174704197
assert (plan['exact_compressed_stream_size_for_this_recipe'], plan['compressed_stream_sha256_for_this_recipe']) == EXPECTED_MIXED

class CompressedWriter:
    def __init__(self, stream):
        self.stream = stream
        self.compressor = lzma.LZMACompressor(format=lzma.FORMAT_XZ, preset=3)
    def write(self, block):
        self.stream.write(self.compressor.compress(block))
        return len(block)
    def finish(self):
        self.stream.write(self.compressor.flush())

class HashingReader:
    def __init__(self, stream):
        self.stream = stream
        self.count = 0
        self.sha = hashlib.sha256()
    def read(self, n=-1):
        block = self.stream.read(n)
        self.count += len(block)
        self.sha.update(block)
        return block

mixed_name = 'M5-four-mixed-originals.tar.xz'
mixed_index = []
with (OUT / mixed_name).open('xb') as output:
    writer = CompressedWriter(output)
    with tarfile.open(fileobj=writer, mode='w|', format=tarfile.PAX_FORMAT) as archive:
        for name in sorted(selected):
            safe(name)
            path = MIXED / name
            before = path.lstat()
            assert stat.S_ISREG(before.st_mode), name
            member = 'runtime-review/round9-m5-mixed-review/' + name
            info = tarfile.TarInfo(member)
            info.size = before.st_size
            info.mode = stat.S_IMODE(before.st_mode)
            info.mtime = 0
            with path.open('rb') as stream:
                reader = HashingReader(stream)
                archive.addfile(info, reader)
            assert stable(before) == stable(path.lstat()), name
            result = {'bytes': reader.count, 'sha256': reader.sha.hexdigest()}
            assert result == {k: planned[name][k] for k in result}, name
            if name in original_manifest:
                assert result == original_manifest[name], name
            mixed_index.append({'member': member, 'source_workspace_path': member,
                                'mode': info.mode, **result})
    writer.finish()
assert tuple(digest(OUT / mixed_name).values()) == EXPECTED_MIXED
print(json.dumps({'step': 'mixed_archive_created', **digest(OUT / mixed_name)}), flush=True)

copy_map = {
    'M5-interrupted-soak-and-backfill-originals.tar.xz': PART / 'M5-interrupted-soak-and-backfill-originals.tar.xz',
    'partial-preservation-manifest.json': PART / 'preservation-manifest.json',
    'partial-preservation-receipt.json': PART / 'preservation-receipt.json',
    'partial-original-handoff.json': PART / 'handoff.json',
    'partial-original-collector.py': PART / 'preserve_partial.py',
    'mixed-original-size-plan.json': PART / 'mixed-package-size-plan.json',
    'mixed-original-measurement-recipe.py': PART / 'measure_mixed_package.py',
    'mixed-original-evidence-manifest.json': MIXED / 'evidence-manifest.json',
    'mixed-independent-report.json': MIXED / 'mixed-independent-report.json',
    'mixed-final-independent-handoff.json': MIXED / 'final-independent-handoff.json',
    'interruption-observation.json': ROOT / 'runtime-review/round9-execution-loss/interruption-observation.json',
    'prepare_archive.py': Path(__file__),
}
copies = []
for name, source in copy_map.items():
    before = source.lstat()
    assert stat.S_ISREG(before.st_mode)
    data = source.read_bytes()
    assert stable(before) == stable(source.lstat()), name
    with (OUT / name).open('xb') as stream:
        stream.write(data)
    copies.append({'destination': name, 'source_workspace_path': str(source.relative_to(ROOT)),
                   'source_mode': stat.S_IMODE(before.st_mode), **digest(OUT / name)})
assert tuple(digest(OUT / 'M5-interrupted-soak-and-backfill-originals.tar.xz').values()) == EXPECTED_PARTIAL
partial_manifest_bytes = (OUT / 'partial-preservation-manifest.json').read_bytes()
partial = json.loads(partial_manifest_bytes)
assert partial['source_commit'] == SOURCE
assert partial['original_file_count'] == 2093 and partial['original_file_bytes'] == 278231211
partial_index = [{'member': e['path'], 'source_workspace_path': e['path'],
                  'mode': e['initial_stat']['mode'], 'bytes': e['bytes'], 'sha256': e['sha256']}
                 for e in partial['files']]
partial_index.append({'member': 'preservation-manifest.json',
                      'source_workspace_path': 'runtime-review/round9-partial-execution-preservation/preservation-manifest.json',
                      'mode': 0o644, **digest(OUT / 'partial-preservation-manifest.json')})
partial_dirs = {name: entry['mode'] for name, entry in partial['directories'].items()}

def verify_archive(path, index, expected_dirs):
    expected = {e['member']: e for e in index}
    assert len(expected) == len(index)
    seen, directories = set(), {}
    total = 0
    with tarfile.open(path, 'r|xz') as archive:
        for member in archive:
            safe(member.name)
            assert member.name not in seen, member.name
            seen.add(member.name)
            assert member.uid == 0 and member.gid == 0 and not member.uname and not member.gname
            if member.isdir():
                assert member.name in expected_dirs
                assert member.mode == expected_dirs[member.name]
                directories[member.name] = member.mode
                continue
            assert member.isreg(), (member.name, member.type)
            entry = expected[member.name]
            assert member.size == entry['bytes'] and member.mode == entry['mode'], member.name
            reader = HashingReader(archive.extractfile(member))
            while reader.read(1024 * 1024):
                pass
            assert reader.count == entry['bytes'] and reader.sha.hexdigest() == entry['sha256'], member.name
            total += reader.count
    assert seen == set(expected) | set(expected_dirs)
    assert directories == expected_dirs
    return {'regular_members': len(expected), 'directory_members': len(directories),
            'regular_member_bytes': total, 'all_member_size_mode_sha256_verified': True,
            'duplicate_or_unexpected_members': 0, 'links_or_special_members': 0,
            'archive': {'name': path.name, **digest(path)}}

mixed_verification = verify_archive(OUT / mixed_name, mixed_index, {})
partial_verification = verify_archive(OUT / 'M5-interrupted-soak-and-backfill-originals.tar.xz', partial_index, partial_dirs)
print(json.dumps({'step': 'both_archives_member_readback_complete', 'mixed': mixed_verification, 'partial': partial_verification}), flush=True)

mixed_report = json.loads((OUT / 'mixed-final-independent-handoff.json').read_bytes())
interruption = json.loads((OUT / 'interruption-observation.json').read_bytes())
assert mixed_report['source'] == SOURCE
assert mixed_report['status'] == 'failed_original_matrix_with_C1_scoped_success'
assert mixed_report['original_tasks_completed'] == 0
assert sorted(c['concurrency'] for c in mixed_report['blocked_cells']) == [4, 8, 16]
for cell in mixed_report['blocked_cells']:
    assert cell['acceptance'] is False and cell['phase_exits'] == {'mixed': 2, 'verify': 2}
    assert cell['original_report_exit_code'] == 0
assert partial['original_receipts']['soak']['status'] == 'running' if isinstance(partial['original_receipts'], dict) and 'soak' in partial['original_receipts'] else True

write_json(OUT / 'archive-source-index.json', {
    'schema_version': 1, 'kind': 'opaque_original_archive_members_and_source_paths',
    'source_commit': SOURCE,
    'source_path_base': '/workspace/scratch/28fef0db5e01',
    'source_paths_are_historical_capture_locations_not_a_current_liveness_claim': True,
    'archives': [
        {'name': mixed_name, **digest(OUT / mixed_name), 'source_original_files': 100,
         'source_original_bytes': 174704197, 'members': mixed_index, 'directories': {}},
        {'name': 'M5-interrupted-soak-and-backfill-originals.tar.xz', **digest(OUT / 'M5-interrupted-soak-and-backfill-originals.tar.xz'),
         'source_original_files': 2093, 'source_original_bytes': 278231211,
         'members': partial_index, 'directories': partial_dirs,
         'additional_member': 'preservation-manifest.json'},
    ],
})
write_json(OUT / 'copied-originals-index.json', {'schema_version': 1, 'files': copies,
    'mixed_archive_is_newly_materialized_exact_frozen_recipe_not_a_copied_original_archive': True})
write_json(OUT / 'archive-verification-receipt.json', {
    'schema_version': 1, 'kind': 'M5_original_runtime_preservation_only',
    'completed_at_utc': now(), 'elapsed_seconds': time.monotonic() - START,
    'source_commit': SOURCE, 'status': 'archive_bytes_and_complete_member_indexes_verified',
    'mixed_archive': mixed_verification, 'partial_archive': partial_verification,
    'mixed_recipe_matches_preexisting_frozen_size_and_sha256': True,
    'partial_archive_copied_without_recompression': True,
    'original_copy_files_verified': len(copies),
    'source_files_modified': False, 'shared_repository_index_HEAD_or_refs_modified': False,
    'artifact_code_or_binary_executed': False, 'sqlite_opened': False,
    'original_product_seal_repaired_or_replaced': False,
    'original_execution_statuses_changed': False,
    'product_tests_run': False, 'runtime_acceptance_granted': False,
    'publication_performed': False, 'original_tasks_completed': 0,
    'ledger': {'total': 192, 'completed': 163, 'remaining': 29},
})
readme = '''# M5 原始运行证据保全（第 10 轮）

本目录只保存固定产品 M5 `fec0698c7fa4d76076b828cc17cf277ad8b307e0` 的原始运行证据及其已有独立审查。它不是新的一次产品执行，也不提供 runtime 或原 TODO 验收通过结论。原始任务账本仍为 192 项，其中已完成 163 项、未完成 29 项；本包新增完成数为 0。

## 原运行状态

| 原运行 | 必须保留的实际状态 |
| --- | --- |
| mixed C1 | 原 CLI 0、verify 0；已有独审确认该格 900 次终态、600 次读取、300 次构建，以及对应 raw、RPC、统计和 parity 范围。该单格成功不使四格矩阵成功。 |
| mixed C4 / C8 / C16 | 三格原 CLI 2、verify 2，四格矩阵失败。原 report 在封存之前曾写 `passed_observation` 和 exit 0，这些原字节完整保留，但不能覆盖实际失败退出。C4/C16 没有成功生成 seal；C8 原 seal 存在但随后的 inventory 验证失败。未重封、重试、删项或修改原报告。 |
| M5 小时 soak | 原 receipt 仍为 `running`。23:08:23 UTC 的单独观测记录会话返回 Unknown process id，没有观察到终态退出码或最终 verify；这是未完成证据，不是 1 小时通过，也不能据此断言进程为何停止。 |
| M5 新 backfill | 原 receipt 仍为 `running`，构建未观察到完成及随后 backfill 执行验收；没有终态退出码或最终 verify。保留同一中断观测的未知边界。 |

所有原因分析仍按原报告保留：不能将 mixed 封存异常归因于产品或环境；四格同机运行且与 soak 重叠，不能据此做隔离性能或因果结论。后来产品修复不追溯改变本包中的 M5 运行结果。

## 两个独立压缩包

- `M5-four-mixed-originals.tar.xz`：按预先冻结的 `mixed-original-size-plan.json` 与原测量 recipe 首次落盘。100 个普通文件，共 174,704,197 字节，包含原 99 文件清单及该清单本身，其中 `failed-originals/` 为 84 个文件。保存实际 raw、主/对照 RPC、统计、parity、日志、原报告、seal（如原本存在）及 14 个 SQLite/WAL/SHM 字节快照。压缩结果必须精确为 11,728,748 字节、SHA-256 `a37d29dafce06eca6444cee9f258d8a379256bebf24f4a7e6b432c07f6d840eb`。这是冻结选集，不是四格项目目录的完整展开副本；原夹具和构建身份由已有收据绑定。
- `M5-interrupted-soak-and-backfill-originals.tar.xz`：原包逐字复制，没有重新压缩。包含 2,093 个原文件，共 278,231,211 字节，另有包内 `preservation-manifest.json` 和 282 个目录记录。压缩包 4,774,280 字节、SHA-256 `96cd0f808e4e8babba7eefdbfdf2a329c114dc501ca339c71e96bf67a4e2993c`。原 collector 只保证采集窗口内观察到字节稳定，不宣称全局进程消失或原子快照。原 receipt 的 `running` 状态保持不变。

两包共 16,503,028 字节，分别保存，没有相互嵌套，也没有复制旧的大型审查包。partial 原方案明确排除可重建的源码 checkout、私有构建 target、依赖 registry，以及三个已有构建收据和摘要绑定的 native binary；这是原件保全包，不是自包含构建包。完整排除清单保留在 `partial-preservation-manifest.json`。不存在的最终结果没有被补造。

## 索引与复核范围

`archive-source-index.json` 为两个包分别列出每一个普通成员的历史 source 路径、成员路径、权限 mode、size 与 SHA-256，并列出全部目录 mode。partial 包中的额外清单成员单独注明。`copied-originals-index.json` 绑定本目录所有原样复制的报告、清单、收据、原采集/测量脚本和 interruption observation。`package-index.json` 列出本目录除其自身之外的最终交付文件；索引不自我引用。

本次复核对两个 tar.xz 全部成员进行流式读取，核对精确路径集合、普通文件/目录类型、重复成员、mode、size 与 SHA-256。所有 SQLite/WAL/SHM 只当作不透明字节读取：没有打开数据库、checkpoint 或执行归档内二进制、脚本；没有修改原现场、原 seal、共享仓库 source/index/HEAD/refs 或任务状态。`archive-verification-receipt.json` 只证明归档传输与内容保全，不是产品测试报告。

`mixed-independent-report.json`、`mixed-final-independent-handoff.json` 和 `interruption-observation.json` 是原报告的逐字副本；它们同时在对应原包的完整清单范围内保留。少量顶层副本便于读者直接查看，未再次展开大型 raw、RPC 或 SQLite 数据。原 collector 与测量脚本作为审计原件保存，不能仅因出现在本目录而执行。`prepare_archive.py` 是本轮保全脚本原件，输出目录使用独占创建以避免覆盖已冻结交付。

本包待独立复核后由主智能体另行发布到 main 的纯 artifacts PR。当前没有发布、合并、关闭 PR 或修改 TODO 的授权结果；发布本包也不构成任何原验收条款的通过。
'''
with (OUT / 'README.md').open('x', encoding='utf-8') as stream:
    stream.write(readme)
package_files = []
for path in sorted(OUT.iterdir()):
    assert path.is_file() and not path.is_symlink()
    package_files.append({'path': path.name, 'git_mode': '100644', **digest(path)})
write_json(OUT / 'package-index.json', {'schema_version': 1, 'scope': 'all_sibling_delivery_files_except_this_index', 'files': package_files})
all_files = []
for path in sorted(OUT.iterdir()):
    data = path.read_bytes()
    blob = hashlib.sha1(b'blob ' + str(len(data)).encode() + b'\0' + data).hexdigest()
    all_files.append({'path': PREFIX + '/' + path.name, 'mode': '100644', 'git_blob': blob, **digest(path)})
write_json(OWN / 'delivery-receipt.json', {'schema_version': 1, 'frozen_at_utc': now(),
    'status': 'prepared_for_non_author_review_not_published',
    'delivery_root': str(OWN / 'delivery'), 'file_count': len(all_files),
    'total_bytes': sum(e['bytes'] for e in all_files), 'expected_additions': all_files,
    'no_git_tree_or_index_mutation': True, 'original_tasks_completed': 0, 'remaining_original_tasks': 29})
print(json.dumps({'step': 'delivery_frozen', 'files': len(all_files), 'bytes': sum(e['bytes'] for e in all_files), 'receipt': digest(OWN / 'delivery-receipt.json'), 'root': str(OUT)}), flush=True)
