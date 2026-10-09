#!/usr/bin/env python3
"""Prepare an isolated required-file projection of actual P2; no guard/native run."""
import hashlib
import json
import os
from pathlib import Path
import stat
import subprocess
from datetime import datetime, timezone

B = Path('/workspace/scratch/a217aaae3bde')
OUT = B / 'p8-db-lock-observation-integration/LG2-actual-P2-proof-v2'
OLD = B / 'p8-lock-observation-validated'
VIEW = B / 'p8-lock-observation-P2-validated'
GITDIR = Path('/dev/shm/a217aaae3bde/P2-required-view.git')
P2 = 'b79f7749eb8f0a6c728493adfb073ca7db5563be'
TREE = '4140bc1bcdef409817e6edc381411b919dabe14e'
LG = '18255c53b7fa153bb71c96f57f1b97ca139e427f'
BASE = '7354db236c9d9850a75f31672697ae9eab44565e'
ENV = dict(os.environ, GIT_NO_LAZY_FETCH='1', GIT_TERMINAL_PROMPT='0')


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def oid(raw):
    return hashlib.sha1(b'blob ' + str(len(raw)).encode() + b'\0' + raw).hexdigest()


def canonical(value):
    return (json.dumps(value, indent=2, sort_keys=True) + '\n').encode()


def git(*args, root=OLD, data=None):
    return subprocess.check_output(['git', '-C', str(root), *args], input=data,
                                   env=ENV, stderr=subprocess.PIPE)


def tree(ref):
    result = {}
    for row in git('ls-tree', '-r', '-z', ref).split(b'\0'):
        if not row:
            continue
        meta, path = row.split(b'\t', 1)
        mode, kind, object_id = meta.decode().split()
        result[path.decode()] = (mode, kind, object_id)
    return result


def write(name, value):
    raw = canonical(value)
    path = OUT / name
    with path.open('xb') as stream:
        stream.write(raw)
    return {'path': str(path), 'bytes': len(raw), 'sha256': sha(raw), 'git_blob': oid(raw)}


