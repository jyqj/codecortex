#!/usr/bin/env python3
"""Prepare an isolated exact296 base and compute prospective archive union trees.

No guard runs, network operations, shared-file edits or claims of complete merged
materialization. Missing objects remain explicitly enumerated.
"""
import hashlib
import json
import os
import pathlib
import subprocess

RAM = pathlib.Path('/dev/shm/a217aaae3bde')
SOURCE = RAM / 'codecortex'
OUT = RAM / 'prospective-combined-tree-prep'
META = pathlib.Path('/workspace/scratch/a217aaae3bde/prospective-combined-tree-prep')
WORK = OUT / 'checkout'
GIT = OUT / 'git'
HEAD = '29682890c89511dd6f477a6bf48bd969aa1537af'
BASE = '7354db236c9d9850a75f31672697ae9eab44565e'
F88 = 'f88a41e6af46ffe67b76f420f1d6b4ef21155946'

def h(raw): return hashlib.sha256(raw).hexdigest()
def git_obj(kind, raw): return hashlib.sha1((kind + ' ' + str(len(raw)) + '\0').encode() + raw).hexdigest()
def original(*args): return subprocess.check_output(['git', '-C', str(SOURCE), *args])
def private(*args, raw=None): return subprocess.check_output(['git', '--git-dir=' + str(GIT), '--work-tree=' + str(WORK), *args], input=raw)
def fingerprint():
    index = SOURCE / '.git/index'
    return {'HEAD': original('rev-parse', 'HEAD').decode().strip(),
            'refs_sha256': h(original('show-ref')), 'index_sha256': h(index.read_bytes())}
def put(kind, raw, expected=None):
    identity = git_obj(kind, raw)
    if expected is not None: assert identity == expected, (kind, identity, expected)
    assert private('hash-object', '-t', kind, '-w', '--stdin', raw=raw).decode().strip() == identity
    return identity
def tree_body(entries):
    # Git tree name sorting includes an implicit slash for directories.
    rows = sorted(entries, key=lambda e: (e['path'] + ('/' if e['type'] == 'tree' else '')).encode())
    return b''.join((str(int(e['mode'])) + ' ' + e['path']).encode() + b'\0' + bytes.fromhex(e['sha']) for e in rows)
def parse_tree(treeish):
    rows=[]
    for row in original('ls-tree', '-z', treeish).split(b'\0'):
        if not row: continue
        desc,path=row.split(b'\t'); mode,kind,sha=desc.decode().split()
        rows.append({'path':path.decode(),'mode':mode,'type':kind,'sha':sha})
    return rows

before=fingerprint(); assert before['HEAD']==HEAD
assert not OUT.exists(), 'private preparation directory must be new'
OUT.mkdir(); WORK.mkdir()
subprocess.check_call(['git','init','--quiet','--separate-git-dir='+str(GIT),str(WORK)])
(GIT/'objects/info/alternates').write_text(str(SOURCE/'.git/objects')+'\n')
private('config','core.sharedRepository','false')
private('config','gc.auto','0')
# This private checkout HEAD remains the exact base. Prospective tree is separately named.
(GIT/'HEAD').write_text(HEAD+'\n')
private('read-tree',HEAD)
inventory=json.loads((META/'object-and-space-inventory.json').read_bytes())
linked=[]
for entry in inventory['base_files']:
    source=SOURCE/entry['path']; target=WORK/entry['path']; info=source.lstat()
    assert source.is_file() and not source.is_symlink()
    raw=source.read_bytes()
    assert len(raw)==entry['size'] and git_obj('blob',raw)==entry['sha']
    assert bool(info.st_mode & 0o111)==(entry['mode']=='100755')
    target.parent.mkdir(parents=True,exist_ok=True)
    os.link(source,target)
    assert target.stat().st_ino==info.st_ino and target.stat().st_dev==info.st_dev
    linked.append({'path':entry['path'],'sha':entry['sha'],'bytes':len(raw),'mode':entry['mode'],
                   'inode':info.st_ino,'device':info.st_dev})

