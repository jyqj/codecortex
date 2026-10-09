#!/usr/bin/env python3
"""Prepare a public, byte-preserving catalog after completed blob uploads."""
import hashlib
import json
import pathlib
import shutil

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parent
OUT = HERE / "final-public"
PREFIX = "artifacts/benchmarks/original-custody-C3ff-20261009"


def canonical(value):
    return (json.dumps(value, ensure_ascii=False, sort_keys=True, indent=2) + "\n").encode()


def write_new(relative, content):
    target = OUT / relative
    target.parent.mkdir(parents=True, exist_ok=True)
    with target.open("xb") as handle:
        handle.write(content)


def copy_new(source, relative):
    write_new(relative, source.read_bytes())


def main():
    plan = json.loads((HERE / "private-transport-plan.json").read_bytes())
    receipts = json.loads((HERE / "upload-receipts.json").read_bytes())
    assert receipts["uploaded_chunks"] == len(plan["chunks"]) == 236
    assert receipts["uploaded_bytes"] == 467676992
    for index, (expected, actual) in enumerate(zip(plan["chunks"], receipts["chunks"])):
        assert actual["index"] == index
        assert actual["path"] == expected["repository_path"]
        for key in ("bytes", "sha256", "git_blob"):
            assert actual[key] == expected[key], (index, key)
    seals = []
    for original in plan["source_stat_seals"]:
        current = pathlib.Path(original["local_path"]).stat()
        after = {"dev": current.st_dev, "inode": current.st_ino,
                 "bytes": current.st_size, "mtime_ns": current.st_mtime_ns}
        assert all(after[key] == original[key] for key in after), original["artifact_id"]
        seals.append({"artifact_id": original["artifact_id"], "before_and_after_equal": True,
                      "bytes": original["bytes"], "original_zip_sha256": original["sha256"]})
    for artifact in plan["catalog"]["artifacts"]:
        source = HERE / artifact["manifest"]
        data = source.read_bytes()
        assert len(data) == artifact["manifest_bytes"]
        assert hashlib.sha256(data).hexdigest() == artifact["manifest_sha256"]
        write_new(artifact["manifest"], data)
    for name in ("restore_original_zip.py", "catalog-planned.json", "catalog-checkpoint-01.json",
                 "checkpoint-01-publication-receipt.json", "upload-receipts.json"):
        copy_new(HERE / name, name)
    for name in ("prepare_custody.py", "prepare_final_publication.py", "review_restore_once.py"):
        copy_new(HERE / name, "source-receipts/" + name)
    copy_new(ROOT / "original-C3ff-platform-gates/final-delivery-navigation-draft/benchmark-navigation-and-raw-custody-draft.json",
             "source-receipts/input-artifact-catalog.json")
    write_new("source-receipts/transport-source-stat-check.json", canonical({
        "scope": "All 25 original file identities remained unchanged across transport; whole ZIP SHA256 was verified in prepare, every uploaded chunk GitOID matched its frozen plan.",
        "artifacts": seals, "native_execution": False,
    }))
    for source in sorted((HERE / "independent-review").iterdir()):
        assert source.is_file()
        copy_new(source, "transport-review/" + source.name)
    collector = ROOT / "original-C3ff-platform-gates/official-collector-11617153706"
    metadata = json.loads((collector / "artifact-metadata.json").read_bytes())
    original_zip = (collector / "11617153706.zip").read_bytes()
    assert len(original_zip) == metadata["size_in_bytes"] == 1494
    assert "sha256:" + hashlib.sha256(original_zip).hexdigest() == metadata["digest"]
    assert metadata["workflow_run"]["head_sha"] == plan["catalog"]["source_commit"]
    for name in ("11617153706.zip", "artifact-metadata.json", "job-metadata.json",
                 "independent-equivalence-review.json", "normalized-function-payload.json",
                 "original-job.log", "original-matrix.json", "official-jobs-complete.json",
                 "official-artifacts-complete.json"):
        copy_new(collector / name, "supplemental/official-platform-collector/" + name)
    catalog = dict(plan["catalog"])
    catalog["publication_state"] = "complete original byte set; confirm all referenced blobs in the published Git tree"
    catalog["all_uploaded_chunk_git_oids_match_frozen_plan"] = True
    catalog["all_25_source_file_identities_unchanged_across_transport"] = True
    catalog["supplemental_artifacts"] = [{
        "artifact_id": 11617153706, "run_id": metadata["workflow_run"]["id"],
        "scope": "official platform collector metadata; additional to the 25 principal original ZIPs",
        "path": "supplemental/official-platform-collector/11617153706.zip",
        "bytes": len(original_zip), "sha256": hashlib.sha256(original_zip).hexdigest(),
        "git_blob": hashlib.sha1(f"blob {len(original_zip)}\0".encode() + original_zip).hexdigest(),
        "source_commit": metadata["workflow_run"]["head_sha"],
        "created_at": metadata["created_at"], "expires_at": metadata["expires_at"],
        "official_artifact_locator": metadata["url"],
        "equivalence_review": "supplemental/official-platform-collector/independent-equivalence-review.json",
    }]
    catalog["supplemental_artifact_count"] = 1
    catalog["all_original_zip_count"] = 26
    catalog["all_original_zip_bytes"] = 467678486
    catalog["transport_review"] = "transport-review/scoped-independent-review.json"
    write_new("catalog.json", canonical(catalog))
    readme = """# C3ff 原始 Actions 工件保全

本目录完整保存固定源码 `3ffcefc3b28ee1a4ed80caecebd7208a45c3e302` 已验收运行的 **25 份主要原始 Actions ZIP，共 467,676,992 字节**，以及后来取得的官方平台汇总 ZIP 1,494 字节。合计 26 份、467,678,486 字节。`catalog.json` 是完整索引。

主要 ZIP 使用 236 个连续原字节块保存。没有重新打包、重新压缩、删除成员或重跑测试；逐请求记录、原数据库、日志、夹具和实际执行二进制都保留。每块记录长度、原偏移、SHA-256 与 Git blob OID。25 份原 ZIP 的完整摘要均与官方值核准；每个远端上传对象均匹配计算出的 Git OID。

## 范围

| 原运行 | 主要原件 | 对应已验收范围 |
| --- | ---: | --- |
| 37908825814 | 6 | C1/C4/C8/C16 mixed、controlled backfill、1 小时 soak |
| 37908825715 | 1 | 生命周期与缓存分层 |
| 37908825722 | 17 | 8 格平台的 16 原 ZIP，以及 recovery/version-pair |
| 37908826034 | 1 | 故障 gate 与失败退出 |

后到的 `11617153706` 是运行 37908825722 的官方平台汇总，只增加 metadata 原件，不增加平台样本或 native 执行。独立对照验证了共同 collector payload；原 CLI 额外四个字段单独保留，没有把两份完整 JSON 说成逐字相同。

## 恢复一个原 ZIP

在检出包含本目录的固定提交后，从仓库根目录运行：

```sh
python artifacts/benchmarks/original-custody-C3ff-20261009/restore_original_zip.py artifacts/benchmarks/original-custody-C3ff-20261009/manifests/11609403178.json --output /tmp/11609403178.zip
```

更换 manifest 即可恢复另一主要原件。命令逐块校验 SHA-256、Git OID 和长度，再校验整 ZIP；已有输出不会被覆盖。它不访问网络、不提取 ZIP、不运行内部程序。官方汇总小 ZIP 已直接保存在 `supplemental/official-platform-collector/`。

恢复工具已独立实际验证：以原 backfill ZIP 的四块恢复 7,835,686 字节，与原 ZIP 逐字一致；已有输出拒绝且不变；一位损坏的测试块被拒绝且没有残留输出。完整命令、输出和仅自有测试文件的清理记录见 `transport-review/`。

## 固定来源和边界

`manifests/<artifact-id>.json` 固定官方 run、artifact ID、原源码、ZIP 大小与摘要及验收引用。`source-receipts/` 保留制作输入和脚本；`upload-receipts.json` 记录实际对象绑定。`catalog-checkpoint-01.json` 与其发布收据记录首件保全的历史状态，`catalog-planned.json` 是原始计划，均保留不改。

custody manifest 里的 review publication_status 是原制作输入清单时的快照；相应 `artifacts/benchmarks/<run-id>/` 导航给出后续已发布报告的固定提交。报告解释各自的源码、样本和平台范围；原件保存不扩大原验收，不把不同源码的规模样本合并。

大型原件只放在独立分支 `evidence/p8-originals-c3ff-a217-20261009`，代码 PR 的 benchmark 导航引用固定提交。本次保全不产生新的 native 执行、统计样本或 TODO 完成数。当前原任务仍 192 项、163 done、29 未完成；完整规模研究与其他尚缺原件的验收继续进行。
"""
    write_new("README.md", readme.encode())
    public_files = []
    for source in sorted(OUT.rglob("*")):
        if not source.is_file():
            continue
        data = source.read_bytes()
        relative = source.relative_to(OUT).as_posix()
        assert not relative.startswith("private-") and "/private-" not in relative
        public_files.append({
            "source": str(source), "target": PREFIX + "/" + relative, "mode": "100644",
            "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest(),
            "git_blob": hashlib.sha1(f"blob {len(data)}\0".encode() + data).hexdigest(),
            "encoding": "base64" if relative.endswith(".zip") else "utf-8",
        })
    result = {"schema": "original-custody-final-public-file-manifest-v1", "files": public_files,
              "small_file_count": len(public_files), "small_file_bytes": sum(f["bytes"] for f in public_files),
              "chunk_count": 236, "principal_original_zip_bytes": 467676992,
              "supplemental_original_zip_bytes": 1494, "ref_updated": False}
    with (HERE / "final-public-file-manifest.json").open("xb") as output:
        output.write(canonical(result))
    print(json.dumps({key: result[key] for key in ("small_file_count", "small_file_bytes", "chunk_count")}, sort_keys=True))


if __name__ == "__main__":
    main()
