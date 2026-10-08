#!/usr/bin/env python3
import hashlib,json,pathlib,subprocess
out=pathlib.Path("artifacts/checkpoints/p8-product-repair-20261008/counterexample")
out.mkdir(parents=True,exist_ok=True)
cmd=["cargo","test","-p","cc-index","--test","explicit_text_admission","--","--nocapture"]
r=subprocess.run(cmd,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,timeout=900)
(out/"original-tests-only.log").write_bytes(r.stdout)
s=r.stdout.decode("utf-8","replace")
receipt={"schema_version":1,"source_commit":subprocess.check_output(["git","rev-parse","HEAD"]).decode().strip(),"argv":cmd,"returncode":r.returncode,"log_bytes":len(r.stdout),"log_sha256":hashlib.sha256(r.stdout).hexdigest(),"failure_summary":[x for x in s.splitlines() if x.startswith("test ") or "panicked at" in x or x.startswith("error:")],"expected_runtime_counterexample":r.returncode==101 and "test result: FAILED." in s and "panicked at" in s,"not_a_fixed_product_result":True}
(out/"receipt.json").write_text(json.dumps(receipt,ensure_ascii=False,indent=2)+"\n")
print("P8_COUNTEREXAMPLE "+json.dumps(receipt,separators=(",",":")),flush=True)
if not receipt["expected_runtime_counterexample"]:
    raise SystemExit("Missing actual expected runtime test failure; inspect retained log.")