def main():
    assert not OUT.exists() and not VIEW.exists() and not GITDIR.exists()
    OUT.mkdir(parents=True)
    integration = OUT.parent
    expected = json.loads((integration / 'LG2-three-path-repair-inputs.json').read_bytes())
    registry = json.loads((integration / 'LG2-old-registry.json').read_bytes())
    assert git('rev-parse', 'HEAD').decode().strip() == LG
    assert git('rev-parse', P2 + '^{tree}').decode().strip() == TREE
    assert git('rev-list', '--parents', '-n', '1', P2).decode().split() == [P2, LG]
    assert git('diff', '--name-only') == b''
    assert git('diff', '--cached', '--name-only') == b''
    old_gitdir = Path(git('rev-parse', '--absolute-git-dir').decode().strip())
    protected = {str(p): sha(p.read_bytes()) for p in [old_gitdir / 'HEAD', old_gitdir / 'index']}
    before_tree, after_tree, base_tree = tree(LG), tree(P2), tree(BASE)
    changed = {p for p in before_tree.keys() | after_tree.keys()
               if before_tree.get(p) != after_tree.get(p)}
    assert changed == {row['path'] for row in expected['changes']}

    GITDIR.parent.mkdir(parents=True, exist_ok=True)
    subprocess.check_call(['git', 'init', '-q', '--separate-git-dir=' + str(GITDIR), str(VIEW)], env=ENV)
    common = Path(git('rev-parse', '--git-common-dir').decode().strip()).resolve()
    (GITDIR / 'objects/info/alternates').write_text(str(common / 'objects') + '\n')
    git('config', 'gc.auto', '0', root=VIEW)
    git('config', 'core.autocrlf', 'false', root=VIEW)
    assert git('remote', root=VIEW) == b''
    hydrated = []
    for row in expected['changes']:
        source = B / 'p8-lock-observation-work' / row['path']
        raw = source.read_bytes()
        assert len(raw) == row['bytes'] and sha(raw) == row['sha256'] and oid(raw) == row['git_blob']
        assert after_tree[row['path']][2] == oid(raw)
        assert git('hash-object', '-w', '--stdin', root=VIEW, data=raw).decode().strip() == oid(raw)
        hydrated.append(dict(source=str(source), **row))
    hydration_record = write('exact-three-private-object-materialization.json', hydrated)
    batch = subprocess.Popen(['git', '-C', str(VIEW), 'cat-file', '--batch'],
                             stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                             stderr=subprocess.PIPE, env=ENV)
    cache = {}

    def blob(object_id):
        if object_id not in cache:
            batch.stdin.write(object_id.encode() + b'\n')
            batch.stdin.flush()
            header = batch.stdout.readline().decode().split()
            assert len(header) == 3 and header[:2] == [object_id, 'blob'], header
            size = int(header[2])
            raw = batch.stdout.read(size)
            assert len(raw) == size and batch.stdout.read(1) == b'\n'
            assert oid(raw) == object_id
            cache[object_id] = raw
        return cache[object_id]

    def source_path(path):
        return path in ('Cargo.toml', 'Cargo.lock') or path.startswith('crates/')

    def validation_path(path):
        return (path.startswith(('scripts/', '.github/workflows/', 'tests/source_integrity/'))
                and path not in {'scripts/verify_reviewed_source_v15.py',
                                 'scripts/reviewed-source-registry-v15.json',
                                 '.github/workflows/ci.yml'}
                and not ('__pycache__' in Path(path).parts and path.endswith('.pyc')))

    product_paths = {p for p in after_tree if source_path(p)}
    validation_paths = {p for p in after_tree if validation_path(p)}
    assert len(product_paths) == 1094 and len(validation_paths) == 142
    assert product_paths == set(registry['complete_inputs'])
    assert validation_paths == set(registry['validation_inputs'])
    full = {}
    for p in sorted(product_paths | validation_paths):
        mode, kind, object_id = after_tree[p]
        assert kind == 'blob' and mode in ('100644', '100755')
        raw = blob(object_id)
        full[p] = dict(bytes=len(raw), mode=mode, sha=object_id, sha256=sha(raw), type=kind)
    product = {p: full[p]['sha256'] for p in sorted(product_paths)}
    validation = {p: full[p]['sha256'] for p in sorted(validation_paths)}
    expected_product = dict(registry['complete_inputs'])
    expected_validation = dict(registry['validation_inputs'])
    for row in expected['changes']:
        assert full[row['path']]['bytes'] == row['bytes']
        assert full[row['path']]['sha'] == row['git_blob']
        assert full[row['path']]['sha256'] == row['sha256']
        (expected_product if source_path(row['path']) else expected_validation)[row['path']] = row['sha256']
    assert product == expected_product and validation == expected_validation
    actual_delta_paths = {p for p in base_tree.keys() | after_tree.keys()
                          if source_path(p) and base_tree.get(p) != after_tree.get(p)}
    assert len(actual_delta_paths) == 60 and actual_delta_paths == set(registry['delta'])
    delta = {}
    for p in sorted(actual_delta_paths):
        before = None if p not in base_tree else sha(blob(base_tree[p][2]))
        delta[p] = dict(before_sha256=before, sha256=product[p])
        assert registry['delta'][p]['before_sha256'] == before
        if p not in changed:
            assert registry['delta'][p] == delta[p]
    for row in expected['current15']:
        raw = blob(after_tree[row['path']][2])
        assert len(raw) == row['bytes'] and sha(raw) == row['sha256'] and oid(raw) == row['git_blob']

    outputs = {}
    for label, value in [('product-domain', product), ('validation-domain', validation),
                         ('historical-delta', delta), ('full-leaf', full)]:
        independent_raw = canonical(value)
        assert independent_raw == (integration / ('LG2-' + label + '-draft.json')).read_bytes(), label
        outputs[label] = write(label + '.json', value)
    print(json.dumps({'stage': 'actual_git_domains_verified', 'counts': [1094, 142, 60],
                      'outputs': outputs}), flush=True)

    materialized = []
    for p, (mode, kind, object_id) in sorted(before_tree.items()):
        path = OLD / p
        if not path.exists():
            continue
        assert kind == 'blob' and not path.is_symlink() and path.is_file(), p
        st = path.stat()
        raw = path.read_bytes()
        assert oid(raw) == object_id
        assert stat.S_IMODE(st.st_mode) == int(mode[-3:], 8)
        materialized.append(dict(path=p, bytes=len(raw), sha256=sha(raw), git_blob=object_id,
                                 mode=mode, device=st.st_dev, inode=st.st_ino,
                                 mtime_ns=st.st_mtime_ns, physical_mode=stat.S_IMODE(st.st_mode)))
    assert len(materialized) == 1850
    materialized_paths = {x['path'] for x in materialized}
    assert product_paths | validation_paths <= materialized_paths
    assert changed <= materialized_paths
    before_record = write('old-materialized-before.json', materialized)
    git('update-ref', '--no-deref', 'HEAD', P2, root=VIEW)
    git('read-tree', P2, root=VIEW)
    allpaths = b'\0'.join(p.encode() for p in sorted(after_tree)) + b'\0'
    git('update-index', '--skip-worktree', '-z', '--stdin', root=VIEW, data=allpaths)
    for row in materialized:
        p = row['path']
        target = VIEW / p
        target.parent.mkdir(parents=True, exist_ok=True)
        if p in changed:
            raw = blob(after_tree[p][2])
            with target.open('xb') as stream:
                stream.write(raw)
            target.chmod(int(after_tree[p][0][-3:], 8))
            assert target.stat().st_ino != (OLD / p).stat().st_ino
        else:
            assert before_tree[p] == after_tree[p]
            os.link(OLD / p, target)
            assert target.stat().st_ino == row['inode']
    present = b'\0'.join(p.encode() for p in sorted(materialized_paths)) + b'\0'
    git('update-index', '--no-skip-worktree', '-z', '--stdin', root=VIEW, data=present)
    actual_after = []
    for row in materialized:
        p = row['path']
        raw = (OLD / p).read_bytes()
        st = (OLD / p).stat()
        assert sha(raw) == row['sha256'] and oid(raw) == row['git_blob']
        assert st.st_dev == row['device'] and st.st_ino == row['inode']
        assert st.st_mtime_ns == row['mtime_ns'] and stat.S_IMODE(st.st_mode) == row['physical_mode']
        raw = (VIEW / p).read_bytes()
        assert oid(raw) == after_tree[p][2]
        assert stat.S_IMODE((VIEW / p).stat().st_mode) == int(after_tree[p][0][-3:], 8)
        actual_after.append(dict(path=p, bytes=len(raw), sha256=sha(raw), git_blob=oid(raw),
                                 mode=after_tree[p][0], hardlink_reused=p not in changed))
    assert {p: sha(Path(p).read_bytes()) for p in protected} == protected
    assert git('rev-parse', 'HEAD').decode().strip() == LG
    assert git('rev-parse', 'HEAD', root=VIEW).decode().strip() == P2
    assert git('diff', '--name-only', root=VIEW) == b''
    assert git('diff', '--cached', '--name-only', root=VIEW) == b''
    assert git('remote', root=VIEW) == b''
    batch.stdin.close()
    assert batch.wait(timeout=10) == 0 and batch.stderr.read() == b''
    view_record = write('P2-materialized-view.json', actual_after)
    proof = dict(schema=1, verdict='accepted_scoped_actual_input_identity',
                 completed_at=datetime.now(timezone.utc).isoformat(), source=P2, tree=TREE,
                 parent=LG, base=BASE, exact_changes=expected['changes'],
                 private_object_materialization=hydration_record,
                 all_fifteen_frozen_implementation_paths_match=True,
                 product_input_count=1094, validation_input_count=142, base_delta_count=60,
                 manifests=outputs, old_materialized=before_record, new_materialized=view_record,
                 old_view=str(OLD), new_view=str(VIEW), private_git_dir=str(GITDIR),
                 private_objects_alternate=str(common / 'objects'),
                 old_HEAD_and_index_sha256_unchanged=protected,
                 unchanged_hardlinks=1847, exclusive_changed_files=3,
                 all_materialized_files=1850, complete_tree_leaf_count=len(after_tree),
                 not_materialized_tree_leaf_count=len(after_tree) - 1850,
                 required_file_projection=True, full_checkout=False,
                 future_pin_updates='Atomic replacement of new-view files only; never in-place writes to hardlinks.',
                 original_guard_cli='not_run_waiting_for_fixed_R2_G2',
                 compile_native_and_controls='not_run', formal_task_completion=False,
                 formal_task_counts=dict(done=163, remaining=29),
                 source_script=dict(path=str(Path(__file__).resolve()), sha256=sha(Path(__file__).read_bytes())))
    record = write('actual-P2-independent-proof.json', proof)
    print(json.dumps({'stage': 'complete', 'proof': record, 'view': str(VIEW)}), flush=True)


if __name__ == '__main__':
    main()