# Reconstruct official changed trees from complete recursive metadata and verify SHA.
changed=json.loads((META/'changed-checkpoint-subtrees.json').read_bytes())
for item in changed:
    data=json.loads((META/'official-trees'/(item['sha']+'.json')).read_bytes())
    assert data['sha']==item['sha'] and data['truncated'] is False
    directories={'':item['sha']}
    directories.update({e['path']:e['sha'] for e in data['tree'] if e['type']=='tree'})
    for prefix,expected in sorted(directories.items(),key=lambda p:p[0].count('/'),reverse=True):
        rows=[]
        for e in data['tree']:
            parent,_,leaf=e['path'].rpartition('/')
            if parent==prefix: rows.append(dict(e,path=leaf))
        put('tree',tree_body(rows),expected)

official=json.loads((RAM/'platform-review/main-341-drift/pr165-official-result-trees.json').read_bytes())
base_root={e['path']:e for e in parse_tree(BASE)}
for e in official['root']['tree']:
    if e['path']!='artifacts':
        assert e['sha']==base_root[e['path']]['sha'] and e['mode']==base_root[e['path']]['mode']
base_checkpoints={e['path']:e for e in parse_tree(BASE+':artifacts/checkpoints')}
main_checkpoints={e['path']:e for e in official['checkpoints']['tree']}
assert set(base_checkpoints)<=set(main_checkpoints)
assert all(main_checkpoints[k]['sha']==v['sha'] and main_checkpoints[k]['mode']==v['mode'] for k,v in base_checkpoints.items())
assert set(main_checkpoints)-set(base_checkpoints)=={e['path'] for e in changed}
for section in ['checkpoints','artifacts','root','archive']:
    put('tree',tree_body(official[section]['tree']),official[section]['sha'])
for data in official['archive_children'].values():
    put('tree',tree_body(data['tree']),data['sha'])

# Main adds 16 entirely new child paths versus7354; keep every296 path, including its proof.
union={e['path']:e for e in parse_tree(HEAD+':artifacts/checkpoints')}
assert not set(union).intersection(e['path'] for e in changed)
union.update({e['path']:e for e in changed})
checkpoints=put('tree',tree_body(list(union.values())))
artifacts={e['path']:e for e in parse_tree(HEAD+':artifacts')}
artifacts['checkpoints']['sha']=checkpoints
artifacts_tree=put('tree',tree_body(list(artifacts.values())))
root={e['path']:e for e in parse_tree(HEAD)}; root['artifacts']['sha']=artifacts_tree
prospective=put('tree',tree_body(list(root.values())))
assert not (WORK/'artifacts/checkpoints/localwidth4-100k-paired-20261003').exists()
assert private('write-tree').decode().strip()==original('rev-parse',HEAD+'^{tree}').decode().strip()
assert fingerprint()==before

record={'schema':'prospective-combined-tree-private-base-preparation-v1',
    'status':'exact296_base_prepared_prospective_archive_materialization_blocked',
    'root_fingerprint_before_after':before,'private_git_dir':str(GIT),'private_worktree':str(WORK),
    'private_HEAD_and_index':HEAD,'read_only_alternate':str(SOURCE/'.git/objects'),
    'shared_tracked_files_linked':len(linked),'linked_logical_bytes':sum(e['bytes'] for e in linked),
    'all_linked_file_bytes_blob_modes_and_inode_identity_verified':True,
    'linked_files_must_never_be_modified_or_chmodded':True,
    'prospective_parents':[HEAD,F88],'merge_base':BASE,
    'prospective_tree':prospective,'prospective_checkpoints_tree':checkpoints,
    'main_change_is_only16_new_checkpoint_subtrees_relative_to_merge_base':True,
    'complete_combined_checkout_materialized':False,'main_PR165_merge_observed':False,
    'guard_executed':False,'known_missing_new_blobs':len(inventory['missing_new_blobs']),
    'known_missing_new_blob_bytes':sum(x['size'] for x in inventory['missing_new_blobs']),
    'missing_binary_objects_cannot_be_replaced_by_placeholders':True,
    'root_HEAD_refs_index_and_all_linked_contents_unchanged':True,
    'task_statuses_changed':False,'done':163,'remaining':29}
(META/'linked-base-files.json').write_text(json.dumps(linked,indent=2)+'\n')
(META/'private-base-preparation.json').write_text(json.dumps(record,sort_keys=True,indent=2)+'\n')
print(json.dumps(record,sort_keys=True))
