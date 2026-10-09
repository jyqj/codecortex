import sys
sys.dont_write_bytecode=True
import pathlib,json,shutil,sqlite3
from contextlib import closing
base=pathlib.Path.cwd()
source=(base/"source").resolve()
sys.path.insert(0,str(source/"scripts"))
from p7_build_identity import source_snapshot
from p8_cold_build import digest
from p8_rollback import source_manifest,file_manifest,db_snapshot,validate_rebuild
from p8_recovery import full_evidence_manifest
home=base/"raw"/"11592146562"/"extracted"/"p8-full-recovery"
j=json.loads((home/"full-recovery.json").read_text())
out=base/"review-platform"/"recovery-database-replay"
out.mkdir()
copied={}
def snapshot_copy(path):
    rel=path.relative_to(home)
    dest=out/rel
    dest.parent.mkdir(parents=True,exist_ok=True)
    inputs={}
    for suffix in ("","-wal","-shm"):
        src=path.with_name(path.name+suffix)
        if src.exists():
            dst=dest.with_name(dest.name+suffix)
            shutil.copyfile(src,dst)
            assert digest(src)==digest(dst)
            inputs[src.relative_to(home).as_posix()]={"sha256":digest(src),"bytes":src.stat().st_size}
    state=db_snapshot(dest)
    copied[rel.as_posix()]={"copied_inputs":inputs,"observed_snapshot":state,"operation":"SQLite mode=ro PRAGMA integrity_check/foreign_key_check and SELECT only on owned byte copies"}
    return state,dest
local=[]
for c in j["local_recovery"]["cases"]:
    if c["status"]!="passed": continue
    expected=c["result"]["after_reopen"]["database"] if c["id"]=="deleted_source" else c["result"]["database"]
    state,path=snapshot_copy(home/"local-recovery"/c["id"]/"fixture"/".codecortex"/"index.sqlite3")
    assert state==expected and state["schema_version"]==25 and state["integrity"]=="ok" and state["foreign_key_errors"]==0
    local.append({"case":c["id"],"database":state})
active=[]
for a in j["active_stdio"]:
    seed=a["seed"]
    state,path=snapshot_copy(home/f"active-stdio-{seed}"/"project"/".codecortex"/"index.sqlite3")
    assert state["schema_version"]==25 and state["integrity"]=="ok" and state["foreign_key_errors"]==0
    with closing(sqlite3.connect(path.as_uri()+"?mode=ro",uri=True)) as db:
        count=db.execute("SELECT COUNT(*) FROM semantic_manifest").fetchone()[0]
        invalid=db.execute("SELECT COUNT(*) FROM semantic_manifest s LEFT JOIN document_manifest d ON d.doc_key=s.doc_key AND d.doc_version=s.doc_version WHERE d.doc_key IS NULL").fetchone()[0]
    assert count==1 and invalid==0
    active.append({"seed":seed,"database":state,"semantic_manifest_count":count,"stale_document_versions":invalid})
rollback=j["rollback"]
rhome=home/"rollback"
assert rollback==json.loads((rhome/"rollback.json").read_text())
assert rollback["status"]=="passed_limited_local_drill"
expected_cases={"future_schema_controlled_rebuild":"passed","semantic_to_default_disabled_local":"passed","old_database_backup_restore":"passed","operator_cache_version_roots":"passed","active_cache_reader_rejects_unknown_format":"not_run","source_and_config_integrity":"passed"}
assert {c["id"]:c["status"] for c in rollback["cases"]}==expected_cases
rstates={}
for key,name in [("old_database","index-old.sqlite3"),("future_database","index-future.sqlite3"),("rebuilt_database","index-rebuilt.sqlite3")]:
    path=rhome/"backups"/name
    assert digest(path)==rollback[key]["sha256"]
    state,_=snapshot_copy(path)
    assert state==rollback[key]["snapshot"]
    rstates[key]=state
