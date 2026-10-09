import json,subprocess,datetime,sys,hashlib
from pathlib import Path
payload=json.load(sys.stdin)
base=Path("/Users/jin/Desktop/codecortex-rust/artifacts/benchmarks/p8-pr184-native-20261009-root"); work=base/"worktree"; user=Path("/Users/jin/Desktop/codecortex-rust")
def git(where,args): return subprocess.check_output(["/usr/bin/git",*args],cwd=where,text=True)
def state(where):
    ids=git(where,["rev-parse","HEAD","HEAD^{tree}"]).splitlines()
    return {"head":ids[0],"tree":ids[1],"status":git(where,["status","--porcelain=v1","--untracked-files=normal"])}
before=state(work)
assert before=={"head":"cfc77b68828228cea1466e0161f9dc745fe5aef3","tree":"35cd8de386bd5b4d3755929743efe7bc8a9544fd","status":""},before
user_head=git(user,["rev-parse","HEAD"]).strip();tracked=git(user,["diff","--name-only","HEAD","--"])
assert user_head=="ff458bc591b4e7e444af4464d6eef2513cdb335c" and not tracked
fetch=subprocess.run(["/usr/bin/git","fetch","--no-tags","--no-write-fetch-head","https://github.com/jyqj/codecortex.git","883230f2b44ebcf09495870db8c0e31d61270331"],cwd=work,capture_output=True,text=True,timeout=180)
assert fetch.returncode==0,fetch.stderr
actual=git(work,["show","-s","--format=%T%n%P","883230f2b44ebcf09495870db8c0e31d61270331"]).splitlines()
assert actual==["e738e0d6bd25f18775379dc6e3849ac9373b92d6","d7512ed031f4027437dc756012cb7f21582b50f7"],actual
sp=subprocess.run(["/usr/bin/git","sparse-checkout","set","--no-cone","--stdin"],cwd=work,input=payload["sparse_rules"],capture_output=True,text=True,timeout=180)
assert sp.returncode==0,sp.stderr
change=subprocess.run(["/usr/bin/git","checkout","--detach","883230f2b44ebcf09495870db8c0e31d61270331"],cwd=work,capture_output=True,text=True,timeout=180)
assert change.returncode==0,change.stderr
after=state(work)
assert after=={"head":"883230f2b44ebcf09495870db8c0e31d61270331","tree":"e738e0d6bd25f18775379dc6e3849ac9373b92d6","status":""},after
assert git(user,["rev-parse","HEAD"]).strip()==user_head and git(user,["diff","--name-only","HEAD","--"])==tracked
receipt={"schema":"p8-pr184-finalG-P4-guard-checkout-v1","before":before,"after":after,"user_checkout_head":user_head,"user_tracked_diff_unchanged":True,"fetch":{"exit":fetch.returncode,"stdout":fetch.stdout,"stderr":fetch.stderr},"checkout":{"exit":change.returncode,"stdout":change.stdout,"stderr":change.stderr},"sparse":{"exit":sp.returncode,"stdout":sp.stdout,"stderr":sp.stderr,"rules_sha256":hashlib.sha256(payload["sparse_rules"].encode()).hexdigest()},"observed_at_utc":datetime.datetime.now(datetime.timezone.utc).isoformat()}
with (base/"finalG-P4-guard-checkout.json").open("x") as f:json.dump(receipt,f,indent=2);f.write("\n")
print(json.dumps(receipt))
