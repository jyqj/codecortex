#!/usr/bin/env python3
"""Accept only eight delivered originals; call unchanged validate_shard and inspect_raw."""
import collections,hashlib,json,pathlib,platform,sys,traceback
if sys.flags.optimize: raise SystemExit("optimized Python forbidden")
sys.dont_write_bytecode=True
P=pathlib.Path
SOURCE="275e8799d4947d297329073eaa3ca675d3fd0777"
RUN=37896198208
ATTEMPT=1
SDK_PATHS=["/usr/local/lib/android","/usr/share/dotnet"]
source=P(sys.argv[1]).resolve(strict=True)
build=P(sys.argv[2]).resolve(strict=True)
slices=P(sys.argv[3]).resolve(strict=True)
out=P("/output/inspection.json")
assert not out.exists()
def require(condition,message):
    if not condition: raise AssertionError(message)
def digest(path):
    h=hashlib.sha256()
    with P(path).open("rb") as stream:
        for data in iter(lambda:stream.read(1024*1024),b""): h.update(data)
    return h.hexdigest()
def capacity_review(directory, scale, observer, matrix, github_job):
    """Check the separately retained observer receipts; no SDK commands run."""
    receipt = matrix.read_json(directory / "receipt.json")
    require(receipt.get("profile") == matrix.CAPACITY_PROFILE and receipt.get("scale") == scale and
            receipt.get("status") == "capacity_available" and receipt.get("exit_code") == 0 and
            receipt.get("artifact_state") == "sealed_after_commands_finished" and
            receipt.get("source_commit") == SOURCE and receipt.get("observer_sha256") == observer,
            "capacity receipt did not complete at 275e")
    require(receipt.get("files") == matrix.inventory(directory, ("receipt.json",)), "capacity files changed after sealing")
    context = receipt.get("context", {})
    for key, value in {"GITHUB_REPOSITORY": "jyqj/codecortex", "GITHUB_RUN_ID": str(RUN),
                       "GITHUB_RUN_ATTEMPT": str(ATTEMPT), "GITHUB_JOB": github_job, "RUNNER_OS": "Linux",
                       "RUNNER_ENVIRONMENT": "github-hosted", "ImageOS": "ubuntu24", "P8_EXPECTED_SOURCE": SOURCE}.items():
        require(context.get(key) == value, "capacity context mismatch: " + key)
    require(isinstance(context.get("RUNNER_NAME"), str) and context["RUNNER_NAME"] and
            isinstance(context.get("ImageVersion"), str) and context["ImageVersion"], "capacity host/image missing")
    need = scale * 256 * 1024 + 4 * 1024**3
    require(receipt.get("required_free_bytes") == need and receipt.get("allow_preparation") is True,
            "capacity requirement/invocation changed")
    for state in (receipt.get("initial"), receipt.get("final")):
        require(isinstance(state, dict) and all(type(state.get(key)) is int and state[key] >= 0
                for key in ("total_bytes", "used_bytes", "free_bytes")), "invalid capacity disk observation")
    require(receipt["final"]["free_bytes"] >= need, "registered capacity unavailable")
    steps = receipt.get("steps")
    require(isinstance(steps, list) and len(steps) <= 2, "invalid capacity step list")
    for index, step in enumerate(steps):
        require(step.get("target") == SDK_PATHS[index] and type(step.get("existed")) is bool,
                "unregistered capacity target/order")
        if not step["existed"]:
            require("directory_size" not in step and "removal" not in step, "nonexistent SDK has command")
            continue
        for field, operation, suffix in (("directory_size", ["du", "-sx", "--block-size=1", "--", SDK_PATHS[index]], "size"),
                                         ("removal", ["rm", "-rf", "--one-file-system", "--preserve-root=all", "--", SDK_PATHS[index]], "remove")):
            command = step[field]
            label = f"sdk-{index}-{suffix}"
            require(command == matrix.read_json(directory / (label + ".command.json")), "capacity command differs from original receipt")
            require(command.get("operation") == operation and command.get("argv") ==
                    ["sudo", "-n", "timeout", "--signal=TERM", "--kill-after=5s", "300s", *operation] and
                    command.get("exit_code") == 0 and command.get("status") == "completed" and
                    command.get("cleanup_complete") is True and command.get("stdout") == label + ".stdout" and
                    command.get("stderr") == label + ".stderr", "capacity command failed/unregistered/unsealed")
            require((directory / command["stdout"]).is_file() and (directory / command["stderr"]).is_file(), "missing capacity command output")
    return {"scale": scale, "receipt_sha256": digest(directory / "receipt.json"), "context": context,
            "required_free_bytes": need, "final_free_bytes": receipt["final"]["free_bytes"], "status": "passed"}


result={"schema":"p8-275e-first-original-slices-acceptance-v1","source":SOURCE,"run":RUN,"attempt":ATTEMPT,
        "original_population":{"shards":150,"capacity":150,"samples":1500},
        "scope":"Exactly three delivered rep0 measurement shards plus all five rep0 capacity originals; partial reception only. No aggregate/combine, full N150, TODO or release certification.",
        "transport_blob":"61059f7fe049d05dede9020fc6d98b08a3f10052",
        "prior_original_build_acceptance_blob":"c2e5d10caaa40564bbf178bffefbf6fb64936d26",
        "new_measurement_or_Cargo":False,"original_validator_modified":False,
        "original_shards":[],"capacity":[],"TODO_closed":0,"TODO_remaining":29}