final_r,_=snapshot_copy(rhome/"fixture"/".codecortex"/"index.sqlite3")
assert final_r==rstates["old_database"]
future=next(c for c in rollback["cases"] if c["id"]=="future_schema_controlled_rebuild")
assert future["before"]==rstates["future_database"] and future["after"]==rstates["rebuilt_database"]
validate_rebuild(future["before"],future["after"])
assert rstates["old_database"]["schema_version"]==rstates["rebuilt_database"]["schema_version"]==25
assert rstates["future_database"]["schema_version"]==1025
integrity=next(c for c in rollback["cases"] if c["id"]=="source_and_config_integrity")
assert source_manifest(rhome/"fixture")==integrity["source_manifest"]
assert json.loads((rhome/"source-before.json").read_text())==json.loads((rhome/"source-after.json").read_text())==integrity["source_manifest"]
assert digest(rhome/"fixture"/".codecortex.json")==digest(rhome/"backups"/"config-before.json")==integrity["config_sha256"]
assert json.loads((rhome/"fixture"/".codecortex.json").read_text())["semantic"]["enabled"] is False
assert json.loads((rhome/"backups"/"config-candidate.json").read_text())["semantic"]["enabled"] is True
cache=next(c for c in rollback["cases"] if c["id"]=="operator_cache_version_roots")
assert file_manifest(rhome/"semantic-cache"/"future-format-999")==cache["future_files_unchanged"]
assert cache["rollback_root_created"] is False and not (rhome/"semantic-cache"/"rollback-format-1").exists()
assert rollback["network_policy"]=={"kind":"disabled_config_only","enforcement":"not_measured"}
pair=j["actual_source_version_pair"]
phome=home/"actual-version-pair"
assert pair==json.loads((phome/"version-pair.json").read_text())
assert pair["status"]=="passed_actual_source_version_pair" and pair["production_schema_versions"]=={"current":25,"previous":24}
assert pair["schema_fault_injection"] is False and pair["released_version_pair"] is False and pair["release_certified"] is False
assert [c["id"] for c in pair["cases"]]==["actual_previous_binary_rebuilds_newer_schema","current_binary_restored_after_actual_downgrade","actual_current_database_backup_restore"]
assert all(c["status"]=="passed" for c in pair["cases"])
assert pair["cases"][0]["source_commit"]=="277f2490fad3fa30f2812b5547bad033867c9ea5" and pair["cases"][0]["user_version_injection"] is False
pstates={}
for key,name in [("current_database_backup","current-original.sqlite3"),("previous_database_backup","previous-rebuilt.sqlite3"),("current_rebuilt_backup","current-rebuilt.sqlite3")]:
    path=phome/"backups"/name
    assert digest(path)==pair[key]["sha256"]
    state,_=snapshot_copy(path)
    assert state==pair[key]["snapshot"]
    pstates[key]=state
final_p,_=snapshot_copy(phome/"fixture"/".codecortex"/"index.sqlite3")
assert [pstates[k]["schema_version"] for k in ("current_database_backup","previous_database_backup","current_rebuilt_backup")]==[25,24,25]
first,second,third=pair["cases"]
assert first["before"]["database"]==pstates["current_database_backup"]
assert first["after"]["database"]==pstates["previous_database_backup"]
assert second["result"]["database"]==pstates["current_rebuilt_backup"]
assert third["result"]["database"]==final_p and final_p==pstates["current_database_backup"]
for label,record in [("01-current-build",first["before"]),("02-previous-opens-current",first["after"]),("03-current-restored",second["result"]),("04-original-backup-restored",third["result"])]:
    stderr=phome/label/"product-stderr.log"
    assert digest(stderr)==record["stderr_sha256"]
    count=stderr.read_text().count("index schema version mismatch, rebuild required")
    assert count==record["schema_mismatch_diagnostics"]
    if label in ("02-previous-opens-current","03-current-restored"):
        assert count>0
    assert json.loads((phome/label/"local.json").read_text())==record["local"]
for label,key in [("current","default"),("previous","previous-default")]:
    assert digest(phome/(label+"-product")/"codecortex")==j["products"][key]["binary_sha256"]
    assert json.loads((phome/(label+"-product")/"build-receipt.json").read_text())==j["products"][key]
assert pair["source_unchanged"] is True and source_manifest(phome/"fixture")==pair["source_files"]==json.loads((phome/"source-before.json").read_text())
assert digest(phome/"fixture"/".codecortex.json")==digest(phome/"backups"/"config-original.json")==pair["configuration_sha256"]
assert json.loads((phome/"fixture"/".codecortex.json").read_text())["semantic"]["enabled"] is False
assert source_snapshot(source)==j["source"]
assert full_evidence_manifest(home)==j["evidence_files_sha256"],"original artifact changed"
result={"schema_version":1,"status":"accepted_original_database_config_and_version_rollback","source_commit":j["source"]["source_commit"],"historical_source_commit":"277f2490fad3fa30f2812b5547bad033867c9ea5","artifact_id":11592146562,"local_databases":local,"active_databases":active,"rollback_schemas":[25,1025,25,25],"actual_historical_schemas":[25,24,25,25],"database_copies":copied,"source_and_config_unchanged":True,"original_artifact_inventory_and_hashes_unchanged":True,"limitations":j["limitations"],"inactive_cache_reader_not_run_preserved":True,"todo_status_change":False}
(base/"review-platform"/"recovery-database-rollback-audit.json").write_text(json.dumps(result,indent=2)+"\n")
print(json.dumps({"status":result["status"],"copied_database_count":len(copied),"local_databases":local,"active_databases":active,"rollback_schemas":result["rollback_schemas"],"actual_historical_schemas":result["actual_historical_schemas"],"source_and_config_unchanged":True,"original_inventory_unchanged":True},indent=2))
