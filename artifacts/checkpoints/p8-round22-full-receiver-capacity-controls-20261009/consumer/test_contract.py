#!/usr/bin/env python3
"""Pure synthetic controls; never run receiver operations, Git, ELF, Docker or network."""
import argparse,ast,copy,hashlib,io,json,math,pathlib,sys,tempfile,types,unittest
if sys.flags.optimize: raise SystemExit("optimized Python forbidden")
parser=argparse.ArgumentParser()
parser.add_argument("--controller",required=True,type=pathlib.Path)
parser.add_argument("--operator",required=True,type=pathlib.Path)
parser.add_argument("--matrix",required=True,type=pathlib.Path)
parser.add_argument("--output",required=True,type=pathlib.Path)
args=parser.parse_args()
wanted={"controller":(38134,"678e163df14847203a0c71197afc58d617de7bacc437ed785c11a33dce25ee1c"),
        "matrix":(65092,"f169b0f26cf2a87d3e2548d734431c530cbf63d353ac054045ec8be80889f957"),
        "operator":(9745,"60a65b720fd94901fea51dd02f98e7f05d0fa1fdf41038ec79b4714d84a928cd")}
payload,modules={},{}
for name in ("controller","operator","matrix"):
    body=getattr(args,name).read_bytes()
    assert (len(body),hashlib.sha256(body).hexdigest())==wanted[name]
    payload[name]=body.decode()
    parsed=ast.parse(body,filename=name+".py")
    compile(parsed,name+".py","exec")
    modules[name]={"bytes":len(body),"sha256":hashlib.sha256(body).hexdigest(),"AST_and_compile":"passed",
                   "executed_as_operation":False}
ns={"__name__":"synthetic_receiver","__file__":str(args.controller)}
exec(compile(payload["controller"],"synthetic_receiver.py","exec"),ns)