try:
    require(platform.system()=="Linux" and platform.machine()=="x86_64","needs original Linux ELF environment")
    expected={"scripts/p8_scale_matrix.py":"f169b0f26cf2a87d3e2548d734431c530cbf63d353ac054045ec8be80889f957",
              "scripts/p7_build_identity.py":"5003305105464936edf8f444fc0b93cd14a5d5f074dc650f16de2259e86f1cc5",
              "scripts/p8_runner_capacity.py":"2e37954df30b153ad82b49a5841a977451288c1730d03bfd2d0d7a2ace412939"}
    for name,wanted in expected.items(): require(digest(source/name)==wanted,"changed original observer")
    sys.path.insert(0,str(source/"scripts"))
    import p8_scale_matrix as matrix
    before=matrix.source_snapshot(source)
    require(before["source_commit"]==SOURCE and before["source_tree"]=="5859c18f6ead4f8eff5abc3be05d80dfc2fc5b46"
            and before["input_count"]==1089 and before["manifest_sha256"]=="593cf443fc914998dd2cb5a30c3caf180eb8d0d7a398a86e4624e3df2ca65e83","changed source")
    build_sha="dbfbe43fca5197853dcda87df3f3ed9b367fdf495ff9652dd0fdda13b177acd2"
    require(digest(build/"build.json")==build_sha,"original accepted build receipt changed")
    record=matrix.read_json(build/"build.json")
    binary=build/"p8-scale"
    require(record["status"]=="passed" and record["exit_code"]==0 and record["source_commit"]==SOURCE,"build original status/source")
    require(record["files"]==matrix.inventory(build,("build.json",)),"accepted build member bytes changed")
    require(digest(binary)==record["binary_sha256"]=="1cee91b943fe0b787e2767112872e93a6991281790c626bb46a23b07d21ba394","accepted binary bytes changed")
    require(matrix.native_digest(binary,binary)==record["binary_blake3"]=="667c53bcae6ea302f98b310ee7d98b0b62abf029bbbe980b9dd7eec960a06d81","accepted binary BLAKE3 changed")
    require(digest(slices/"transport.json")=="36b129882e7225687126bd5b9d0d44d2f93a39813f3e54f553d7e46aae262d4e","full transport receipt changed")
    transport=matrix.read_json(slices/"transport.json")
    require(transport["source"]==SOURCE and transport["run"]==RUN and transport["attempt"]==ATTEMPT,"transport source/run changed")
    wanted={("shard",scale,0) for scale in (1000,5000,10000)}|{("capacity",scale,0) for scale in (1000,5000,10000,50000,100000)}
    require(len(transport["artifacts"])==8 and {(r["kind"],r["scale"],r["index"]) for r in transport["artifacts"]}==wanted,"partial reception population changed")
    def originals_unchanged():
        for row in transport["artifacts"]:
            directory=P(row["directory"])
            require(directory==slices/(row["kind"]+"-"+str(row["scale"])+"-"+str(row["index"])),"unregistered receiver path")
            archive=slices/"zips"/(str(row["artifact"]["id"])+".zip")
            require(archive.stat().st_size==row["artifact"]["size_in_bytes"] and "sha256:"+digest(archive)==row["artifact"]["digest"],"original ZIP changed")
            expected_members={k:{"bytes":v["bytes"],"sha256":v["sha256"]} for k,v in row["members"].items()}
            require(matrix.inventory(directory)==expected_members,"original expanded member closure changed")
    originals_unchanged()
    for row in sorted(transport["artifacts"],key=lambda r:(r["kind"],r["scale"],r["index"])):
        directory=P(row["directory"]);scale=row["scale"]
        if row["kind"]=="capacity":
            result["capacity"].append(capacity_review(directory,scale,expected["scripts/p8_runner_capacity.py"],matrix,"preflight"))
        else:
            expected_plan=matrix.registered_plan(scale,0,30,30,12648430,18000000,matrix.CAPACITY_PROFILE)
            require(matrix.read_json(directory/"registered-plan.json")==expected_plan,"original fixed N30/seed/budgets changed")
            inspected=matrix.validate_shard(directory,record,binary,build_sha)
            require(inspected["plan"]==expected_plan,"validated plan changed")
            result["original_shards"].append(inspected)
    originals_unchanged()
    require(matrix.source_snapshot(source)==before and matrix.driver_snapshot(source)==record["driver_source"],"source/driver changed")
    require(record["files"]==matrix.inventory(build,("build.json",)) and digest(build/"build.json")==build_sha,"build changed")
    for name,wanted_hash in expected.items(): require(digest(source/name)==wanted_hash,"observer changed")
    require(len(result["capacity"])==5 and len(result["original_shards"])==3,"incomplete delivered-original validation")
    samples=[sample for shard in result["original_shards"] for sample in shard["measurements"]]
    require(len(samples)==32,"unexpected first-slice sample population")
    result.update(status="accepted_scoped_partial_originals",validated_shards=3,validated_capacity=5,
                  validated_samples=32,remaining_measurement_shards=147,
                  original_validate_shard_calls=3,original_inspect_raw_called_by_original_validate_shard=True,
                  source_before_after_equal=True,all_original_bytes_unchanged=True,
                  original_ELF_execution_scope="--hash-file only; no product/measurement/statistics workload",
                  original_aggregate_executed=False,complete_N150_accepted=False)
except BaseException as error:
    result.update(status="failed",exception_type=type(error).__name__,error=str(error))
    traceback.print_exc()
    raise
finally:
    with out.open("x") as stream: json.dump(result,stream,sort_keys=True,indent=2);stream.write("\n")
    print(json.dumps({"status":result["status"],"shards_validated":len(result["original_shards"]),"capacity_validated":len(result["capacity"]),"TODO_closed":0,"TODO_remaining":29}))
