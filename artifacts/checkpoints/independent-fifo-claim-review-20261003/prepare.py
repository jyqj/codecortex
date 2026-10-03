"""Recreate fixed sources/custom drivers under local/ in a fresh evidence checkout.
Needs the four commits in git object store and the documented installed Rust toolchain.
Does not alter production checkout, credentials, permissions, or system configuration.
"""
from pathlib import Path
import io,tarfile,subprocess,os
p=Path(__file__).resolve().parent
toolchain=Path('/workspace/.rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin')
env=dict(os.environ,PATH=str(toolchain)+os.pathsep+os.environ['PATH'],CARGO_HOME='/workspace/.cargo',CARGO_TARGET_DIR=str(p/'local/target'))
for n,s in [('old','5ffbadcf48e26523b2eb46beda0d187a2e2e29cd'),('candidate','807f4710495a38bc6631549da2ab9623c83684b8')]:
    d=p/'local'/n;d.mkdir(parents=True,exist_ok=True)
    data=subprocess.check_output(['git','archive',s,'Cargo.toml','Cargo.lock','crates'])
    with tarfile.open(fileobj=io.BytesIO(data)) as t:t.extractall(d,filter='data')
    driver=p/'local'/('driver-'+n);(driver/'src').mkdir(parents=True,exist_ok=True)
    (driver/'src/main.rs').write_bytes((p/'driver.rs').read_bytes())
    for f in ['Cargo.toml','Cargo.lock']:(driver/f).write_bytes((p/(n+'-driver-'+f)).read_bytes())
    with (p/('reproduce-build-'+n+'.log')).open('w') as log:
        subprocess.run([str(toolchain/'cargo'),'build','--locked','--manifest-path',str(driver/'Cargo.toml')],env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
