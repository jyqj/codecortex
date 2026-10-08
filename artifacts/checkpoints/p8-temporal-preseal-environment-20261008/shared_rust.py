#!/usr/bin/env python3
"""Observe identical fixed Rust paths for the parent and an isolated child HOME."""
import hashlib,json,os,shutil,stat,subprocess
from pathlib import Path
def need(value,message):
    if not value: raise ValueError(message)
def run(argv,env):
    result=subprocess.run(argv,env=env,stdout=subprocess.PIPE,stderr=subprocess.PIPE,timeout=60,check=False)
    need(result.returncode==0,"Rust environment probe failed: "+repr(argv)+" "+result.stderr.decode("utf-8","replace"))
    return result.stdout.decode("utf-8")
def rec(path):
    path=path.resolve(strict=True); data=path.read_bytes()
    return {"path":str(path),"bytes":len(data),"mode":stat.S_IMODE(path.stat().st_mode),"sha256":hashlib.sha256(data).hexdigest()}
need(os.environ["RUSTUP_HOME"]=="/home/runner/.rustup" and os.environ["CARGO_HOME"]=="/home/runner/.cargo","explicit fixed hosted-runner Rust homes")
need(os.environ["RUSTUP_TOOLCHAIN"]=="1.95.0","original fixed Rust version")
home=Path(os.environ["RUNNER_TEMP"])/"ptv2-rust-probe-home"
home.mkdir(exist_ok=False)
child={key:os.environ[key] for key in ("PATH","RUSTUP_HOME","CARGO_HOME","RUSTUP_TOOLCHAIN")}
child.update({"HOME":str(home),"LANG":"C.UTF-8","LC_ALL":"C.UTF-8"})
parent_home=run(["rustup","show","home"],os.environ).strip()
child_home=run(["rustup","show","home"],child).strip()
need(parent_home==child_home==os.environ["RUSTUP_HOME"],"Rust home routing differs")
records={}
for tool in ("rustc","cargo"):
    argv=["rustup","which","--toolchain","1.95.0",tool]
    parent_path=Path(run(argv,os.environ).strip()).resolve(strict=True)
    child_path=Path(run(argv,child).strip()).resolve(strict=True)
    need(parent_path==child_path and parent_path.is_relative_to(Path(parent_home).resolve()),"actual child Rust binary differs")
    parent_version=run([tool,"-Vv"],os.environ)
    child_version=run([tool,"-Vv"],child)
    direct_version=run([str(parent_path),"-Vv"],child)
    need(parent_version==child_version==direct_version and parent_version.startswith(tool+" 1.95.0 "),"Rust version or actual direct binary differs")
    records[tool]={"actual_binary":rec(parent_path),"parent_entrypoint":rec(Path(shutil.which(tool))),"parent_version":parent_version,"isolated_child_version":child_version,"direct_binary_version":direct_version}
need(not (home/".rustup").exists() and not (home/".cargo").exists(),"isolated child installed a new toolchain")
value={"schema_version":1,"status":"same_fixed_parent_and_child_rust_paths_verified","rustup_home":parent_home,"cargo_home":os.environ["CARGO_HOME"],"toolchain":"1.95.0","tools":records,"scope":"Actual parent and isolated-HOME version probes with the same Rust environment forwarded by the unchanged temporal3.1 allowlist. No claim of complete OS shared-library isolation.","new_product_queries":0}
out=Path(os.environ["RUNNER_TEMP"])/"ptv2-shared-rust-environment.json"
out.write_text(json.dumps(value,sort_keys=True,indent=2)+"\n")
print("P8_SHARED_RUST_RUNTIME "+json.dumps(value,sort_keys=True),flush=True)
