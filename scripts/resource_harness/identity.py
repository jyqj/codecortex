"""Identity and permission prechecks; no mutation of cgroups or source checkouts."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess

SOURCES = dict(baseline='513a98c9a94b15ec77153df41af26fa3c8c0b5e8',
               candidate='e3c04fed903c4d0e3c26b5d7cf0e055a6f7c5207')
PREPARATION_SHA256 = 'a76865a466252f5863c8e0ad278c4c6814093d4a4a629a9409acf2e6b916901b'


def digest(path):
    h = hashlib.sha256()
    with Path(path).open('rb') as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b''):
            h.update(chunk)
    return h.hexdigest()


def git(root, *args):
    return subprocess.check_output(['git', *args], cwd=root, text=True).strip()


def driver_identity():
    directory = Path(__file__).resolve().parent
    return {p.name: digest(p) for p in sorted(directory.glob('*.py'))} | {
        'protocol-manifest.json': digest(directory / 'protocol-manifest.json')}


def checkout_identity(root, variant):
    root = Path(root).resolve()
    sha = SOURCES[variant]
    if git(root, 'rev-parse', 'HEAD') != sha or git(root, 'status', '--porcelain'):
        raise RuntimeError(f'{variant}: exact clean source checkout required at {sha}')
    if digest(root / 'scripts/p7_release_resource_preparation.py') != PREPARATION_SHA256:
        raise RuntimeError('preparation source hash mismatch')
    # Hash build/script inputs, not historical logs or private diagnostic copies.
    entries = subprocess.check_output(['git', 'ls-tree', '-r', '-z', 'HEAD', '--',
                                     'Cargo.toml', 'Cargo.lock', 'crates', 'scripts', '.cargo'], cwd=root)
    hashes = {}
    algorithm = git(root, 'rev-parse', '--show-object-format')
    for entry in entries.split(b'\0'):
        if not entry:
            continue
        meta, raw_name = entry.split(b'\t', 1)
        mode, kind, object_id = meta.split()
        if kind != b'blob':
            raise RuntimeError('build inputs must not contain unbound submodules')
        name = os.fsdecode(raw_name)
        path = root / name
        data = os.fsencode(os.readlink(path)) if mode == b'120000' else path.read_bytes()
        actual_blob = hashlib.new(algorithm, b'blob ' + str(len(data)).encode() + b'\0' + data).hexdigest()
        if actual_blob != object_id.decode():
            raise RuntimeError(f'{variant}: pinned source bytes differ: {name}')
        hashes[name] = hashlib.sha256(data).hexdigest()
    return dict(source_sha=sha, source_tree=git(root, 'rev-parse', 'HEAD^{tree}'),
                source_root=str(root), source_files_sha256=hashes)


def validate_receipt(receipt, variant):
    actual = checkout_identity(receipt['source_root'], variant)
    for key in actual:
        if receipt.get(key) != actual[key]:
            raise RuntimeError(f'{variant}: receipt {key} no longer matches source')
    artifact = receipt['compiler_artifact']
    if (receipt['build_exit_code'] != 0 or receipt.get('guard_stop') is not None
            or receipt['profile'] != 'release' or '--release' not in receipt['command']
            or '--locked' not in receipt['command']
            or sorted(artifact['features']) != ['semantic', 'semantic-http']
            or artifact['profile']['opt_level'] != '3'
            or artifact['profile']['debug_assertions']
            or digest(artifact['executable']) != receipt['binary_sha256']):
        raise RuntimeError(f'{variant}: invalid release artifact receipt')


def precheck(group, output_parent):
    """Real file/socket errors propagate. Never probe by launching the product."""
    from .runtime import memory_snapshot
    group = Path(group).resolve()
    sample = memory_snapshot(group, scope='shared_or_unknown')
    for key in ('memory_current_bytes', 'members'):
        if sample[key]['error']:
            raise RuntimeError(f'required guard/group identity unreadable: {sample[key]["error"]}')
    if sample['memory_current_bytes']['value'] >= 12 * 1024**3:
        raise RuntimeError('initial group usage must be below 12GiB')
    if shutil.disk_usage(output_parent).free < 4 * 1024**3:
        raise RuntimeError('disk reserve below 4GiB')
    if not os.access(output_parent, os.W_OK):
        raise PermissionError(f'output parent not writable: {output_parent}')
    try:
        sample['harness_cgroup_membership'] = Path('/proc/self/cgroup').read_text()
        sample['cgroup_mounts'] = [line for line in Path('/proc/self/mountinfo').read_text().splitlines()
                                 if ' - cgroup2 ' in line or ' - cgroup ' in line]
    except OSError as exc:
        sample['membership_visibility_error'] = repr(exc)
    sample['harness_listed_in_direct_members'] = os.getpid() in sample['members']['value']
    sample['group_independence'] = 'unknown'
    sample['group_relation'] = 'direct_member_observed' if sample['harness_listed_in_direct_members'] else 'unknown'
    return sample


def compiler_scan():
    """Best effort full cmdline scan; missing visibility cannot certify no build."""
    visible, errors = [], []
    for process in Path('/proc').iterdir():
        if not process.name.isdigit():
            continue
        try:
            parts = (process / 'cmdline').read_bytes().split(b'\0')
            executable = Path(os.fsdecode(parts[0])).name
            if executable == 'rustc' or (executable == 'cargo' and b'build' in parts):
                visible.append(dict(pid=int(process.name), command=[os.fsdecode(p) for p in parts if p]))
        except OSError as exc:
            # Process disappearance is ordinary; denied visibility is material.
            if process.exists():
                errors.append(dict(pid=int(process.name), error=repr(exc)))
    if visible or errors:
        raise RuntimeError(f'cannot certify builds finished: {visible}; visibility errors: {errors}')
    return dict(no_compiler_process_observed=True, visibility='current proc mount only')