class ReceptionContractTests(unittest.TestCase):
    def make(self, remote=False):
        expected = ns["expected_entries"]()
        jobs = [{"id": 8000000+i, "name": name, "run_id": ns["RUN"],
                 "head_sha": ns["SOURCE"], "run_attempt": 1, "status": "completed",
                 "conclusion": "failure" if name=="aggregate" else "success"}
                for i,name in enumerate(sorted({e["job_name"] for e in expected.values()}|{"aggregate"}))]
        artifacts=[]
        entries=list(sorted(expected))
        if remote: entries.append("p8-scale-matrix-"+str(ns["RUN"]))
        for i,name in enumerate(entries):
            known=ns["KNOWN_ARTIFACTS"].get(name, {"id":9000000+i,"bytes":123,"sha256":"a"*64})
            artifacts.append({"id":known["id"],"name":name,"size_in_bytes":known["bytes"],
                              "digest":"sha256:"+known["sha256"],"expired":False,
                              "workflow_run":{"id":ns["RUN"],"head_sha":ns["SOURCE"],"head_branch":ns["SOURCE_BRANCH"]}})
        return {"run":{"id":ns["RUN"],"head_sha":ns["SOURCE"],"run_attempt":1,
                       "event":"pull_request","path":".github/workflows/p8-scale.yml",
                       "workflow_id":378814687,"head_branch":ns["SOURCE_BRANCH"],
                       "created_at":"2026-10-09T06:56:25Z","status":"completed","conclusion":"failure"},
                "jobs":jobs,"artifacts":artifacts}
    def test_301_core_and_failed_aggregate_job_not_rejected(self):
        result=ns["register"](self.make())
        self.assertTrue(result["complete_core_301"])
        self.assertEqual(len(result["artifacts"]),301)
        self.assertFalse(result["remote_matrix_present"])
        self.assertEqual(result["remote_run_conclusion"],"failure")
    def test_optional_remote_artifact_retained(self):
        result=ns["register"](self.make(True))
        self.assertTrue(result["complete_core_301"])
        self.assertTrue(result["remote_matrix_present"])
        self.assertEqual(len(result["artifacts"]),302)
    def test_missing_original_is_reported_not_counted(self):
        fixture=self.make()
        missing=fixture["artifacts"].pop()["name"]
        result=ns["register"](fixture)
        self.assertFalse(result["complete_core_301"])
        self.assertEqual(result["missing_core_artifacts"],[missing])
        self.assertEqual(len(result["artifacts"]),300)
    def test_wrong_source_or_attempt_or_created_time_rejected(self):
        for key,value in (("head_sha","0"*40),("run_attempt",2),("created_at","2026-10-09T06:56:26Z")):
            fixture=self.make();fixture["run"][key]=value
            with self.subTest(key=key),self.assertRaises(ValueError): ns["register"](fixture)
    def test_artifact_and_job_source_mismatch_rejected(self):
        fixture=self.make();fixture["artifacts"][0]["workflow_run"]["head_sha"]="0"*40
        with self.assertRaises(ValueError): ns["register"](fixture)
        fixture=self.make();fixture["jobs"][0]["run_attempt"]=2
        with self.assertRaises(ValueError): ns["register"](fixture)
    def test_duplicate_extra_and_expired_artifacts_rejected(self):
        fixture=self.make();fixture["artifacts"].append(copy.deepcopy(fixture["artifacts"][0]))
        with self.assertRaises(ValueError): ns["register"](fixture)
        fixture=self.make();fixture["artifacts"][0]["name"]="not-registered"
        with self.assertRaises(ValueError): ns["register"](fixture)
        fixture=self.make();fixture["artifacts"][0]["expired"]=True
        with self.assertRaises(ValueError): ns["register"](fixture)
    def test_previously_received_original_identity_pinned(self):
        fixture=self.make()
        row=next(r for r in fixture["artifacts"] if r["name"] in ns["KNOWN_ARTIFACTS"])
        row["digest"]="sha256:"+"b"*64
        with self.assertRaises(ValueError): ns["register"](fixture)
    def test_new_upload_not_added_to_selected_batch(self):
        first=self.make();saved=first["artifacts"].pop()
        before=ns["register"](first)
        first["artifacts"].append(saved);after=ns["register"](first)
        self.assertEqual(ns["selected_entries_unchanged"](before,after),[saved["name"]])
        self.assertEqual(len(before["artifacts"]),300)
    def test_selected_identity_change_refused(self):
        before=ns["register"](self.make());after=copy.deepcopy(before)
        after["artifacts"][0]["sha256"]="f"*64
        with self.assertRaises(ValueError): ns["selected_entries_unchanged"](before,after)
    def test_exact_full_directory_relocation(self):
        directories={f"p8-scale-shard-{scale}-{index}-{ns['RUN']}":pathlib.Path("/owned")/f"p8-scale-shard-{scale}-{index}-{ns['RUN']}"
                     for scale in ns["SCALES"] for index in range(30)}
        value={"evidence":[{"directory":str(ns["REMOTE_SHARDS_ROOT"]/name),"receipt_sha256":"d"*64} for name in sorted(directories)],
               "unchanged_value":{"number":1.25,"samples":1500}}
        normalized,changes=ns["canonical_matrix"](value,directories)
        self.assertEqual(len(changes),150)
        self.assertEqual(normalized["unchanged_value"],value["unchanged_value"])
        self.assertTrue(all(row["directory"]==str(directories[pathlib.PurePosixPath(row["directory"]).name]) for row in normalized["evidence"]))
        self.assertTrue(all(row["directory"].startswith(str(ns["REMOTE_SHARDS_ROOT"])) for row in value["evidence"]))
    def test_relocation_rejects_missing_duplicate_extra_field(self):
        directories={f"x-{i}":pathlib.Path("/owned")/f"x-{i}" for i in range(150)}
        value={"evidence":[{"directory":"/old/"+name,"receipt_sha256":"d"*64} for name in directories]}
        missing=copy.deepcopy(value);missing["evidence"].pop()
        with self.assertRaises(ValueError): ns["canonical_matrix"](missing,directories)
        duplicate=copy.deepcopy(value);duplicate["evidence"][1]=duplicate["evidence"][0]
        with self.assertRaises(ValueError): ns["canonical_matrix"](duplicate,directories)
        extra=copy.deepcopy(value);extra["evidence"][0]["unregistered"]=1
        with self.assertRaises(ValueError): ns["canonical_matrix"](extra,directories)

    def test_remote_matrix_original_json_limit_capacity_boundaries(self):
        # Compile only these exact original functions; never import a producer or run aggregate.
        names={"require","unique_object","decode","read_json","exact_equal"}
        tree=ast.parse(payload["matrix"],filename="original_matrix.py")
        selected=[node for node in tree.body if isinstance(node,ast.FunctionDef) and node.name in names]
        self.assertEqual({node.name for node in selected},names)
        self.assertEqual(len(selected),len(names))
        real={"Path":pathlib.Path,"json":json,"math":math}
        exec(compile(ast.Module(body=selected,type_ignores=[]),"original_matrix.py","exec"),real)
        matrix=types.SimpleNamespace(inventory=ns["inventory"],read_json=real["read_json"],exact_equal=real["exact_equal"])
        with tempfile.TemporaryDirectory(prefix="p8-full-receiver-json-capacity-") as temporary:
            root=pathlib.Path(temporary)
            remote=root/"remote";remote.mkdir()
            directories={f"p8-scale-matrix-{ns['RUN']}":remote}
            shards={f"p8-scale-shard-{scale}-{index}-{ns['RUN']}":root/f"shard-{scale}-{index}"
                    for scale in ns["SCALES"] for index in range(30)}
            value={"passed":True,"status":"synthetic_exact_comparison_only",
                   "evidence":[{"directory":str(ns["REMOTE_SHARDS_ROOT"]/name),"receipt_sha256":"a"*64}
                               for name in sorted(shards)]}
            wanted,_=ns["canonical_matrix"](value,shards)
            compact=json.dumps(value,sort_keys=True,separators=(",",":")).encode()
            registration={"remote_matrix_present":True,"remote_run_status":"completed","remote_run_conclusion":"success"}
            for index,size in enumerate((8*1024**2+1,16*1024**2,16*1024**2+1)):
                with self.subTest(bytes=size):
                    path=remote/"matrix.json"
                    with path.open("wb") as stream:
                        stream.write(compact)
                        remaining=size-len(compact)
                        while remaining:
                            block=b" "*min(64*1024,remaining)
                            stream.write(block);remaining-=len(block)
                    self.assertEqual(path.stat().st_size,size)
                    before=ns["file_row"](path)
                    # The unchanged default remains 8MiB.
                    with self.assertRaises(ValueError): real["read_json"](path)
                    review=root/f"review-{index}";review.mkdir()
                    if size<=16*1024**2:
                        report=ns["compare_remote_matrix"](wanted,registration,directories,shards,matrix,review)
                        self.assertTrue(report["successful_remote_matrix_exact_equal"])
                        self.assertEqual(report["original_matrix_sha256"],before["sha256"])
                        self.assertEqual(len(ns["json_load"](review/"remote-directory-relocations.json")),150)
                    else:
                        with self.assertRaises(ValueError):
                            ns["compare_remote_matrix"](wanted,registration,directories,shards,matrix,review)
                        self.assertEqual(list(review.iterdir()),[])
                    self.assertEqual(ns["file_row"](path),before)

