"""Original G2 platform receiver delivery; complete stdlib disk-backed ZIP intake."""
import hashlib
import json
from pathlib import Path
import sys
import zipfile

from disk_zip import require, IntakeBudget, audit_zip, compare_expanded

SOURCE = "0272a1fb152fd76a7cfb22386a580629d4038a64"
CONTROL = "08bfcb4538c4fe390adac6c5a127c0a8795a1e7a"
RUN = 37881939531
ARTIFACT = 11594372339
SIZE = 137100343
SHA = "bb5a3ae2a3808a908c00b35b8bbbdffa7b191279703b569f4338f1af1d413f14"

def sha(raw):
    return hashlib.sha256(raw).hexdigest()

def inspect(archive_path, source_root, scratch):
    registration_path = Path(__file__).with_name("G2-platform-registration.json")
    registration_bytes = registration_path.read_bytes()
    require(hashlib.sha1(b"blob " + str(len(registration_bytes)).encode() + b"\0" + registration_bytes).hexdigest()
            == "85a6238c80683f3ce96a0f5bf743ccedbc99d48a", "fixed original G2 registration")
    reg = json.loads(registration_bytes)
    require(reg["source_commit"] == SOURCE and len(reg["artifacts"]) == 9,
            "original G2 population and identity")
    selected_names = {"receipt.json", "file-inventory.json", "registration.json",
                      "source-before.json", "source-after.json", "source-inputs.json",
                      "observer-before.json", "observer-after.json", "replay/matrix.json",
                      "collector-comparison.json", "commands/original-cold-collector/command.json",
                      "commands/original-cold-collector/result.json",
                      "commands/original-cold-collector/stdout.log",
                      "commands/original-cold-collector/stderr.log"}
    for spec in reg["artifacts"]:
        selected_names.add("members/%d.json" % spec["id"])
        prefix = ("cells/" if spec["kind"] == "cell" else "original-collector/") + str(spec["id"]) + "/"
        selected_names.add(prefix + ("bundle.json" if spec["kind"] == "cell" else "matrix.json"))
    require(archive_path.stat().st_size == SIZE, "fixed platform outer bytes")
    with archive_path.open("rb") as raw:
        require(hashlib.file_digest(raw, "sha256").hexdigest() == SHA, "complete original platform ZIP digest")
        raw.seek(0)
        budget = IntakeBudget()
        with zipfile.ZipFile(raw) as outer:
            members, selected = audit_zip(outer, budget, selected_names)
            value = lambda name: json.loads(selected[name])
            receipt, inventory = value("receipt.json"), value("file-inventory.json")
            require(set(members) == set(inventory) | {"receipt.json", "file-inventory.json"},
                    "platform full outer inventory population")
            require(all(inventory[name] == {k: members[name][k] for k in ("bytes", "sha256")}
                        for name in inventory), "platform full outer inventory bytes")
            require(sha(selected["file-inventory.json"]) == receipt["file_inventory_sha256"],
                    "platform outer inventory bound to receipt")
            require(receipt["status"] == "accepted_scoped_original_G2_platform_replay"
                    and receipt["exit_code"] == 0 and receipt["controller_commit"] == CONTROL
                    and receipt["source_commit"] == SOURCE and receipt["original_run"] == reg["run_id"]
                    and receipt["counts"] == dict(passed=8, failed=0, not_run=0)
                    and receipt["product_or_Cargo_execution"] is False
                    and receipt["original_artifact_ids"] == [s["id"] for s in reg["artifacts"]],
                    "exact original platform replay identity/result")
            require(value("registration.json") == reg, "retained original platform registration")
            require(selected["source-before.json"] == selected["source-after.json"]
                    and selected["observer-before.json"] == selected["observer-after.json"],
                    "platform receiver source/observer unchanged")
            source, observer = value("source-before.json"), value("observer-before.json")
            require(source["source_commit"] == SOURCE and source["source_tree"] == reg["source_tree"]
                    and source["input_count"] == reg["source_inputs"] == 1089
                    and source["manifest_sha256"] == reg["source_manifest_sha256"],
                    "original G2 1089 source")
            require(observer["source_commit"] == SOURCE and len(observer["inputs"]) == reg["observer_inputs"] == 4
                    and observer["manifest_sha256"] == reg["observer_manifest_sha256"],
                    "original G2 four cold observers")
            local_root = Path(source_root)
            sys.path.insert(0, str(local_root / "scripts"))
            import p8_cold_build as cold
            local_source, local_inputs = cold.source_identity(local_root, SOURCE)
            local_observer = cold.observer_snapshot(local_root)
            require(value("source-inputs.json") == local_inputs
                    and dict(source, source_root=local_source["source_root"]) == local_source
                    and dict(observer, source_root=local_observer["source_root"]) == local_observer,
                    "complete original source/observer exact local Git proof")
            originals = []
            for spec in sorted(reg["artifacts"], key=lambda x: outer.getinfo("zips/%d.zip" % x["id"]).header_offset):
                zipname = "zips/%d.zip" % spec["id"]
                require(members[zipname]["bytes"] == spec["bytes"] and members[zipname]["sha256"] == spec["sha256"],
                        "registered original nested platform ZIP")
                destination = scratch / ("original-%d.zip" % spec["id"])
                digest = hashlib.sha256()
                copied = 0
                with outer.open(zipname) as source_file, destination.open("xb") as output:
                    while block := source_file.read(64 * 1024):
                        copied += len(block)
                        require(copied <= spec["bytes"], "platform nested ZIP exceeds registration")
                        output.write(block)
                        digest.update(block)
                require(copied == spec["bytes"] and digest.hexdigest() == spec["sha256"],
                        "platform nested disk-copy identity")
                with zipfile.ZipFile(destination) as nested:
                    inner_members, _ = audit_zip(nested, budget)
                prefix = ("cells/" if spec["kind"] == "cell" else "original-collector/") + str(spec["id"]) + "/"
                compare_expanded(inner_members, members, prefix)
                declared = value("members/%d.json" % spec["id"])
                require(declared == {n: dict(r, extracted=True) for n, r in inner_members.items()},
                        "platform nested full declared inventory")
                if spec["kind"] == "cell":
                    require(value(prefix + "bundle.json")["cell"] == spec["cell"],
                            "exact registered platform cell")
                    require(inner_members["source-inputs.json"]["sha256"] == source["manifest_sha256"],
                            "each original cell full product list")
                    for path, identity in observer["inputs"].items():
                        row = inner_members["observer-source/" + path]
                        require(row["sha256"] == identity["sha256"] and row["bytes"] == identity["bytes"],
                                "each original cell cold observer bytes")
                else:
                    require(set(inner_members) == {"matrix.json"}, "original collector sole member")
                originals.append(dict(artifact_id=spec["id"], bytes=spec["bytes"], sha256=spec["sha256"],
                                      prefix=prefix, members=inner_members))
            root = source["source_root"]
            command = value("commands/original-cold-collector/command.json")
            result = value("commands/original-cold-collector/result.json")
            argv = command["argv"]
            output = "/home/runner/work/_temp/p8-G2-offline-platform"
            require(Path(argv[0]).name in ("python3", "python3.12", "python")
                    and argv[1:] == ["-B", root + "/scripts/p8_cold_build.py", "--source-root", root,
                                    "--collect-cells", output + "/cells", "--expected-commit", SOURCE,
                                    "--output-dir", output + "/replay"],
                    "actual unmodified G2 collector argv")
            require(command["cwd"] == root and command["source_commit"] == SOURCE
                    and command["scope"] == "offline replay only" and result["exit_code"] == 0,
                    "actual original collector result")
            collectors = [s for s in reg["artifacts"] if s["kind"] == "collector"]
            require(len(collectors) == 1, "exact original collector count")
            original_path = "original-collector/%d/matrix.json" % collectors[0]["id"]
            original_matrix, replay = value(original_path), value("replay/matrix.json")
            mapped = json.loads(selected[original_path])
            mapped["source"]["source_root"] = root
            require(replay == mapped and replay["counts"] == dict(passed=8, failed=0, not_run=0),
                    "all original matrix fields except source_root")
            comparison = value("collector-comparison.json")
            require(comparison["exact_except_source_root"] is True
                    and comparison["original_sha256"] == sha(selected[original_path])
                    and comparison["replay_sha256"] == sha(selected["replay/matrix.json"]),
                    "actual exact original collector comparison")
            after, after_inputs = cold.source_identity(local_root, SOURCE)
            require(after == local_source and after_inputs == local_inputs
                    and cold.observer_snapshot(local_root) == local_observer, "local G2 source unchanged")
            return dict(schema="p8-G2-external-disk-intake-v1", status="accepted_scoped_original_receiver_delivery",
                        source_commit=SOURCE, receiver_controller=CONTROL, receiver_run=RUN, artifact_id=ARTIFACT,
                        zip_bytes=SIZE, zip_sha256=SHA, actual_outer_members=members, actual_original_ZIPs=originals,
                        source=source, observer=observer, receipt=receipt,
                        actual_original_collector_command=command, actual_original_collector_result=result,
                        original_matrix=original_matrix, replay_matrix=replay,
                        selected_original_outputs={k: v.decode() for k, v in selected.items()},
                        budgets=dict(metadata_bytes=budget.metadata_bytes, selected_bytes=budget.selected_bytes,
                                     member_count=budget.member_count),
                        transport="independent disk-backed standard zipfile; not a Range validation",
                        product_Cargo_or_native_statistics_execution=False, shared_local_files_written=False,
                        TODO_closed=0, TODO_remaining=29)
