import hashlib, json, pathlib, subprocess

ROOT = pathlib.Path(__file__).resolve().parent
REPO = ROOT.parents[3]
SHAS = {'old': 'b25723458a77edd2207ed1a52dceb0ea1009cb93',
        'rejected': 'da5b05ee08d84fd336d5a98f25da7cae176daebf',
        'repaired': '671063b11af8cb40a0d526098de82e684dd24aca'}
def git(*args):
    return subprocess.check_output(['git', '-C', str(REPO), *args])
manifest = {}
for label, sha in SHAS.items():
    # Materialize only build sources. Never read benchmark/holdout content.
    paths = git('ls-tree', '-r', '--name-only', sha).decode().splitlines()
    selected = [p for p in paths if p in ('Cargo.toml', 'Cargo.lock') or
                (p.startswith('crates/') and ('/src/' in p or p.endswith('/Cargo.toml')))]
    selected = [p for p in selected if 'public-v19' not in p and 'holdout' not in p]
    dest = ROOT / 'snapshots' / label
    manifest[label] = {'sha': sha, 'files': {}}
    for p in selected:
        data = git('show', f'{sha}:{p}')
        target = dest / p
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)
        manifest[label]['files'][p] = hashlib.sha256(data).hexdigest()
    helper = (dest / 'crates/cc-index/src/resolver/helpers.rs').read_text()
    start = helper.index('pub(in crate::resolver) fn type_atoms(')
    end = helper.find('\n#[cfg(test)]', start)
    function = helper[start:end if end != -1 else len(helper)]
    (ROOT / f'harness-{label}/src').mkdir(parents=True, exist_ok=True)
    (ROOT / f'harness-{label}/src/atoms.rs').write_text(function.replace('pub(in crate::resolver)', 'pub'))
    (ROOT / f'harness-{label}/src/main.rs').write_text((ROOT / 'harness.rs').read_text())
    (ROOT / f'harness-{label}/Cargo.lock').write_bytes((dest / 'Cargo.lock').read_bytes())
    (ROOT / f'harness-{label}/Cargo.toml').write_text(f'''[package]
name = "independent-requests-recheck-{label}"
version = "0.0.0"
edition = "2021"
[workspace]
[dependencies]
cc-index = {{path = "../snapshots/{label}/crates/cc-index"}}
cc-model = {{path = "../snapshots/{label}/crates/cc-model"}}
cc-db = {{path = "../snapshots/{label}/crates/cc-db"}}
serde_json = "1"
rusqlite = {{version = "0.40", features = ["bundled"]}}
''')
(ROOT / 'source-manifest.json').write_text(json.dumps(manifest, indent=2))