suite=unittest.defaultTestLoader.loadTestsFromTestCase(ReceptionContractTests)
expected=sorted(t.id() for t in suite)
stream=io.StringIO()
result=unittest.TextTestRunner(stream=stream,verbosity=2).run(suite)
passed=result.wasSuccessful() and result.testsRun==12 and not result.skipped
report={"schema":"p8-275e-full-consumer-author-static-and-synthetic-v1","modules":modules,
        "methods":expected,"actual_tests":result.testsRun,"failed":len(result.failures),"errors":len(result.errors),
        "skipped":len(result.skipped),"stdout_stderr":stream.getvalue(),
        "synthetic_scope":"The original 11 metadata methods plus one real temporary JSON-file boundary method using five unmodified original matrix functions and derived compare_remote_matrix; no network, ZIP, aggregate, ELF, product or Cargo invocation.",
        "receiver_executed":False,"operator_executed":False,
        "status":"passed_static_and_synthetic_only" if passed else "failed"}
assert args.output.is_absolute() and args.output.parent.resolve(strict=True)==args.output.parent and not args.output.exists()
with args.output.open("x") as output: json.dump(report,output,sort_keys=True,indent=2);output.write("\n")
print(stream.getvalue())
print(json.dumps({"status":report["status"],"report_sha256":hashlib.sha256(args.output.read_bytes()).hexdigest()}))
raise SystemExit(0 if passed else 1)
